//! Shared streaming core: `MinHashStreaming` and `SimHashStreaming`
//! buffer UTF-8-validated bytes across `update` calls and run the
//! wrapped offline fingerprinter at `finalize` time. This module holds
//! that machinery once.

use crate::classical::utf8_stream::Utf8StreamBuffer;
use crate::error::Result;

/// Buffered-streaming core shared by the classical streamers, generic
/// over the wrapped offline fingerprinter.
pub(crate) struct BufferedStream<F> {
    inner: F,
    buf: Utf8StreamBuffer,
}

impl<F> BufferedStream<F> {
    /// Wrap `inner` with a buffer capped at `max_bytes`.
    pub(crate) fn new(inner: F, max_bytes: usize) -> Self {
        Self {
            inner,
            buf: Utf8StreamBuffer::new(max_bytes),
        }
    }

    /// Override the buffer cap. Builder-style.
    #[inline]
    pub(crate) fn set_max_bytes(&mut self, max_bytes: usize) {
        self.buf.set_max_bytes(max_bytes);
    }

    /// Validated UTF-8 bytes accumulated so far (excluding the
    /// in-progress multi-byte carry).
    #[inline]
    pub(crate) fn buffered_bytes(&self) -> usize {
        self.buf.buffered_bytes()
    }

    /// Borrow the wrapped offline fingerprinter.
    #[inline]
    pub(crate) fn inner(&self) -> &F {
        &self.inner
    }

    /// Borrow the accumulated buffer as `&str`.
    ///
    /// # Errors
    ///
    /// Same as [`Utf8StreamBuffer::finalize_str`]: trailing incomplete
    /// UTF-8 or an empty stream surface as `Error::InvalidInput`.
    #[inline]
    pub(crate) fn finalize_str(&self) -> Result<&str> {
        self.buf.finalize_str()
    }

    /// Append a chunk to the buffer.
    #[inline]
    pub(crate) fn update(&mut self, chunk: &[u8]) -> Result<()> {
        self.buf.update(chunk)
    }

    /// Drop the buffered state, retaining allocations for reuse.
    #[inline]
    pub(crate) fn reset(&mut self) {
        self.buf.reset();
    }
}
