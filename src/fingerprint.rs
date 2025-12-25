//! Unified [`Fingerprint`] enum + metadata.
//!
//! Most callers work with concrete signature types ([`MinHashSig`],
//! [`SimHash64`], [`Embedding`]). The [`Fingerprint`] enum exists for
//! the cross-modal `ucfp` integrator: it lets a single column in a
//! database hold any signature variant while preserving variant-aware
//! similarity routing.
//!
//! [`MinHashSig`]: crate::classical::minhash::MinHashSig
//! [`SimHash64`]: crate::classical::simhash::SimHash64
//! [`Embedding`]: crate::semantic::Embedding

use alloc::format;
use alloc::string::String;

use crate::canonical::Canonicalizer;

/// Stable algorithm identifier embedded in [`FingerprintMetadata::algorithm`].
///
/// These identifiers are part of the v0.1.0 stable API.
pub mod algo {
    /// MinHash with `H = 128` slots.
    pub const MINHASH_128: &str = "minhash-h128";
    /// MinHash with arbitrary `H`. The integrator pulls H out of metadata.
    pub const MINHASH: &str = "minhash";
    /// SimHash with 64-bit width.
    pub const SIMHASH_64: &str = "simhash-b64";
    /// TLSH (when the `tlsh` feature is enabled).
    pub const TLSH: &str = "tlsh";
    /// Semantic embedding produced by an `EmbeddingProvider` (semantic feature).
    pub const EMBEDDING: &str = "embedding";
}

/// Cross-variant fingerprint container.
///
/// `Fingerprint` is `Clone + PartialEq + Debug` but **not**
/// `bytemuck::Pod` — the inner signatures are. UCFP typically
/// `match`es the variant, then `bytemuck::cast_slice`s the inner array.
///
/// The variants have very different sizes — `MinHash(MinHashSig<128>)`
/// is ~1 KiB while `SimHash(SimHash64)` is 8 bytes. The size disparity
/// is intentional: the enum is meant to hold a single fingerprint at
/// rest, where allocator overhead from boxing is worse than the
/// per-fingerprint memory cost.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Fingerprint {
    /// MinHash with default `H = 128`.
    #[cfg(feature = "minhash")]
    #[cfg_attr(docsrs, doc(cfg(feature = "minhash")))]
    MinHash(crate::classical::minhash::MinHashSig<128>),

    /// SimHash 64-bit.
    #[cfg(feature = "simhash")]
    #[cfg_attr(docsrs, doc(cfg(feature = "simhash")))]
    SimHash(crate::classical::simhash::SimHash64),

    /// TLSH 48-byte body. The exact wrapped type is opaque to keep the
    /// public surface stable across `tlsh2` upgrades within v0.1.x.
    #[cfg(feature = "tlsh")]
    #[cfg_attr(docsrs, doc(cfg(feature = "tlsh")))]
    Tlsh(TlshFingerprint),

    /// Semantic embedding from an [`crate::semantic::EmbeddingProvider`].
    #[cfg(feature = "semantic")]
    #[cfg_attr(docsrs, doc(cfg(feature = "semantic")))]
    Embedding(crate::semantic::Embedding),
}

/// Wrapper around `tlsh2`'s 48-byte body fingerprint, kept opaque so
/// that internal type churn in the upstream crate does not break us.
#[cfg(feature = "tlsh")]
#[cfg_attr(docsrs, doc(cfg(feature = "tlsh")))]
#[derive(Clone, Debug, PartialEq)]
pub struct TlshFingerprint {
    /// Hex-encoded TLSH body, exactly 70 ASCII characters.
    pub hex: alloc::string::String,
}

/// Metadata describing how a [`Fingerprint`] was produced.
///
/// Used by UCFP and any other consumer to refuse comparisons across
/// incompatible canonicalizer / tokenizer / algorithm configurations.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FingerprintMetadata {
    /// Stable algorithm identifier from [`algo`].
    pub algorithm: &'static str,
    /// Hash of the canonicalizer + tokenizer + algorithm config string,
    /// produced by [`config_hash`]. Two fingerprints with different
    /// `config_hash` values must not be compared.
    pub config_hash: u64,
    /// Model identifier for embedding fingerprints; `None` for classical
    /// fingerprints.
    pub model_id: Option<String>,
    /// Schema version embedded in the inner signature (matches
    /// [`crate::classical::minhash::SCHEMA_VERSION`] etc.).
    pub schema_version: u16,
    /// Wire size of the underlying signature in bytes.
    pub byte_size: usize,
}

