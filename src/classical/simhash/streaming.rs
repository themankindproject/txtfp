//! Streaming SimHash fingerprinter.
//!
//! Buffered variant for v0.1.0 — same trade-off as MinHash streaming.

use crate::classical::StreamingFingerprinter;
use crate::classical::utf8_stream::Utf8StreamBuffer;
use crate::error::Result;
use crate::tokenize::Tokenizer;

use super::fingerprinter::SimHashFingerprinter;
use super::sig::SimHash64;

/// Default cap on the running buffer in bytes (16 MiB).
pub const DEFAULT_MAX_BUFFER_BYTES: usize = 16 * 1024 * 1024;

/// Buffered streaming SimHash sketcher.
pub struct SimHashStreaming<T: Tokenizer> {
    inner: SimHashFingerprinter<T>,
    buf: Utf8StreamBuffer,
}

impl<T: Tokenizer> SimHashStreaming<T> {
    /// Construct a streamer wrapping `inner`.
    pub fn new(inner: SimHashFingerprinter<T>) -> Self {
        Self {
            inner,
            buf: Utf8StreamBuffer::new(DEFAULT_MAX_BUFFER_BYTES),
        }
    }

    /// Override the buffer cap.
    #[must_use]
    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.buf.set_max_bytes(max_bytes);
        self
    }

    /// Bytes accumulated so far.
    pub fn buffered_bytes(&self) -> usize {
        self.buf.buffered_bytes()
    }
}

impl<T: Tokenizer> StreamingFingerprinter for SimHashStreaming<T> {
    type Output = SimHash64;

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
    use crate::tokenize::WordTokenizer;

    fn make() -> SimHashStreaming<WordTokenizer> {
        SimHashStreaming::new(SimHashFingerprinter::new(
            Canonicalizer::default(),
            WordTokenizer,
        ))
    }

    #[test]
    fn streaming_matches_offline_for_single_chunk() {
        let txt = "the quick brown fox jumps over the lazy dog";
        let mut s = make();
        s.update(txt.as_bytes()).unwrap();
        let stream_sig = s.finalize().unwrap();
        let offline = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer);
        let offline_sig = offline.fingerprint(txt).unwrap();
        assert_eq!(stream_sig, offline_sig);
    }

    #[test]
    fn streaming_matches_offline_across_chunks() {
        let txt = "the quick brown fox jumps over the lazy dog";
        let mut s = make();
        for chunk in txt.as_bytes().chunks(5) {
            s.update(chunk).unwrap();
        }
        let stream_sig = s.finalize().unwrap();
        let offline = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer);
        let offline_sig = offline.fingerprint(txt).unwrap();
        assert_eq!(stream_sig, offline_sig);
    }

    #[test]
    fn empty_finalize_errors() {
        assert!(matches!(make().finalize(), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn invalid_utf8_errors() {
        let mut s = make();
        assert!(matches!(s.update(&[0x80]), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn reset_clears() {
        let mut s = make();
        s.update(b"hello world hello world").unwrap();
        s.reset();
        assert_eq!(s.buffered_bytes(), 0);
    }
}
