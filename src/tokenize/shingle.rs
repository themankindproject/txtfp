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
/// `k = 1` is functionally identical to the inner tokenizer, but with
/// the materialization overhead — prefer the inner tokenizer directly
/// in that case.
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