impl Fingerprint {
    /// Produce a metadata record for this fingerprint.
    ///
    /// Cheap; does not re-hash the signature.
    #[must_use]
    pub fn metadata(&self) -> FingerprintMetadata {
        match self {
            #[cfg(feature = "minhash")]
            Fingerprint::MinHash(sig) => FingerprintMetadata {
                algorithm: algo::MINHASH_128,
                config_hash: 0,
                model_id: None,
                schema_version: sig.schema,
                byte_size: sig.as_bytes().len(),
            },
            #[cfg(feature = "simhash")]
            Fingerprint::SimHash(sig) => FingerprintMetadata {
                algorithm: algo::SIMHASH_64,
                config_hash: 0,
                model_id: None,
                schema_version: crate::classical::simhash::SCHEMA_VERSION,
                byte_size: sig.as_bytes().len(),
            },
            #[cfg(feature = "tlsh")]
            Fingerprint::Tlsh(tlsh) => FingerprintMetadata {
                algorithm: algo::TLSH,
                config_hash: 0,
                model_id: None,
                schema_version: 1,
                byte_size: tlsh.hex.len(),
            },
            #[cfg(feature = "semantic")]
            Fingerprint::Embedding(emb) => FingerprintMetadata {
                algorithm: algo::EMBEDDING,
                config_hash: 0,
                model_id: emb.model_id.clone(),
                schema_version: 1,
                byte_size: emb.vector.len() * core::mem::size_of::<f32>(),
            },
        }
    }

    /// Stable display name like `"minhash-h128-cfg=<cfg>-v1"` used by
    /// UCFP for compatibility checks.
    #[must_use]
    pub fn name(&self) -> String {
        match self {
            #[cfg(feature = "minhash")]
            Fingerprint::MinHash(sig) => format!("{}-v{}", algo::MINHASH_128, sig.schema),
            #[cfg(feature = "simhash")]
            Fingerprint::SimHash(_) => format!(
                "{}-v{}",
                algo::SIMHASH_64,
                crate::classical::simhash::SCHEMA_VERSION
            ),
            #[cfg(feature = "tlsh")]
            Fingerprint::Tlsh(_) => format!("{}-v1", algo::TLSH),
            #[cfg(feature = "semantic")]
            Fingerprint::Embedding(emb) => match &emb.model_id {
                Some(m) => format!("{}/{m}-v1", algo::EMBEDDING),
                None => format!("{}-v1", algo::EMBEDDING),
            },
        }
    }
}

/// Hash a canonicalizer + tokenizer + algorithm-specific config string
/// into the `config_hash` field of [`FingerprintMetadata`].
///
/// Uses `xxh3_64` for speed and stability across builds.
#[must_use]
pub fn config_hash(canonicalizer: &Canonicalizer, tokenizer_name: &str, algo_cfg: &str) -> u64 {
    let mut buf = String::with_capacity(64);
    buf.push_str(canonicalizer.config_string().as_str());
    buf.push('|');
    buf.push_str(tokenizer_name);
    buf.push('|');
    buf.push_str(algo_cfg);
    xxhash_rust::xxh3::xxh3_64(buf.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_hash_is_deterministic() {
        let c = Canonicalizer::default();
        let a = config_hash(&c, "word-uax29", "minhash-h128-mmh3");
        let b = config_hash(&c, "word-uax29", "minhash-h128-mmh3");
        assert_eq!(a, b);
    }

    #[test]
    fn config_hash_differs_on_change() {
        let c = Canonicalizer::default();
        let a = config_hash(&c, "word-uax29", "minhash-h128-mmh3");
        let b = config_hash(&c, "grapheme-uax29", "minhash-h128-mmh3");
        assert_ne!(a, b);
    }

    #[cfg(feature = "minhash")]
    #[test]
    fn minhash_metadata_round_trip() {
        let sig = crate::classical::minhash::MinHashSig::<128>::empty();
        let fp = Fingerprint::MinHash(sig);
        let md = fp.metadata();
        assert_eq!(md.algorithm, algo::MINHASH_128);
        assert!(md.byte_size >= 128 * 8);
        assert_eq!(md.schema_version, 1);
    }

    #[cfg(feature = "simhash")]
    #[test]
    fn simhash_metadata_round_trip() {
        use crate::classical::simhash::SimHash64;
        let fp = Fingerprint::SimHash(SimHash64::new(0xDEADBEEF));
        let md = fp.metadata();
        assert_eq!(md.algorithm, algo::SIMHASH_64);
        assert_eq!(md.byte_size, 8);
    }

    #[cfg(feature = "minhash")]
    #[test]
    fn minhash_name_includes_schema() {
        let sig = crate::classical::minhash::MinHashSig::<128>::empty();
        let n = Fingerprint::MinHash(sig).name();
        assert_eq!(n, "minhash-h128-v1");
    }

    #[cfg(feature = "simhash")]
    #[test]
    fn simhash_name_is_stable() {
        use crate::classical::simhash::SimHash64;
        let n = Fingerprint::SimHash(SimHash64::new(0)).name();
        assert_eq!(n, "simhash-b64-v1");
    }
}
