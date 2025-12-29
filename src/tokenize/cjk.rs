//! CJK tokenizers.
//!
//! Two segmenters are exposed:
//!
//! - [`CjkSegmenter::Jieba`] — `jieba-rs` with the bundled HMM
//!   Simplified-Chinese dictionary. Lazily initialized via
//!   `OnceLock` on first use; subsequent calls reuse the same
//!   Trie. Use this for Simplified or Traditional Chinese.
//! - [`CjkSegmenter::Lindera`] — Japanese / Korean morphological
//!   analysis via `lindera`. Requires the appropriate dictionary
//!   feature (`embed-ipadic` / `embed-ko-dic`) to be enabled on
//!   the `lindera` dependency. v0.1.0 ships without those embedded
//!   dictionaries to keep binary size small; the variant exists in
//!   the API but currently delegates to UAX #29 word boundaries.
//!   Real Japanese/Korean morphological tokenization lands in v0.1.1
//!   together with the appropriate dictionary embedding.
//!
//! # Performance
//!
//! Jieba's Trie is built once and shared across all `CjkTokenizer`
//! instances in a process. The initialization cost is amortized over
//! the program's lifetime. Per-call cost is `O(n)` in the input
//! length.

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use std::sync::OnceLock;

use jieba_rs::Jieba;

use super::{TokenStream, Tokenizer};

/// Underlying segmenter.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CjkSegmenter {
    /// `jieba-rs` with the default Simplified-Chinese dictionary.
    Jieba,
    /// `lindera` (Japanese / Korean). v0.1.0 placeholder; see module
    /// docs.
    Lindera,
}

/// CJK tokenizer.
#[derive(Copy, Clone, Debug)]
pub struct CjkTokenizer {
    segmenter: CjkSegmenter,
    /// Use Jieba's HMM model for unknown words. Default `false`
    /// (deterministic dictionary cuts only).
    use_hmm: bool,
}

impl Default for CjkTokenizer {
    fn default() -> Self {
        Self {
            segmenter: CjkSegmenter::Jieba,
            use_hmm: false,
        }
    }
}

impl CjkTokenizer {
    /// Construct with an explicit segmenter selection.
    #[must_use]
    pub fn new(segmenter: CjkSegmenter) -> Self {
        Self {
            segmenter,
            use_hmm: false,
        }
    }

    /// Toggle Jieba HMM (probabilistic) cutting for OOV words.
    /// Default `false` for byte-stable output.
    #[must_use]
    pub fn with_hmm(mut self, use_hmm: bool) -> Self {
        self.use_hmm = use_hmm;
        self
    }

    /// Borrow the configured segmenter.
    #[must_use]
    pub fn segmenter(&self) -> CjkSegmenter {
        self.segmenter
    }

    /// True if HMM cutting is on.
    #[must_use]
    pub fn uses_hmm(&self) -> bool {
        self.use_hmm
    }
}

/// Lazy-init singleton Jieba instance. The default constructor loads
/// the bundled SC dictionary at first use; this call is millisecond-
/// scale on a modern machine.
fn jieba() -> &'static Jieba {
    static JIEBA: OnceLock<Jieba> = OnceLock::new();
    JIEBA.get_or_init(Jieba::new)
}

impl Tokenizer for CjkTokenizer {
    fn tokens<'a>(&'a self, input: &'a str) -> TokenStream<'a> {
        match self.segmenter {
            CjkSegmenter::Jieba => {
                let segs: Vec<&'a str> = jieba()
                    .cut(input, self.use_hmm)
                    .into_iter()
                    .filter(|s| !s.trim().is_empty())
                    .collect();
                TokenStream::Borrowed(Box::new(segs.into_iter()))
            }
            CjkSegmenter::Lindera => {
                // Provisional: defer to UAX-29 word tokenizer until the
                // dictionary-embedded build lands. Per-codepoint
                // segmentation is the fallback that still produces
                // useful (though over-segmented) output for CJK input.
                let it = unicode_segmentation::UnicodeSegmentation::unicode_words(input)
                    .filter(|s| !s.is_empty());
                TokenStream::Borrowed(Box::new(it))
            }
        }
    }

    fn name(&self) -> Cow<'static, str> {
        match self.segmenter {
            CjkSegmenter::Jieba => {
                if self.use_hmm {
                    Cow::Borrowed("cjk-jieba-hmm")
                } else {
                    Cow::Borrowed("cjk-jieba")
                }
            }
            CjkSegmenter::Lindera => Cow::Borrowed("cjk-lindera"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;

    fn collect(s: &str, t: &CjkTokenizer) -> Vec<String> {
        t.tokens(s).into_string_iter().collect()
    }

    #[test]
    fn names_are_stable() {
        assert_eq!(CjkTokenizer::new(CjkSegmenter::Jieba).name(), "cjk-jieba");
        assert_eq!(
            CjkTokenizer::new(CjkSegmenter::Jieba).with_hmm(true).name(),
            "cjk-jieba-hmm"
        );
        assert_eq!(
            CjkTokenizer::new(CjkSegmenter::Lindera).name(),
            "cjk-lindera"
        );
    }

    #[test]
    fn jieba_segments_chinese() {
        let t = CjkTokenizer::default();
        let toks = collect("我爱北京天安门", &t);
        // Jieba's default dict produces "我", "爱", "北京", "天安门" for
        // this canonical example.
        assert!(toks.contains(&"北京".to_string()), "got {toks:?}");
        assert!(toks.contains(&"天安门".to_string()), "got {toks:?}");
    }

    #[test]
    fn jieba_handles_mixed_punctuation() {
        let t = CjkTokenizer::default();
        let toks = collect("中文测试，简单一点。", &t);
        assert!(toks.contains(&"中文".to_string()));
        assert!(toks.contains(&"测试".to_string()));
    }

    #[test]
    fn jieba_is_deterministic_in_default_mode() {
        let t = CjkTokenizer::default();
        let a = collect("我爱你 中文测试 世界", &t);
        let b = collect("我爱你 中文测试 世界", &t);
        assert_eq!(a, b);
    }

    #[test]
    fn jieba_singleton_is_shared() {
        // Two CjkTokenizers share the same Jieba via OnceLock.
        let j1 = jieba();
        let j2 = jieba();
        assert!(core::ptr::eq(j1, j2));
    }

    #[test]
    fn empty_input_yields_empty() {
        let t = CjkTokenizer::default();
        assert!(collect("", &t).is_empty());
    }

    #[test]
    fn ascii_passes_through_jieba() {
        let t = CjkTokenizer::default();
        let toks = collect("hello world", &t);
        assert!(toks.contains(&"hello".to_string()));
        assert!(toks.contains(&"world".to_string()));
    }

    #[test]
    fn lindera_variant_runs_without_panic() {
        let t = CjkTokenizer::new(CjkSegmenter::Lindera);
        let toks = collect("私は日本語を勉強しています", &t);
        assert!(!toks.is_empty());
    }

    #[test]
    fn default_uses_jieba_no_hmm() {
        let t = CjkTokenizer::default();
        assert_eq!(t.segmenter(), CjkSegmenter::Jieba);
        assert!(!t.uses_hmm());
    }

    #[test]
    fn hmm_toggle_changes_name() {
        let off = CjkTokenizer::new(CjkSegmenter::Jieba);
        let on = off.with_hmm(true);
        assert_ne!(off.name(), on.name());
    }
}
