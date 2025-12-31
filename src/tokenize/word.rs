//! UAX #29 word tokenizer.
//!
//! Filters segments to those containing at least one alphanumeric
//! codepoint, dropping pure-punctuation and pure-whitespace segments.

use alloc::borrow::Cow;
use alloc::boxed::Box;

use unicode_segmentation::UnicodeSegmentation;

use super::{TokenStream, Tokenizer};

/// UAX #29 word-boundary tokenizer.
///
/// Splits text into words using the algorithm specified in
/// [Unicode Annex #29 §4.1](https://www.unicode.org/reports/tr29/#Word_Boundaries),
/// then drops segments that contain no alphanumeric content (whitespace
/// and pure-punctuation runs are filtered).
///
/// # Performance
///
/// Zero-sized (`Copy`). The `for_each_token` impl walks `unicode_words()`
/// directly and yields borrowed `&str` slices — no allocation per token.
///
/// # Behaviour
///
/// - English contractions like `"don't"` are one token.
/// - CJK codepoints become individual tokens (UAX #29 doesn't perform
///   dictionary segmentation; use `CjkTokenizer` (`cjk` feature) for
///   that).
/// - Numeric tokens such as `"v1.2"` and `"3.14"` are kept whole.
/// - Pure-punctuation segments are dropped.
///
/// # Example
///
/// ```
/// use txtfp::{Tokenizer, WordTokenizer};
///
/// let mut tokens = Vec::new();
/// WordTokenizer.for_each_token("the quick brown fox", &mut |t| tokens.push(t.to_owned()));
/// assert_eq!(tokens, ["the", "quick", "brown", "fox"]);
/// ```
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct WordTokenizer;

impl Tokenizer for WordTokenizer {
    fn tokens<'a>(&'a self, input: &'a str) -> TokenStream<'a> {
        let it = input.unicode_words().filter(|s| !s.is_empty());
        TokenStream::Borrowed(Box::new(it))
    }

    #[inline]
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("word-uax29")
    }

    /// Zero-allocation path: walk `unicode_words()` and forward borrowed
    /// `&str` slices directly. No `String` materialization.
    #[inline]
    fn for_each_token(&self, input: &str, f: &mut dyn FnMut(&str)) {
        for w in input.unicode_words() {
            if !w.is_empty() {
                f(w);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    fn collect(s: &str) -> Vec<String> {
        WordTokenizer
            .tokens(s)
            .into_string_iter()
            .collect::<Vec<_>>()
    }

    #[test]
    fn empty_input() {
        assert!(collect("").is_empty());
    }

    #[test]
    fn ascii_words() {
        assert_eq!(collect("hello world"), ["hello", "world"]);
    }

    #[test]
    fn punctuation_and_whitespace_drop() {
        assert_eq!(collect("hi, world!\n\nhi"), ["hi", "world", "hi"]);
    }

    #[test]
    fn unicode_letters() {
        assert_eq!(collect("café résumé"), ["café", "résumé"]);
    }

    #[test]
    fn cjk_codepoints_become_words() {
        let toks = collect("我爱你");
        assert!(!toks.is_empty());
    }

    #[test]
    fn name_is_stable() {
        assert_eq!(WordTokenizer.name(), "word-uax29");
    }

    #[test]
    fn numbers_are_words() {
        let toks = collect("v1.2 RC3");
        assert!(toks.contains(&"RC3".to_string()));
    }

    #[test]
    fn apostrophes_in_contractions() {
        let toks = collect("don't go");
        assert!(toks.iter().any(|t| t.contains("don")));
    }
}
