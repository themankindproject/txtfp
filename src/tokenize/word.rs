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
/// Zero-sized. `Copy`, `Send`, `Sync`.
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
