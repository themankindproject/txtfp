//! CJK tokenizer.
//!
//! A single segmenter is exposed:
//!
//! - [`CjkSegmenter::Jieba`] — `jieba-rs` with the bundled HMM
//!   Simplified-Chinese dictionary. Lazily initialized via
//!   [`OnceLock`] on first use; subsequent calls reuse the same trie.
//!
//! For Japanese, Korean, or other languages requiring morphological
//! analysis, implement the [`super::Tokenizer`] trait against a
//! dedicated tokenizer crate (`lindera`, `vibrato`, `kuromoji-rs`,
//! …) and feed it into any [`crate::Fingerprinter`]. Bundling those
//! tokenizers here would bloat the binary by 50–150 MiB per language
//! and add a build-time network dependency on the dictionary host.
//!
//! # Performance
//!
//! The jieba dictionary is loaded once via [`OnceLock`] and shared
//! across every [`CjkTokenizer`] in the process. Per-call cost is
//! linear in the input length plus dictionary lookups (`O(n log m)`).
//!
//! # Binary size
//!
//! - `cjk` (jieba alone): adds ~5 MiB of compressed dictionary.
//!
//! [`OnceLock`]: std::sync::OnceLock

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::vec::Vec;
use std::sync::OnceLock;

use jieba_rs::Jieba;

use super::{TokenStream, Tokenizer};

/// Underlying segmenter selection.
///
/// `#[non_exhaustive]` so additional language-specific segmenters can
/// be added in a future minor release without breaking downstream
/// match arms.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CjkSegmenter {
    /// `jieba-rs` with the default Simplified-Chinese dictionary.
    Jieba,
}

/// CJK tokenizer.
///
/// Cheap to construct (a segmenter discriminant plus an HMM toggle
/// for jieba). All work happens lazily on the first `tokens()` /
/// `for_each_token()` call.
///
/// # Example
///
/// ```
/// # #[cfg(feature = "cjk")]
/// # {
/// use txtfp::{CjkSegmenter, CjkTokenizer, Tokenizer};
///
/// let t = CjkTokenizer::new(CjkSegmenter::Jieba);
/// let mut tokens = Vec::new();
/// t.for_each_token("我爱北京天安门", &mut |s| tokens.push(s.to_string()));
/// assert!(tokens.contains(&"北京".to_string()));
/// # }
/// ```
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
    ///
    /// # Arguments
    ///
    /// * `segmenter` — currently only [`CjkSegmenter::Jieba`].
    #[must_use]
    pub fn new(segmenter: CjkSegmenter) -> Self {
        Self {
            segmenter,
            use_hmm: false,
        }
    }

    /// Toggle Jieba HMM (probabilistic) cutting for OOV words.
    ///
    /// Default `false` for byte-stable output. Enabling HMM gives
    /// better recall on unfamiliar proper nouns but introduces
    /// tokenization variance across jieba updates — use only when the
    /// downstream task tolerates non-deterministic shingles.
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
        }
    }

    fn for_each_token(&self, input: &str, f: &mut dyn FnMut(&str)) {
        match self.segmenter {
            CjkSegmenter::Jieba => {
                for s in jieba().cut(input, self.use_hmm) {
                    if !s.trim().is_empty() {
                        f(s);
                    }
                }
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::{String, ToString};

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
    }

    #[test]
    fn jieba_segments_chinese() {
        let t = CjkTokenizer::default();
        let toks = collect("我爱北京天安门", &t);
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
