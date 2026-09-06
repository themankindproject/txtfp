//! Shared UTF-8 streaming buffer for [`super::minhash::MinHashStreaming`] and
//! [`super::simhash::SimHashStreaming`].
//!
//! Both streamers face the same problem: arbitrary byte chunks may
//! arrive split mid-codepoint, and the running buffer must be
//! validated as UTF-8 before being handed to the canonicalizer. This
//! helper encapsulates the carry/commit machinery and the cap
//! enforcement so the two streaming wrappers do not duplicate the
//! logic.
//!
//! The struct is `pub(crate)` and not part of the public API. Its
//! field layout is free to evolve.

use alloc::vec::Vec;
use core::str;

use crate::error::{Error, Result};

/// UTF-8-validated streaming buffer used by classical streaming sketchers.
///
/// Invariants:
/// - `buffer` is always valid UTF-8.
/// - `carry` holds at most 3 bytes that are a valid prefix of an
///   in-progress multi-byte UTF-8 sequence.
/// - after every successful `update`, `buffer.len() + carry.len()` is
///   at most `max_bytes`.
pub(crate) struct Utf8StreamBuffer {
    /// Accumulated, validated UTF-8 bytes.
    buffer: Vec<u8>,
    /// Incomplete tail of an in-progress UTF-8 sequence. At most 3 bytes
    /// (the longest valid UTF-8 prefix shorter than a full codepoint).
    carry: Vec<u8>,
    /// Maximum allowed size of `buffer + carry` in bytes.
    max_bytes: usize,
}

impl Utf8StreamBuffer {
    /// Create an empty buffer capped at `max_bytes`.
    #[inline]
    pub(crate) fn new(max_bytes: usize) -> Self {
        Self {
            buffer: Vec::new(),
            carry: Vec::with_capacity(4),
            max_bytes,
        }
    }

    /// Override the cap. Builder-style.
    #[inline]
    pub(crate) fn set_max_bytes(&mut self, max_bytes: usize) {
        self.max_bytes = max_bytes;
    }

    /// Number of validated UTF-8 bytes accumulated so far. The carry
    /// (in-progress codepoint prefix) is not counted.
    #[inline]
    pub(crate) fn buffered_bytes(&self) -> usize {
        self.buffer.len()
    }

    /// Clear all buffered state. Reuses the existing allocations.
    #[inline]
    pub(crate) fn reset(&mut self) {
        self.buffer.clear();
        self.carry.clear();
    }

    /// Append a chunk of bytes.
    ///
    /// Combines any leftover carry with `chunk`, splits at the longest
    /// valid UTF-8 prefix, commits the prefix to `buffer`, and stores
    /// the trailing incomplete codepoint (if any) as the new carry.
    ///
    /// # Errors
    ///
    /// - [`Error::InvalidInput`] when the combined `buffer + carry`
    ///   would exceed `max_bytes`. The buffer is left unchanged on
    ///   failure — including the carry, which is preserved even though
    ///   `chunk` was rejected wholesale.
    /// - [`Error::InvalidInput`] when the combined bytes contain an
    ///   invalid UTF-8 sequence that cannot be a partial prefix of a
    ///   valid codepoint (e.g. a lone continuation byte). Existing
    ///   buffered state is unchanged.
    pub(crate) fn update(&mut self, chunk: &[u8]) -> Result<()> {
        // Cap check up front, counting the carry toward the budget so
        // `buffer.len() + carry.len() <= max_bytes` holds after every
        // successful update (the old check ignored the carry, letting
        // the committed total drift up to 3 bytes past the cap).
        let total = self.buffer.len() + self.carry.len() + chunk.len();
        if total > self.max_bytes {
            return Err(Error::InvalidInput("streaming buffer exceeded cap".into()));
        }

        // Fast path (the steady state for aligned chunks): no carry and
        // the chunk is fully valid UTF-8 — commit it straight to
        // `buffer` with a single extend, skipping the combine pass.
        if self.carry.is_empty() {
            let valid_up_to = match str::from_utf8(chunk) {
                Ok(_) => chunk.len(),
                Err(e) => {
                    if e.error_len().is_some() {
                        return Err(Error::InvalidInput("invalid UTF-8 in stream".into()));
                    }
                    e.valid_up_to()
                }
            };
            self.buffer.extend_from_slice(&chunk[..valid_up_to]);
            self.carry.extend_from_slice(&chunk[valid_up_to..]);
            return Ok(());
        }

        // Slow path: a codepoint is in flight. Concatenate carry + chunk
        // into a transient working buffer, moving the existing carry out
        // so it can be reused as working storage.
        let mut combined = core::mem::take(&mut self.carry);
        combined.reserve(chunk.len());
        combined.extend_from_slice(chunk);

        let valid_up_to = match str::from_utf8(&combined) {
            Ok(_) => combined.len(),
            Err(e) => {
                if e.error_len().is_some() {
                    // Hard error. The chunk cannot be part of any valid
                    // stream, so restore the carry we took — a caller
                    // that ignores the error must not silently lose the
                    // in-progress codepoint (previously `combined` was
                    // dropped, destroying it). `truncate` cannot panic:
                    // the carry held ≤ 3 bytes before the chunk was
                    // appended, and the failure position reported by
                    // `from_utf8` lies within it.
                    let keep = combined.len() - chunk.len();
                    combined.truncate(keep);
                    self.carry = combined;
                    return Err(Error::InvalidInput("invalid UTF-8 in stream".into()));
                }
                e.valid_up_to()
            }
        };

        // Reserve once so extend_from_slice doesn't reallocate twice.
        self.buffer.reserve(valid_up_to);
        self.buffer.extend_from_slice(&combined[..valid_up_to]);
        // Hand the tail (≤ 3 bytes) back as the carry, shrinking the
        // combined buffer in place so its allocation carries forward
        // instead of being dropped and re-allocated every update.
        combined.drain(..valid_up_to);
        self.carry = combined;
        Ok(())
    }

