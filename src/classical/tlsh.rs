//! TLSH (Trend Micro Locality-Sensitive Hash) fingerprinter.
//!
//! Wraps the [`tlsh2`] crate. TLSH is a fixed-size locality-sensitive
//! hash designed for malware/document similarity. Unlike MinHash and
//! SimHash, TLSH operates on **raw bytes**, not tokens — its trigram
//! sliding window does its own structural analysis.
//!
//! # Choosing the variant
//!
//! Two `TlshBuilder` configurations are commonly used:
//!
//! - **128/1 (default)**: 32-byte body + 1-byte checksum, 50-byte
//!   minimum input. Fast, suitable for documents ≥ 50 bytes.
//! - **256/3**: 64-byte body + 3-byte checksum, 50-byte minimum input.
//!   Roughly 2× the bits, lower false-positive rate, slightly slower.
//!
//! `TlshFingerprinter::default()` uses 128/1.
//!
//! # See also
//!
//! - Oliver, Cheng, Chen, Forman (2013). *TLSH — A Locality Sensitive Hash.*

use alloc::format;
use alloc::string::String;
use core::str::FromStr;

use tlsh2::{Tlsh128_1, TlshBuilder128_1};

use crate::canonical::Canonicalizer;
use crate::classical::Fingerprinter;
use crate::error::{Error, Result};
use crate::fingerprint::TlshFingerprint;

/// Minimum input length accepted by the default TLSH builder, in bytes.
///
/// Inputs smaller than this return [`Error::InvalidInput`].
pub const MIN_INPUT_BYTES: usize = 50;

/// Offline TLSH fingerprinter.
///
/// Cheap to clone (zero-sized aside from a [`Canonicalizer`]).
#[derive(Clone, Debug, Default)]
pub struct TlshFingerprinter {
    canonicalizer: Canonicalizer,
}

impl TlshFingerprinter {
    /// Construct with the supplied canonicalizer.
    #[must_use]
    pub fn new(canonicalizer: Canonicalizer) -> Self {
        Self { canonicalizer }
    }

    /// Borrow the canonicalizer.
    #[must_use]
    pub fn canonicalizer(&self) -> &Canonicalizer {
        &self.canonicalizer
    }

    /// Sketch raw bytes (no canonicalization). Useful when the caller
    /// has already canonicalized or is fingerprinting non-text bytes.
    pub fn sketch_bytes(&self, bytes: &[u8]) -> Result<TlshFingerprint> {
        if bytes.len() < MIN_INPUT_BYTES {
            return Err(Error::InvalidInput(format!(
                "tlsh requires at least {MIN_INPUT_BYTES} bytes, got {}",
                bytes.len()
            )));
        }
        let mut builder = TlshBuilder128_1::new();
        builder.update(bytes);
        let tlsh: Tlsh128_1 = builder.build().ok_or_else(|| {
            Error::InvalidInput("tlsh build failed (insufficient entropy)".into())
        })?;
        let hex_bytes = tlsh.hash();
        let hex = String::from_utf8(hex_bytes.to_vec())
            .map_err(|e| Error::InvalidInput(format!("tlsh hash not ASCII: {e}")))?;
        Ok(TlshFingerprint { hex })
    }
}

impl Fingerprinter for TlshFingerprinter {
    type Output = TlshFingerprint;

    fn fingerprint(&self, input: &str) -> Result<Self::Output> {
        if input.is_empty() {
            return Err(Error::InvalidInput("empty document".into()));
        }
        let canonical = self.canonicalizer.canonicalize(input);
        self.sketch_bytes(canonical.as_bytes())
    }
}

/// TLSH similarity distance between two fingerprints.
///
/// Returns [`Error::InvalidInput`] if either hex string fails to parse.
/// Lower scores mean more similar; the literature treats `< 50` as
/// "high similarity" for the 128/1 variant.
pub fn tlsh_distance(a: &TlshFingerprint, b: &TlshFingerprint) -> Result<i32> {
    let parsed_a = parse(&a.hex)?;
    let parsed_b = parse(&b.hex)?;
    Ok(parsed_a.diff(&parsed_b, true))
}

fn parse(hex: &str) -> Result<Tlsh128_1> {
    Tlsh128_1::from_str(hex).map_err(|_| {
        Error::InvalidInput(format!("invalid TLSH hex string of length {}", hex.len()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn long_text() -> String {
        // ≥ 50 bytes with enough entropy for TLSH to build.
        let s = "the quick brown fox jumps over the lazy dog at noon today
the slow grey wolf creeps under the loud ravens at dusk
astronomers detect cosmic background radiation in the night sky
once upon a time in a galaxy far far away there lived a hero";
        s.to_string()
    }

    #[test]
    fn rejects_short_input() {
        let f = TlshFingerprinter::default();
        let r = f.fingerprint("short");
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn deterministic_on_long_input() {
        let f = TlshFingerprinter::default();
        let txt = long_text();
        let a = f.fingerprint(&txt).unwrap();
        let b = f.fingerprint(&txt).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn hex_is_well_formed() {
        let f = TlshFingerprinter::default();
        let s = f.fingerprint(&long_text()).unwrap();
        // 128/1 variant produces a 72-character hex string (incl. "T1" prefix).
        assert!(s.hex.is_ascii());
        assert!(s.hex.len() >= 70);
        assert!(s.hex.chars().all(|c| c.is_ascii_alphanumeric()));
    }

    #[test]
    fn similar_docs_have_small_distance() {
        let f = TlshFingerprinter::default();
        let a = f.fingerprint(&long_text()).unwrap();
        // Same content with one minor edit.
        let edited = long_text().replace("noon", "dusk");
        let b = f.fingerprint(&edited).unwrap();
        let d = tlsh_distance(&a, &b).unwrap();
        assert!(d < 200, "similar docs should have small distance, got {d}");
    }

    #[test]
    fn different_docs_have_large_distance() {
        let f = TlshFingerprinter::default();
        let a = f.fingerprint(&long_text()).unwrap();
        let other = "completely unrelated paragraph about astronomy and stars
where the cosmic background radiation tells us about the early universe
and quantum mechanics weaves together space and time in unexpected ways
many particle physicists spend their careers searching for new particles"
            .to_string();
        let b = f.fingerprint(&other).unwrap();
        let d = tlsh_distance(&a, &b).unwrap();
        assert!(
            d > 50,
            "unrelated docs should have larger distance, got {d}"
        );
    }

    #[test]
    fn sketch_bytes_works_on_raw_input() {
        let f = TlshFingerprinter::default();
        let txt = long_text();
        let from_str = f.fingerprint(&txt).unwrap();
        // sketch_bytes on canonicalized bytes should match the fingerprint().
        let canonical = f.canonicalizer().canonicalize(&txt);
        let from_bytes = f.sketch_bytes(canonical.as_bytes()).unwrap();
        assert_eq!(from_str, from_bytes);
    }

    #[test]
    fn parse_rejects_garbage_hex() {
        let bad = TlshFingerprint {
            hex: "nothex".into(),
        };
        let good = TlshFingerprinter::default()
            .fingerprint(&long_text())
            .unwrap();
        assert!(tlsh_distance(&bad, &good).is_err());
    }
}
