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

use crate::Result;

/// Offline fingerprinter — consumes a whole document and emits one
/// fingerprint.
///
/// Implementations are immutable in their public surface:
/// [`Fingerprinter::fingerprint`] takes `&self` so a single
/// fingerprinter can be shared across worker threads. Internal scratch
/// buffers, if any, must be allocated per call.
pub trait Fingerprinter {
    /// The fingerprint produced by this extractor (e.g. `MinHashSig<128>`).
    type Output;

    /// Compute the fingerprint of `input`.
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
/// [`finalize`]: StreamingFingerprinter::finalize
pub trait StreamingFingerprinter {
    /// The fingerprint produced at end-of-stream.
    type Output;

    /// Append `chunk` to the internal buffer. Returns
    /// [`crate::Error::InvalidInput`] if the running buffer would
    /// otherwise exceed the implementation's documented cap.
    fn update(&mut self, chunk: &[u8]) -> Result<()>;

    /// Finalize the running state and produce the fingerprint.
    /// Consumes the streamer.
    fn finalize(self) -> Result<Self::Output>;

    /// Drop the buffer so the same streamer can be reused without
    /// reallocating.
    fn reset(&mut self);
}