    /// Borrow the accumulated buffer as `&str` for finalization.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] when:
    /// - the buffer ended mid-codepoint (non-empty carry), or
    /// - the buffer is empty (no input was ever fed).
    ///
    /// The returned `&str` is borrowed for the lifetime of `self`.
    pub(crate) fn finalize_str(&self) -> Result<&str> {
        if !self.carry.is_empty() {
            return Err(Error::InvalidInput("trailing incomplete UTF-8".into()));
        }
        if self.buffer.is_empty() {
            return Err(Error::InvalidInput("empty document".into()));
        }
        // SAFETY of correctness: `update` only commits prefixes that
        // `str::from_utf8` accepted; the buffer is guaranteed valid.
        // We still go through the checked converter so a bug elsewhere
        // surfaces as an error rather than UB.
        str::from_utf8(&self.buffer)
            .map_err(|e| Error::InvalidInput(alloc::format!("internal UTF-8: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_finalize_errors() {
        let b = Utf8StreamBuffer::new(64);
        assert!(matches!(b.finalize_str(), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn single_chunk_round_trip() {
        let mut b = Utf8StreamBuffer::new(64);
        b.update(b"hello world").unwrap();
        assert_eq!(b.finalize_str().unwrap(), "hello world");
    }

    #[test]
    fn split_multibyte_assembles() {
        let mut b = Utf8StreamBuffer::new(64);
        // 'é' = 0xC3 0xA9. Split across two updates.
        b.update(&[0xC3]).unwrap();
        b.update(&[0xA9]).unwrap();
        b.update(b" world").unwrap();
        assert_eq!(b.finalize_str().unwrap(), "é world");
    }

    #[test]
    fn invalid_utf8_lone_continuation_errors() {
        let mut b = Utf8StreamBuffer::new(64);
        assert!(matches!(b.update(&[0x80]), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn cap_enforced_on_update() {
        let mut b = Utf8StreamBuffer::new(8);
        b.update(b"01234567").unwrap();
        assert!(matches!(b.update(b"8"), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn finalize_rejects_trailing_carry() {
        let mut b = Utf8StreamBuffer::new(64);
        b.update(&[0xC3]).unwrap();
        assert!(matches!(b.finalize_str(), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn reset_clears_state() {
        let mut b = Utf8StreamBuffer::new(64);
        b.update(b"hello").unwrap();
        b.reset();
        assert_eq!(b.buffered_bytes(), 0);
        assert!(matches!(b.finalize_str(), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn buffered_bytes_excludes_carry() {
        let mut b = Utf8StreamBuffer::new(64);
        b.update(b"abc").unwrap();
        assert_eq!(b.buffered_bytes(), 3);
        b.update(&[0xC3]).unwrap(); // start of 'é'
        // The carry should not increase buffered_bytes().
        assert_eq!(b.buffered_bytes(), 3);
    }

    #[test]
    fn carry_survives_rejected_chunk() {
        // Regression: a hard-invalid chunk used to destroy the
        // in-progress carry. A caller that ignores the error and keeps
        // streaming must not lose those prefix bytes.
        let mut b = Utf8StreamBuffer::new(64);
        b.update(&[0xC3]).unwrap(); // half of 'é'
        // A second lead byte cannot continue 0xC3 → hard error.
        assert!(b.update(&[0xC3]).is_err());
        // The carry (0xC3) is still there; completing the codepoint now
        // yields a valid stream.
        b.update(&[0xA9]).unwrap(); // second half of 'é'
        b.update(b" world").unwrap();
        assert_eq!(b.finalize_str().unwrap(), "é world");
    }

    #[test]
    fn cap_counts_carry_bytes() {
        // Regression: the old cap check ignored the carry, letting
        // buffer + carry drift past max_bytes. Sequence: fill to 9
        // committed bytes + 1 carry byte (total 10 = cap), then try to
        // commit one more ASCII byte — the carry must count against the
        // budget and reject the update.
        let mut b = Utf8StreamBuffer::new(10);
        b.update(b"0123456789").unwrap(); // buffer = 10 = cap
        // A 2-byte codepoint would take the total to 12 > 10 → rejected
        // before the lead byte enters the carry.
        assert!(matches!(
            b.update(&[0xC3, 0xA9]),
            Err(Error::InvalidInput(_))
        ));
        // A 1-byte ASCII continuation also exceeds: 10 + 1 > 10.
        assert!(matches!(b.update(b"x"), Err(Error::InvalidInput(_))));
        // State is untouched: still exactly 10 committed bytes.
        assert_eq!(b.buffered_bytes(), 10);
        // Sanity: the carry counts toward the budget. With cap = 11:
        // 9 committed + 1 carry = 10 held; a 2-byte chunk overflows,
        // but completing the in-flight codepoint (10 + 1 = 11) fits.
        let mut b2 = Utf8StreamBuffer::new(11);
        b2.update(b"01234567").unwrap(); // 8 committed
        b2.update(&[b'a', 0xC3]).unwrap(); // 9 committed + 1 carry
        assert_eq!(b2.buffered_bytes(), 9);
        assert!(matches!(b2.update(b"xy"), Err(Error::InvalidInput(_)))); // 12 > 11
        b2.update(&[0xA9]).unwrap(); // completes 'é' → 11 held = cap
        assert_eq!(b2.buffered_bytes(), 11);
    }
}
