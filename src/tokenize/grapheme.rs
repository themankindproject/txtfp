//! UAX #29 grapheme-cluster tokenizer.
//!
//! Useful when the unit of comparison is the user-perceived character
//! rather than the word: emoji deduplication, character-level shingling
//! on languages without word boundaries, fuzzy matching of mixed-script
//! identifiers.

use alloc::borrow::Cow;
use alloc::boxed::Box;

use unicode_segmentation::UnicodeSegmentation;

use super::{TokenStream, Tokenizer};

/// Grapheme-cluster tokenizer (UAX #29 extended grapheme clusters).
///
/// Splits text into user-perceived characters. Family ZWJ sequences
/// (`👨‍👩‍👧‍👦`), regional-indicator flag pairs (`🇺🇸`), and combining-mark
/// composites (`e + ◌́` → `é`) are each a single grapheme.
///
/// # Performance
///
/// Zero-sized (`Copy`). The `for_each_token` impl walks `graphemes(true)`
/// directly and yields borrowed `&str` slices — no allocation per token.
///
/// # Use cases
///
/// - Character-level shingling on languages without word boundaries
///   (Thai, Chinese, …).
/// - Emoji deduplication.
/// - Fuzzy matching of mixed-script identifiers where word boundaries
///   are ambiguous.
///
/// # Example
///
/// ```
/// use txtfp::{GraphemeTokenizer, Tokenizer};
///
/// let mut count = 0;
/// GraphemeTokenizer.for_each_token("👨\u{200D}👩\u{200D}👧", &mut |_| count += 1);
/// assert_eq!(count, 1);                                              // family emoji = 1 grapheme
/// ```
#[derive(Default, Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct GraphemeTokenizer;

impl Tokenizer for GraphemeTokenizer {
    fn tokens<'a>(&'a self, input: &'a str) -> TokenStream<'a> {
        // `true` = extended grapheme clusters (vs legacy).
        TokenStream::Borrowed(Box::new(input.graphemes(true).filter(|s| !s.is_empty())))
    }

    #[inline]
    fn name(&self) -> Cow<'static, str> {
        Cow::Borrowed("grapheme-uax29")
    }

    #[inline]
    fn for_each_token(&self, input: &str, f: &mut dyn FnMut(&str)) {
        for g in input.graphemes(true) {
            if !g.is_empty() {
                f(g);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec::Vec;

    fn collect(s: &str) -> Vec<String> {
        GraphemeTokenizer
            .tokens(s)
            .into_string_iter()
            .collect::<Vec<_>>()
    }

    #[test]
    fn ascii_is_per_char() {
        assert_eq!(collect("abc"), ["a", "b", "c"]);
    }

    #[test]
    fn flag_emoji_is_one_grapheme() {
        // 🇺🇸 (regional indicator pair) should be one extended grapheme.
        let toks = collect("🇺🇸");
        assert_eq!(toks.len(), 1);
    }

    #[test]
    fn family_zwj_is_one_grapheme() {
        // 👨‍👩‍👧 (man + ZWJ + woman + ZWJ + girl).
        let toks = collect("👨\u{200D}👩\u{200D}👧");
        assert_eq!(toks.len(), 1);
    }

    #[test]
    fn combining_marks_glue_to_base() {
        // 'e' + combining acute = single grapheme.
        let toks = collect("e\u{0301}");
        assert_eq!(toks.len(), 1);
    }

    #[test]
    fn name_is_stable() {
        assert_eq!(GraphemeTokenizer.name(), "grapheme-uax29");
    }
}
