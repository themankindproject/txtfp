//! Classical (non-neural) fingerprinters.
//!
//! Two traits cover the two modes `txtfp` produces fingerprints in:
//!
//! - [`Fingerprinter`] — feed a whole `&str` and get its full output.
//!   Suited to enrolment / batch jobs.
//! - [`StreamingFingerprinter`] — push byte chunks as they arrive and
//!   consolidate at the end. Suited to large-file pipelines and
//!   memory-bounded streaming.
//!
//! Concrete implementations live in feature-gated submodules:
//! [`minhash`] (`minhash` feature), [`simhash`] (`simhash` feature),
//! [`lsh`] (`lsh` feature).

#[cfg(any(feature = "minhash", feature = "simhash"))]
mod hash;

#[cfg(any(feature = "minhash", feature = "simhash"))]
pub use hash::HashFamily;

#[cfg(feature = "lsh")]
#[cfg_attr(docsrs, doc(cfg(feature = "lsh")))]
pub mod lsh;

#[cfg(feature = "minhash")]
#[cfg_attr(docsrs, doc(cfg(feature = "minhash")))]
pub mod minhash;

#[cfg(feature = "simhash")]
#[cfg_attr(docsrs, doc(cfg(feature = "simhash")))]
pub mod simhash;

#[cfg(feature = "tlsh")]
#[cfg_attr(docsrs, doc(cfg(feature = "tlsh")))]
pub mod tlsh;

use crate::Result;

/// Offline fingerprinter — consumes a whole document and emits one
/// fingerprint.
///
/// Implementations are immutable in their public surface:
/// [`Fingerprinter::fingerprint`] takes `&self` so a single
/// fingerprinter can be shared across worker threads. Internal scratch
/// buffers, if any, must be allocated per call.
///
/// # Example: implementing `Fingerprinter` for a custom kernel
///
/// ```
/// use txtfp::{Canonicalizer, Fingerprinter, Result};
///
/// struct LengthHash {
///     canonicalizer: Canonicalizer,
/// }
///
/// impl Fingerprinter for LengthHash {
///     type Output = u64;
///     fn fingerprint(&self, input: &str) -> Result<u64> {
///         Ok(self.canonicalizer.canonicalize(input).len() as u64)
///     }
/// }
/// ```
pub trait Fingerprinter {
    /// The fingerprint produced by this extractor (e.g. `MinHashSig<128>`).
    type Output;

    /// Compute the fingerprint of `input`.
    ///
    /// # Arguments
    ///
    /// * `input` — UTF-8 text to fingerprint.
    ///
    /// # Errors
    ///
    /// Implementations return [`crate::Error::InvalidInput`] when:
    /// - the input is empty,
    /// - the input canonicalizes to an empty string (e.g. only zero-
    ///   width codepoints), or
    /// - the input cannot be tokenized into at least one token.
    ///
    /// Some impls also surface algorithm-specific errors
    /// ([`crate::Error::Config`] for misconfigured fingerprinters).
    fn fingerprint(&self, input: &str) -> Result<Self::Output>;
}

/// Streaming fingerprinter — accumulates bytes across calls, emits one
/// fingerprint at end-of-stream.
///
/// Streaming variants for `txtfp`'s classical algorithms buffer the
/// input internally and run the offline algorithm at [`finalize`] time.
/// True online sketches (positional MinHash, online SimHash) are
/// scheduled for v0.2 — they require positional shingles and a richer
/// state machine than the v0.1.0 contract guarantees.
///
/// # Example
///
/// ```
/// use txtfp::{
///     Canonicalizer, MinHashFingerprinter, MinHashStreaming,
///     ShingleTokenizer, StreamingFingerprinter, WordTokenizer,
/// };
///
/// let inner = MinHashFingerprinter::<_, 64>::new(
///     Canonicalizer::default(),
///     ShingleTokenizer { k: 3, inner: WordTokenizer },
/// );
/// let mut s = MinHashStreaming::new(inner);
///
/// s.update(b"the quick brown fox").unwrap();
/// s.update(b" jumps over the lazy dog").unwrap();
/// let sig = s.finalize().unwrap();
/// assert_eq!(sig.schema, 1);
/// ```
///
/// [`finalize`]: StreamingFingerprinter::finalize
pub trait StreamingFingerprinter {
    /// The fingerprint produced at end-of-stream.
    type Output;

    /// Append `chunk` to the internal buffer.
    ///
    /// # Arguments
    ///
    /// * `chunk` — raw bytes to append. Multi-byte UTF-8 sequences may
    ///   span chunk boundaries; implementations carry the partial
    ///   prefix forward to the next call.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] when:
    /// - the running buffer would exceed the implementation's
    ///   documented cap (default 16 MiB), or
    /// - `chunk` is invalid UTF-8 in a way that cannot be a partial
    ///   prefix of a valid sequence (e.g. lone continuation bytes).
    fn update(&mut self, chunk: &[u8]) -> Result<()>;

    /// Finalize the running state and produce the fingerprint.
    /// Consumes the streamer.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] when:
    /// - no input was ever fed (empty stream),
    /// - the buffer ends mid-multibyte UTF-8 sequence, or
    /// - the canonicalized + tokenized buffer is empty.
    fn finalize(self) -> Result<Self::Output>;

    /// Drop the buffer so the same streamer can be reused without
    /// reallocating.
    ///
    /// Useful when a worker thread fingerprints many small documents in
    /// sequence and you want to amortize the per-streamer allocation.
    fn reset(&mut self);
}
