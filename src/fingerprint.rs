//! Unified [`Fingerprint`] enum + metadata.
//!
//! Most callers work with concrete signature types ([`MinHashSig`],
//! [`SimHash64`], [`Embedding`]). The [`Fingerprint`] enum exists for
//! the cross-modal `ucfp` integrator: it lets a single column in a
//! database hold any signature variant while preserving variant-aware
//! similarity routing.
//!
//! # Cross-modal compatibility
//!
//! `txtfp`'s [`crate::FORMAT_VERSION`] mirrors the same constant in
//! `audiofp` and `imgfprint`. The integrator checks the trio agree
//! before opening a database:
//!
//! ```ignore
//! assert_eq!(audiofp::FORMAT_VERSION, txtfp::FORMAT_VERSION);
//! assert_eq!(imgfprint::FORMAT_VERSION, txtfp::FORMAT_VERSION);
//! ```
//!
//! Per-signature schema versions (e.g.
//! [`crate::classical::minhash::SCHEMA_VERSION`]) live alongside the
//! crate-wide constant for finer-grained migration tracking.
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
/// `bytemuck::Pod` — the inner signatures are. The integrator typically
/// `match`es the variant, then uses [`bytemuck::cast_slice`] to access
/// the inner array as bytes.
///
/// # Example: bulk persist a column of MinHash signatures
///
/// ```
/// # #[cfg(feature = "minhash")] {
/// use txtfp::{Fingerprint, MinHashSig};
///
/// let sigs: Vec<MinHashSig<128>> = (0..3)
///     .map(|_| MinHashSig::<128>::empty())
///     .collect();
///
/// // Zero-copy view of the entire column as a single contiguous byte slice.
/// let bytes: &[u8] = bytemuck::cast_slice(&sigs);
/// // Each signature occupies 8 + 8 * 128 = 1032 bytes.
/// assert_eq!(bytes.len(), sigs.len() * 1032);
///
/// // Round-trip back to a typed slice.
/// let view: &[MinHashSig<128>] = bytemuck::cast_slice(bytes);
/// assert_eq!(view.len(), sigs.len());
///
/// // For the enum, match the variant first.
/// let fp = Fingerprint::MinHash(sigs[0]);
/// match &fp {
///     Fingerprint::MinHash(s) => {
///         let _: &[u8] = bytemuck::bytes_of(s);
///     }
///     _ => unreachable!(),
/// }
/// # }
/// ```
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

/// Sentinel value for [`FingerprintMetadata::config_hash`] meaning
/// "this metadata was produced without a canonicalizer / tokenizer /
/// algorithm-config triple in scope, so the hash is not authoritative".
///
/// Two metadata records with `config_hash == UNCOMPUTED_CONFIG_HASH`
/// **must not** be assumed compatible solely on the basis of equal
/// `algorithm` strings — go read the canonicalizer & tokenizer config
/// from the producer.
pub const UNCOMPUTED_CONFIG_HASH: u64 = 0;

/// Metadata describing how a [`Fingerprint`] was produced.
///
/// Used by UCFP and any other consumer to refuse comparisons across
/// incompatible canonicalizer / tokenizer / algorithm configurations.
///
/// # `config_hash` lifecycle
///
/// `Fingerprint::metadata()` cannot reach the canonicalizer or tokenizer
/// (they aren't stored in the enum), so it sets `config_hash` to
/// [`UNCOMPUTED_CONFIG_HASH`]. Two production paths populate it:
///
/// 1. **Producer-side**: at fingerprinting time, call
///    [`Fingerprint::metadata_with`] with the canonicalizer +
///    tokenizer name + algorithm config string used to produce the
///    fingerprint. This is the recommended path.
/// 2. **Caller-side**: compute via [`config_hash`] and attach via
///    [`FingerprintMetadata::with_config_hash`].
///
/// Persisting fingerprints without their `config_hash` (or a sidecar
/// record of the producing config) is supported but means cross-config
/// comparisons cannot be statically refused — the caller must
/// re-derive the hash before comparing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FingerprintMetadata {
    /// Stable algorithm identifier from [`algo`].
    pub algorithm: &'static str,
    /// Hash of the canonicalizer + tokenizer + algorithm config string,
    /// produced by [`config_hash`]. Two fingerprints with different
    /// non-zero `config_hash` values must not be compared.
    /// [`UNCOMPUTED_CONFIG_HASH`] means "not yet populated".
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

impl FingerprintMetadata {
    /// Attach a previously-computed `config_hash` to an existing metadata
    /// record. Useful when the canonicalizer + tokenizer + algorithm
    /// triple is known in a different scope from the metadata
    /// construction site.
    #[must_use]
    pub fn with_config_hash(mut self, hash: u64) -> Self {
        self.config_hash = hash;
        self
    }
}

impl Fingerprint {
    /// Produce a metadata record for this fingerprint.
    ///
    /// Cheap; does not re-hash the signature. The `config_hash` field is
    /// set to [`UNCOMPUTED_CONFIG_HASH`] — see the
    /// [`FingerprintMetadata`] docs for how to populate it. If you
    /// already have the canonicalizer and tokenizer in scope, prefer
    /// [`Fingerprint::metadata_with`].
    #[must_use]
    pub fn metadata(&self) -> FingerprintMetadata {
        self.metadata_inner(UNCOMPUTED_CONFIG_HASH)
    }

