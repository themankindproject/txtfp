//! K-shingle adaptor.
//!
//! Wraps any [`Tokenizer`] and emits k-grams (joined with a single
//! space) over the inner token stream. **Materializes** the inner
//! iterator into a `Vec` because k-grams need lookback; pick a `k` that
//! fits the memory budget.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use super::{TokenStream, Tokenizer};

/// K-shingle adaptor over an inner [`Tokenizer`].
///
/// Yields each contiguous run of `k` inner tokens joined by a single
/// ASCII space. The classical input shape for MinHash similarity:
/// `ShingleTokenizer { k: 5, inner: WordTokenizer }` over canonicalized
/// English text is the production sweet spot.
///
/// # Choosing `k`
///
/// - **`k = 1`** is functionally identical to the inner tokenizer but
///   with materialization overhead — prefer the inner tokenizer
///   directly.
/// - **`k = 3`** trades precision for recall (more matches, more noise).
/// - **`k = 5`** is the de-facto standard for English document
///   deduplication (datasketch, sourmash, cookbook examples).
/// - **`k = 7..10`** for stricter near-duplicate detection on long
///   technical prose.
///
/// # Performance
///
/// The `for_each_token` hot path uses a single re-used backing buffer
/// plus a range table, so per-shingle allocation is O(1) regardless
/// of input size. The `tokens()` path materializes a `Vec<String>` and
/// is retained only for compatibility with the [`TokenStream`] API;
/// new code should prefer `for_each_token`.
///
/// # Edge cases
///
/// - `k = 0` yields an empty stream.
/// - Input with fewer than `k` inner tokens yields a single shingle
///   containing all available tokens (matches the `datasketch` convention
///   and avoids returning an empty stream when the caller asked for `k=5`
///   on a 3-word document).
///
/// # Example
///
/// ```
/// use txtfp::{ShingleTokenizer, Tokenizer, WordTokenizer};
///
/// let s = ShingleTokenizer { k: 3, inner: WordTokenizer };
/// let mut shingles = Vec::new();
/// s.for_each_token("the quick brown fox", &mut |t| shingles.push(t.to_owned()));
/// assert_eq!(shingles, ["the quick brown", "quick brown fox"]);
/// ```
///
/// [`TokenStream`]: super::TokenStream
#[derive(Clone, Debug)]
pub struct ShingleTokenizer<T: Tokenizer> {
    /// Shingle size. Must be ≥ 1; `k = 0` yields an empty stream.
    pub k: usize,
    /// Inner tokenizer whose output is shingled.
    pub inner: T,
}

impl<T: Tokenizer> Tokenizer for ShingleTokenizer<T> {
    fn tokens<'a>(&'a self, input: &'a str) -> TokenStream<'a> {
        if self.k == 0 {
            return TokenStream::Owned(Box::new(core::iter::empty()));
        }

        // Materialize inner tokens into owned Strings so we can join
        // contiguous windows. Borrowed iterators don't allow random
        // access, and computing windows on a streaming iterator without
        // buffering would be O(k·n) in copies.
        let toks: Vec<String> = self.inner.tokens(input).into_string_iter().collect();

        if toks.is_empty() {
            return TokenStream::Owned(Box::new(core::iter::empty()));
        }
        if toks.len() < self.k {
            // Smaller-than-k input collapses to a single shingle of
            // whatever's there. Matches the `datasketch` convention and
            // avoids an empty stream when the caller asked for k=5 on a
            // 3-word document.
            let joined = toks.join(" ");
            return TokenStream::Owned(Box::new(core::iter::once(joined)));
        }

        let k = self.k;
        let it = (0..=(toks.len() - k)).map(move |i| toks[i..i + k].join(" "));
        TokenStream::Owned(Box::new(it))
    }

    fn name(&self) -> Cow<'static, str> {
        Cow::Owned(format!("shingle-k={}/{}", self.k, self.inner.name()))
    }

    /// Zero-allocation-per-shingle hot path. Maintains a fixed-size ring
    /// of inner-token byte ranges into a re-usable backing string, then
    /// formats each k-gram into a single re-usable [`String`] buffer.
    /// Compared to the [`Tokenizer::tokens`] path, this saves
    /// `O(N + N - k)` allocations per call.
    fn for_each_token(&self, input: &str, f: &mut dyn FnMut(&str)) {
        if self.k == 0 {
            return;
        }
        let k = self.k;

        // Concatenate inner tokens into a single backing buffer, recording
        // their byte ranges. Ranges live as long as `flat`.
        let mut flat = String::with_capacity(input.len());
        let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(input.len() / 4);
        self.inner.for_each_token(input, &mut |w| {
            let start = flat.len();
            flat.push_str(w);
            ranges.push((start, flat.len()));
        });

        if ranges.is_empty() {
            return;
        }

        let mut buf = String::with_capacity(64);

        if ranges.len() < k {
            // Single shingle that covers all available tokens, joined
            // by single ASCII space. Matches the byte layout of the
            // `tokens()` path's `.join(" ")`.
            for (i, (s, e)) in ranges.iter().enumerate() {
                if i > 0 {
                    buf.push(' ');
                }
                buf.push_str(&flat[*s..*e]);
            }
            f(&buf);
            return;
        }

        for i in 0..=(ranges.len() - k) {
            buf.clear();
            for (j, (s, e)) in ranges[i..i + k].iter().enumerate() {
                if j > 0 {
                    buf.push(' ');
                }
                buf.push_str(&flat[*s..*e]);
            }
            f(&buf);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenize::WordTokenizer;
    use alloc::string::ToString;

    fn shingles(k: usize, s: &str) -> Vec<String> {
        ShingleTokenizer {
            k,
            inner: WordTokenizer,
        }
        .tokens(s)
        .into_string_iter()
        .collect()
    }

    #[test]
    fn k1_yields_each_word() {
        assert_eq!(
            shingles(1, "the quick brown fox"),
            ["the", "quick", "brown", "fox"]
        );
    }

    #[test]
    fn k2_yields_pairs() {
        assert_eq!(shingles(2, "a b c"), ["a b", "b c"]);
    }

    #[test]
    fn k3_yields_triples() {
        assert_eq!(
            shingles(3, "the quick brown fox"),
            ["the quick brown", "quick brown fox"]
        );
    }

    #[test]
    fn k_larger_than_input_yields_single_joined() {
        assert_eq!(shingles(5, "a b"), ["a b"]);
    }

    #[test]
    fn empty_input_yields_empty() {
        assert!(shingles(3, "").is_empty());
    }

    #[test]
    fn k_zero_yields_empty() {
        assert!(shingles(0, "a b c").is_empty());
    }

    #[test]
    fn name_is_well_formed() {
        let n = ShingleTokenizer {
            k: 7,
            inner: WordTokenizer,
        }
        .name();
        assert_eq!(n, "shingle-k=7/word-uax29");
    }

    #[test]
    fn name_call_does_not_mutate_state() {
        let s = ShingleTokenizer {
            k: 4,
            inner: WordTokenizer,
        };
        let n1 = s.name().to_string();
        let n2 = s.name().to_string();
        assert_eq!(n1, n2);
    }
}
