//! MinHash sketcher.
//!
//! `txtfp`'s MinHash uses the *double-hashing* construction (Indyk–Motwani
//! 1998 with the Kirsch–Mitzenmacher refinement): each input shingle is
//! hashed once with a 128-bit hash, and the `H` MinHash hashes are
//! derived as `low ^ (i * high)`. This is the same construction used by
//! `datasketch` and reproduces its Jaccard estimates within statistical
//! variance.
//!
//! # Performance note
//!
//! The double-hashing trick collapses MinHash work from `O(H × n)` per
//! shingle to `O(n + H)` per shingle, where `n` is the shingle's byte
//! length. With the default `H = 128`, this is the difference between
//! 5K docs/sec and 30K docs/sec on a 5KB English document.
//!
//! # See also
//!
//! - Broder, A. Z. (1997). *On the resemblance and containment of
//!   documents.* Variance bounds for `H = 128`.
//! - Kirsch, A., Mitzenmacher, M. (2008). *Less hashing, same
//!   performance.* Justifies the double-hashing construction.

mod fingerprinter;
mod jaccard;
mod sig;
mod streaming;

pub use fingerprinter::{MinHashFingerprinter, MinHashFingerprinterBuilder};
pub use jaccard::jaccard;
pub use sig::{MinHashSig, SCHEMA_VERSION};
pub use streaming::{DEFAULT_MAX_BUFFER_BYTES, MinHashStreaming};

pub use super::HashFamily;