    /// Produce a metadata record with `config_hash` computed from the
    /// supplied canonicalizer, tokenizer name, and algorithm-specific
    /// config string. Recommended path for producer code.
    #[must_use]
    pub fn metadata_with(
        &self,
        canonicalizer: &Canonicalizer,
        tokenizer_name: &str,
        algo_cfg: &str,
    ) -> FingerprintMetadata {
        let hash = config_hash(canonicalizer, tokenizer_name, algo_cfg);
        self.metadata_inner(hash)
    }

    fn metadata_inner(&self, config_hash: u64) -> FingerprintMetadata {
        match self {
            #[cfg(feature = "minhash")]
            Fingerprint::MinHash(sig) => FingerprintMetadata {
                algorithm: algo::MINHASH_128,
                config_hash,
                model_id: None,
                schema_version: sig.schema,
                byte_size: sig.as_bytes().len(),
            },
            #[cfg(feature = "simhash")]
            Fingerprint::SimHash(sig) => FingerprintMetadata {
                algorithm: algo::SIMHASH_64,
                config_hash,
                model_id: None,
                schema_version: crate::classical::simhash::SCHEMA_VERSION,
                byte_size: sig.as_bytes().len(),
            },
            #[cfg(feature = "tlsh")]
            Fingerprint::Tlsh(tlsh) => FingerprintMetadata {
                algorithm: algo::TLSH,
                config_hash,
                model_id: None,
                schema_version: 1,
                byte_size: tlsh.hex.len(),
            },
            #[cfg(feature = "semantic")]
            Fingerprint::Embedding(emb) => FingerprintMetadata {
                algorithm: algo::EMBEDDING,
                config_hash,
                model_id: emb.model_id.clone(),
                schema_version: 1,
                byte_size: emb.vector.len() * core::mem::size_of::<f32>(),
            },
        }
    }

    /// Stable display name for the fingerprint, suitable for storage
    /// keys and routing decisions.
    ///
    /// Format (frozen for v0.1.x):
    /// - MinHash: `"minhash-h128-v{schema}"`
    /// - SimHash: `"simhash-b64-v{schema}"`
    /// - TLSH: `"tlsh-v1"`
    /// - Embedding: `"embedding/{model_id}-v1"` if `model_id.is_some()`,
    ///   otherwise `"embedding-v1"`
    ///
    /// **Does not** include `config_hash` in the rendered string —
    /// `config_hash` lives on [`FingerprintMetadata`] and is populated
    /// out-of-band (see [`Fingerprint::metadata_with`]). If you need a
    /// fully-qualified key including config disambiguation, build it
    /// yourself:
    ///
    /// ```
    /// # #[cfg(feature = "minhash")] {
    /// use txtfp::{Canonicalizer, Fingerprint, MinHashSig, config_hash};
    ///
    /// let fp = Fingerprint::MinHash(MinHashSig::<128>::empty());
    /// let cfg = config_hash(&Canonicalizer::default(), "word-uax29", "h128-mmh3");
    /// let key = format!("{}-cfg={cfg:016x}", fp.name());
    /// # }
    /// ```
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
        assert_eq!(md.config_hash, UNCOMPUTED_CONFIG_HASH);
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
    fn metadata_with_populates_config_hash() {
        let sig = crate::classical::minhash::MinHashSig::<128>::empty();
        let fp = Fingerprint::MinHash(sig);
        let canon = Canonicalizer::default();
        let md = fp.metadata_with(&canon, "word-uax29", "h128-mmh3");
        let expected = config_hash(&canon, "word-uax29", "h128-mmh3");
        assert_eq!(md.config_hash, expected);
        assert_ne!(md.config_hash, UNCOMPUTED_CONFIG_HASH);
    }

    #[cfg(feature = "minhash")]
    #[test]
    fn with_config_hash_attaches_lazily() {
        let sig = crate::classical::minhash::MinHashSig::<128>::empty();
        let fp = Fingerprint::MinHash(sig);
        let md = fp.metadata().with_config_hash(0xDEAD_BEEF_CAFE_BABE);
        assert_eq!(md.config_hash, 0xDEAD_BEEF_CAFE_BABE);
    }

    #[cfg(feature = "minhash")]
    #[test]
    fn minhash_name_matches_documented_format() {
        let sig = crate::classical::minhash::MinHashSig::<128>::empty();
        let n = Fingerprint::MinHash(sig).name();
        assert_eq!(n, "minhash-h128-v1");
        assert!(!n.contains("cfg="), "name() must not include cfg=");
    }

    #[cfg(feature = "simhash")]
    #[test]
    fn simhash_name_is_stable() {
        use crate::classical::simhash::SimHash64;
        let n = Fingerprint::SimHash(SimHash64::new(0)).name();
        assert_eq!(n, "simhash-b64-v1");
    }

    #[test]
    fn uncomputed_sentinel_is_zero() {
        assert_eq!(UNCOMPUTED_CONFIG_HASH, 0);
    }
}
