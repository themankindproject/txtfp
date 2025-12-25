//! SimHash sketcher.
//!
//! SimHash projects a token-weighted bag-of-words onto a 64-bit
//! locality-sensitive fingerprint. Two documents with similar SimHashes
//! have similar Charikar cosines; the Hamming distance between
//! signatures is a fast proxy for `1 - cos θ`.
//!
//! Reference: Charikar, M. (2002). *Similarity estimation techniques
//! from rounding algorithms.*
//!
//! # Performance note
//!
//! [`hamming`] uses [`u64::count_ones`], which the compiler lowers to
//! the hardware `POPCNT` instruction on x86_64 and to `cnt` on AArch64.
//! This makes Hamming-distance comparisons effectively free.

mod distance;
mod fingerprinter;
mod sig;
mod streaming;

pub use distance::{cosine_estimate, hamming};
pub use fingerprinter::{IdfTable, SimHashFingerprinter, SimHashFingerprinterBuilder, Weighting};
pub use sig::{SCHEMA_VERSION, SimHash64};
pub use streaming::SimHashStreaming;

pub use super::HashFamily;
