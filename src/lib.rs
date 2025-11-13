//! `txtfp` — text fingerprinting SDK for Rust.
//!
//! `txtfp` extracts compact, deterministic, byte-stable hashes from text
//! so you can deduplicate corpora, detect near-duplicate documents, and
//! retrieve semantically similar passages — the fundamental primitives
//! behind systems like LLM training-set dedup, RAG retrieval, and content
//! moderation.
//!
//! The crate compiles **`no_std + alloc`** when the `std` feature is
//! disabled, so the canonicalizer, tokenizers, and classical
//! fingerprinters can run on `wasm32-unknown-unknown` and embedded
//! targets. The semantic, markup, and PDF features require `std`.
//!
//! # Quick tour
//!
//! - **Errors** — [`Error`] (`#[non_exhaustive]`) plus the [`Result`]
//!   alias.
//! - **Canonicalization** — [`canonical::Canonicalizer`] and its
//!   [`canonical::CanonicalizerBuilder`] implement the default pipeline
//!   (NFKC + simple casefold + Bidi/format strip), with optional UTS #39
//!   confusable skeleton (`security` feature).
//! - **Tokenization** — [`tokenize::Tokenizer`] trait,
//!   [`tokenize::WordTokenizer`], [`tokenize::GraphemeTokenizer`],
//!   [`tokenize::ShingleTokenizer`], and feature-gated CJK tokenizers.
//! - **Classical fingerprinters** — [`Fingerprinter`] (offline) and
//!   [`StreamingFingerprinter`] (incremental). Implementations:
//!   [`MinHashFingerprinter`] (`minhash`), [`SimHashFingerprinter`]
//!   (`simhash`), and [`LshIndex`] (`lsh`).
//! - **Semantic embeddings** — [`Embedding`], [`EmbeddingProvider`],
//!   [`semantic_similarity`] (`semantic` feature). The trait shape is
//!   parity-compatible with `audiofp`/`imgfprint`.
//!
//! # Example: deduplication
//!
//! ```rust,no_run
//! # #[cfg(feature = "minhash")] {
//! use txtfp::{
//!     Canonicalizer, Fingerprinter, MinHashFingerprinter, ShingleTokenizer,
//!     WordTokenizer, jaccard,
//! };
//!
//! let canonicalizer = Canonicalizer::default();
//! let tokenizer     = ShingleTokenizer { k: 5, inner: WordTokenizer };
//! let fp            = MinHashFingerprinter::<_, 128>::new(canonicalizer, tokenizer);
//!
//! let a = fp.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
//! let b = fp.fingerprint("the quick brown fox leaps over the lazy dog").unwrap();
//!
//! let similarity = jaccard(&a, &b);
//! assert!(similarity > 0.5);
//! # }
//! ```
//!
//! # Cargo features
//!
//! See the crate's `README.md` for the full feature matrix. By default:
//! `std`, `minhash`, `simhash`. Default features build cleanly on
//! `wasm32-unknown-unknown`.
//!
//! # Stability
//!
//! Hash byte layouts ([`MinHashSig`], [`SimHash64`]) are **semver-frozen**
//! as of v0.1.0. Each signature struct is prefixed with a `u16` schema
//! version so on-disk fingerprints can be safely round-tripped.
//!
//! # Provenance
//!
//! `txtfp` mirrors the conventions of two sibling crates:
//!
//! - [`audiofp`](https://crates.io/crates/audiofp) — audio fingerprinting.
//! - `imgfprint` — image fingerprinting.
//!
//! [`MinHashSig`]: classical::minhash::MinHashSig
//! [`SimHash64`]: classical::simhash::SimHash64

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]
#![deny(rustdoc::broken_intra_doc_links)]
#![warn(rust_2018_idioms)]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod canonical;
pub mod tokenize;

#[cfg(any(feature = "minhash", feature = "simhash", feature = "lsh"))]
pub mod classical;

#[cfg(feature = "semantic")]
#[cfg_attr(docsrs, doc(cfg(feature = "semantic")))]
pub mod semantic;

#[cfg(feature = "markup")]
#[cfg_attr(docsrs, doc(cfg(feature = "markup")))]
pub mod markup;

#[cfg(feature = "pdf")]
#[cfg_attr(docsrs, doc(cfg(feature = "pdf")))]
pub mod pdf;

mod error;
mod fingerprint;

pub use error::{Error, Result};
pub use fingerprint::{Fingerprint, FingerprintMetadata, algo, config_hash};

#[cfg(feature = "tlsh")]
#[cfg_attr(docsrs, doc(cfg(feature = "tlsh")))]
pub use fingerprint::TlshFingerprint;

pub use canonical::{
    CaseFold, Canonicalizer, CanonicalizerBuilder, Normalization, canonicalize,
};
pub use tokenize::{GraphemeTokenizer, ShingleTokenizer, Tokenizer, WordTokenizer};

#[cfg(any(feature = "minhash", feature = "simhash", feature = "lsh"))]
pub use classical::{Fingerprinter, StreamingFingerprinter};

#[cfg(feature = "minhash")]
#[cfg_attr(docsrs, doc(cfg(feature = "minhash")))]
pub use classical::minhash::{HashFamily, MinHashFingerprinter, MinHashSig, jaccard};

#[cfg(feature = "simhash")]
#[cfg_attr(docsrs, doc(cfg(feature = "simhash")))]
pub use classical::simhash::{
    SimHash64, SimHashFingerprinter, Weighting, cosine_estimate, hamming,
};

#[cfg(feature = "lsh")]
#[cfg_attr(docsrs, doc(cfg(feature = "lsh")))]
pub use classical::lsh::{LshIndex, LshIndexBuilder};

#[cfg(feature = "semantic")]
#[cfg_attr(docsrs, doc(cfg(feature = "semantic")))]
pub use semantic::{
    ChunkMode, ChunkingStrategy, Embedding, EmbeddingProvider, Pooling, chunk_for_model,
    semantic_similarity,
};

/// Crate version string, sourced from `Cargo.toml`.
///
/// Useful when persisting fingerprints alongside the producer version,
/// or when emitting diagnostics that need to identify the SDK build.
///
/// # Example
///
/// ```
/// assert!(!txtfp::VERSION.is_empty());
/// ```
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
