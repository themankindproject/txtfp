//! Banded LSH index over MinHash signatures.
//!
//! Locality-sensitive hashing collapses MinHash similarity search from
//! `O(N)` to roughly `O(1)` per query: for two signatures with Jaccard
//! similarity `t`, the probability of colliding in at least one band is
//! `1 - (1 - t^r)^b` where `b` is the number of bands and `r` the rows
//! per band. Choose `(b, r)` so the curve has its steep transition near
//! your similarity threshold and you get a sharp cutoff between
//! "neighbours" and "everything else".
//!
//! # Example
//!
//! ```
//! # #[cfg(feature = "minhash")] {
//! use txtfp::{
//!     Canonicalizer, Fingerprinter, LshIndex, LshIndexBuilder,
//!     MinHashFingerprinter, ShingleTokenizer, WordTokenizer,
//! };
//!
//! let canon = Canonicalizer::default();
//! let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
//! let fp = MinHashFingerprinter::<_, 128>::new(canon, tok);
//!
//! let mut idx: LshIndex<128> = LshIndexBuilder::for_threshold(0.7, 128)
//!     .unwrap()
//!     .build();
//!
//! idx.insert(0, fp.fingerprint("the quick brown fox").unwrap());
//! idx.insert(1, fp.fingerprint("the slow grey wolf").unwrap());
//!
//! let probe = fp.fingerprint("the quick brown fox").unwrap();
//! assert_eq!(idx.query(&probe), vec![0]);
//! # }
//! ```
//!
//! # Thread safety
//!
//! [`LshIndex`] is `Send + Sync` for read-only access but its
//! `insert`/`remove` mutators take `&mut self`. Wrap in `RwLock` /
//! `Mutex` for shared writes. Concurrency primitives live in UCFP, not
//! here.

mod builder;
mod index;

pub use builder::LshIndexBuilder;
pub use index::LshIndex;
