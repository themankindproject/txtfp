//! Streaming SimHash fingerprinter.
//!
//! Buffered variant for v0.1.0 — same trade-off as MinHash streaming.

use alloc::vec::Vec;
use core::str;

use crate::classical::StreamingFingerprinter;
use crate::error::{Error, Result};
use crate::tokenize::Tokenizer;

use super::fingerprinter::SimHashFingerprinter;
use super::sig::SimHash64;

/// Default cap on the running buffer in bytes (16 MiB).
pub const DEFAULT_MAX_BUFFER_BYTES: usize = 16 * 1024 * 1024;

/// Buffered streaming SimHash sketcher.
pub struct SimHashStreaming<T: Tokenizer> {
    inner: SimHashFingerprinter<T>,
    buffer: Vec<u8>,
    carry: Vec<u8>,
    max_bytes: usize,
}

impl<T: Tokenizer> SimHashStreaming<T> {
    /// Construct a streamer wrapping `inner`.
    pub fn new(inner: SimHashFingerprinter<T>) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
            carry: Vec::with_capacity(4),
            max_bytes: DEFAULT_MAX_BUFFER_BYTES,
        }
    }

    /// Override the buffer cap.
    #[must_use]
    pub fn with_max_bytes(mut self, max_bytes: usize) -> Self {
        self.max_bytes = max_bytes;
        self
    }

    /// Bytes accumulated so far.
    pub fn buffered_bytes(&self) -> usize {
        self.buffer.len()
    }
}

impl<T: Tokenizer> StreamingFingerprinter for SimHashStreaming<T> {
    type Output = SimHash64;

    fn update(&mut self, chunk: &[u8]) -> Result<()> {
        if self.buffer.len().saturating_add(chunk.len()) > self.max_bytes {
            return Err(Error::InvalidInput("streaming buffer exceeded cap".into()));
        }

        let mut combined = core::mem::take(&mut self.carry);
        combined.reserve(chunk.len());
        combined.extend_from_slice(chunk);

        let valid_up_to = match str::from_utf8(&combined) {
            Ok(_) => combined.len(),
            Err(e) => {
                if e.error_len().is_some() {
                    return Err(Error::InvalidInput("invalid UTF-8 in stream".into()));
                }
                e.valid_up_to()
            }
        };

        self.buffer.extend_from_slice(&combined[..valid_up_to]);
        self.carry.clear();
        self.carry.extend_from_slice(&combined[valid_up_to..]);
        Ok(())
    }

    fn finalize(self) -> Result<Self::Output> {
        if !self.carry.is_empty() {
            return Err(Error::InvalidInput("trailing incomplete UTF-8".into()));
        }
        if self.buffer.is_empty() {
            return Err(Error::InvalidInput("empty document".into()));
        }
        let s = str::from_utf8(&self.buffer)
            .map_err(|e| Error::InvalidInput(alloc::format!("internal UTF-8: {e}")))?;
        let canonical = self.inner.canonicalizer().canonicalize(s);
        self.inner.sketch_canonical(&canonical)
    }

    fn reset(&mut self) {
        self.buffer.clear();
        self.carry.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::Canonicalizer;
    use crate::classical::Fingerprinter;
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
