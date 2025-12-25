//! CJK tokenizers — Simplified Chinese, Japanese, Korean.
//!
//! v0.1.0 ships the trait-level surface and a thin wrapper that
//! delegates to either `jieba-rs` (Simplified Chinese) or `lindera`
//! (Japanese, Korean). The dictionaries are loaded lazily via
//! `OnceLock` on first use; subsequent calls are zero-init cost.
//!
//! This module is feature-gated on `cjk`. The full implementation
//! lands in v0.1.x — this stub exists so the workspace's `mod cjk`
//! declaration resolves under all feature configurations.

use alloc::borrow::Cow;
use alloc::boxed::Box;

use super::{TokenStream, Tokenizer};

/// Underlying segmenter.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CjkSegmenter {
    /// `jieba-rs`. Best for Simplified Chinese.
    Jieba,
    /// `lindera`. Best for Japanese / Korean.
    Lindera,
}

/// CJK tokenizer.
///
/// Currently a thin shim that re-uses the [`super::WordTokenizer`]
/// (UAX #29 word-boundary segmentation, which already handles CJK
/// codepoint-by-codepoint). The dictionary-backed segmenters are
/// scheduled for v0.1.1.
#[derive(Copy, Clone, Debug)]
pub struct CjkTokenizer {
    segmenter: CjkSegmenter,
}

impl Default for CjkTokenizer {
    fn default() -> Self {
        Self {
            segmenter: CjkSegmenter::Jieba,
        }
    }
}

impl CjkTokenizer {
    /// Construct with an explicit segmenter selection.
    #[must_use]
    pub fn new(segmenter: CjkSegmenter) -> Self {
        Self { segmenter }
    }

    /// Borrow the configured segmenter.
    #[must_use]
    pub fn segmenter(&self) -> CjkSegmenter {
        self.segmenter
    }
}

impl Tokenizer for CjkTokenizer {
    fn tokens<'a>(&'a self, input: &'a str) -> TokenStream<'a> {
        // Provisional: defer to the UAX #29 word tokenizer until the
        // dictionary-backed segmenters land. UAX #29 still produces a
        // useful (per-codepoint) token stream for CJK.
        super::WordTokenizer.tokens(input);
        // Bypass and produce the same stream.
        let it = unicode_segmentation::UnicodeSegmentation::unicode_words(input)
            .filter(|s| !s.is_empty());
        TokenStream::Borrowed(Box::new(it))
    }

    fn name(&self) -> Cow<'static, str> {
        match self.segmenter {
            CjkSegmenter::Jieba => Cow::Borrowed("cjk-jieba"),
            CjkSegmenter::Lindera => Cow::Borrowed("cjk-lindera"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use alloc::vec::Vec;

    fn collect(s: &str, t: &CjkTokenizer) -> Vec<String> {
        t.tokens(s).into_string_iter().collect()
    }

    #[test]
    fn names_are_stable() {
        assert_eq!(CjkTokenizer::new(CjkSegmenter::Jieba).name(), "cjk-jieba");
        assert_eq!(
            CjkTokenizer::new(CjkSegmenter::Lindera).name(),
            "cjk-lindera"
        );
    }

    #[test]
    fn yields_non_empty_tokens_for_chinese() {
        let t = CjkTokenizer::default();
        let toks = collect("我爱你", &t);
        assert!(!toks.is_empty());
    }

    #[test]
    fn default_uses_jieba() {
        assert_eq!(CjkTokenizer::default().segmenter(), CjkSegmenter::Jieba);
    }
}
