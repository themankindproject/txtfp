//! Streaming MinHash fingerprinter.
//!
//! v0.1.0 ships the buffered variant: `update` appends bytes to a
//! UTF-8-validated buffer, `finalize` runs the offline algorithm.
//! True online positional MinHash is scheduled for v0.2.

use crate::classical::StreamingFingerprinter;
use crate::classical::utf8_stream::Utf8StreamBuffer;
use crate::error::Result;
use crate::tokenize::Tokenizer;

use super::fingerprinter::MinHashFingerprinter;
use super::sig::MinHashSig;

/// Default cap on the running buffer in bytes (16 MiB). Update beyond
/// this returns [`crate::Error::InvalidInput`] so an attacker cannot
/// exhaust memory by streaming unboundedly large input.
pub const DEFAULT_MAX_BUFFER_BYTES: usize = 16 * 1024 * 1024;

/// Buffered streaming sketcher.
///
/// Wraps a [`MinHashFingerprinter`] and accumulates UTF-8 bytes across
/// [`update`] calls. [`finalize`] runs the offline algorithm on the
/// accumulated buffer.
///
/// [`update`]: StreamingFingerprinter::update
/// [`finalize`]: StreamingFingerprinter::finalize
pub struct MinHashStreaming<T: Tokenizer, const H: usize> {
    inner: MinHashFingerprinter<T, H>,
    buf: Utf8StreamBuffer,
}

impl<T: Tokenizer, const H: usize> MinHashStreaming<T, H> {
    /// Construct a streamer wrapping `inner`.
    ///
    /// Buffer cap defaults to [`DEFAULT_MAX_BUFFER_BYTES`] (16 MiB).
    ///
    /// # Arguments
    ///
    /// * `inner` — the offline [`MinHashFingerprinter`] whose
    ///   canonicalizer + tokenizer + hash configuration the streamer
    ///   inherits.
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::{
    ///     Canonicalizer, MinHashFingerprinter, MinHashStreaming,
    ///     ShingleTokenizer, WordTokenizer,
    /// };
    ///
    /// let s = MinHashStreaming::<_, 64>::new(MinHashFingerprinter::new(
    ///     Canonicalizer::default(),
    ///     ShingleTokenizer { k: 3, inner: WordTokenizer },
    /// ));
    /// assert_eq!(s.buffered_bytes(), 0);
    /// ```
    pub fn new(inner: MinHashFingerprinter<T, H>) -> Self {
        Self {
            inner,
            buf: Utf8StreamBuffer::new(DEFAULT_MAX_BUFFER_BYTES),
        }
    }

    /// Override the buffer cap.
    ///
    /// Useful for tests or constrained environments where 16 MiB is too
    /// generous. Setting the cap below the document size causes the
    /// next [`update`] call to return [`crate::Error::InvalidInput`].
    ///
    /// # Arguments
    ///
    /// * `max_bytes` — maximum total bytes the streamer is willing to
    ///   accumulate.
    ///
    /// [`update`]: crate::StreamingFingerprinter::update
    #[must_use]
    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.buf.set_max_bytes(max_bytes);
        self
    }

    /// Total bytes accumulated so far (excluding the unfinished
    /// multi-byte UTF-8 carry).
    ///
    /// # Returns
    ///
    /// The size of the validated UTF-8 prefix. The streamer may also
    /// hold a few additional bytes in a transient carry buffer when an
    /// update arrives mid-codepoint; those are not counted here.
    pub fn buffered_bytes(&self) -> usize {
        self.buf.buffered_bytes()
    }
}

impl<T: Tokenizer, const H: usize> StreamingFingerprinter for MinHashStreaming<T, H> {
    type Output = MinHashSig<H>;

    #[inline]
    fn update(&mut self, chunk: &[u8]) -> Result<()> {
        self.buf.update(chunk)
    }

    fn finalize(self) -> Result<Self::Output> {
        let s = self.buf.finalize_str()?;
        let canonical = self.inner.canonicalizer().canonicalize(s);
        self.inner.sketch_canonical(&canonical)
    }

    #[inline]
    fn reset(&mut self) {
        self.buf.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::Canonicalizer;
    use crate::classical::Fingerprinter;
    use crate::error::Error;
    use crate::tokenize::{ShingleTokenizer, WordTokenizer};

    fn make() -> MinHashStreaming<ShingleTokenizer<WordTokenizer>, 64> {
        MinHashStreaming::new(MinHashFingerprinter::new(
            Canonicalizer::default(),
            ShingleTokenizer {
                k: 3,
                inner: WordTokenizer,
            },
        ))
    }

    #[test]
    fn streaming_matches_offline_for_single_chunk() {
        let txt = "the quick brown fox jumps over the lazy dog";

        let mut s = make();
        s.update(txt.as_bytes()).unwrap();
        let stream_sig = s.finalize().unwrap();

        let offline = MinHashFingerprinter::<_, 64>::new(
            Canonicalizer::default(),
            ShingleTokenizer {
                k: 3,
                inner: WordTokenizer,
            },
        );
        let offline_sig = offline.fingerprint(txt).unwrap();

        assert_eq!(stream_sig, offline_sig);
    }

    #[test]
    fn streaming_matches_offline_across_chunks() {
        let txt = "the quick brown fox jumps over the lazy dog";

        let mut s = make();
        for chunk in txt.as_bytes().chunks(7) {
            s.update(chunk).unwrap();
        }
        let stream_sig = s.finalize().unwrap();

        let offline = MinHashFingerprinter::<_, 64>::new(
            Canonicalizer::default(),
            ShingleTokenizer {
                k: 3,
                inner: WordTokenizer,
            },
        );
        let offline_sig = offline.fingerprint(txt).unwrap();

        assert_eq!(stream_sig, offline_sig);
    }

    #[test]
    fn empty_finalize_errors() {
        let s = make();
        assert!(matches!(s.finalize(), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn invalid_utf8_errors() {
        let mut s = make();
        // Lone continuation byte: never valid.
        let r = s.update(&[0x80]);
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn split_multibyte_works() {
        let mut s = make();
        // 'é' = 0xC3 0xA9. Split across two updates.
        s.update(&[0xC3]).unwrap();
        s.update(&[0xA9]).unwrap();
        s.update(b" cafe noir cafe noir cafe noir").unwrap();
        let sig = s.finalize().unwrap();
        assert_ne!(sig.hashes[0], u64::MAX);
    }

    #[test]
    fn reset_clears_buffer() {
        let mut s = make();
        s.update(b"hello world").unwrap();
        s.reset();
        assert_eq!(s.buffered_bytes(), 0);
        assert!(matches!(s.finalize(), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn buffer_cap_enforced() {
        let mut s = make().with_max_bytes(16);
        s.update(b"0123456789ABCDEF").unwrap(); // exactly 16 bytes
        let r = s.update(b"!");
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn trailing_incomplete_multibyte_errors_on_finalize() {
        let mut s = make();
        s.update(&[0xC3]).unwrap(); // first byte of a 2-byte sequence
        let r = s.finalize();
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }
}
