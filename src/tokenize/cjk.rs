//! CJK tokenizers.
//!
//! Three segmenters are exposed:
//!
//! - [`CjkSegmenter::Jieba`] — `jieba-rs` with the bundled HMM
//!   Simplified-Chinese dictionary. Lazily initialized via
//!   [`OnceLock`] on first use; subsequent calls reuse the same trie.
//!   Always available with the `cjk` feature.
//! - [`CjkSegmenter::Lindera`] — `lindera` morphological analysis with
//!   IPADIC (Japanese). Requires the `cjk-japanese` feature; falls
//!   back to UAX-29 word boundaries on `cjk` builds without
//!   `cjk-japanese`.
//! - [`CjkSegmenter::LinderaKoDic`] — `lindera` with ko-dic (Korean).
//!   Requires the `cjk-korean` feature; falls back to UAX-29 word
//!   boundaries on `cjk` builds without `cjk-korean`.
//!
//! # Performance
//!
//! Each dictionary is loaded once via [`OnceLock`] and shared across
//! every [`CjkTokenizer`] in the process. Per-call cost is linear in
//! the input length plus dictionary lookups (`O(n log m)` for jieba's
//! trie, `O(n)` for lindera's double-array).
//!
//! # Binary size
//!
//! - `cjk` (jieba alone): adds ~5 MiB of compressed dictionary.
//! - `cjk-japanese` (jieba + IPADIC): adds ~50 MiB.
//! - `cjk-korean` (jieba + ko-dic): adds ~150 MiB.
//! - All three: adds ~205 MiB.
//!
//! For wasm or tightly size-constrained binaries, prefer `cjk` alone
//! and accept UAX-29 fallback for Japanese / Korean.
//!
//! [`OnceLock`]: std::sync::OnceLock

use alloc::borrow::Cow;
use alloc::boxed::Box;
#[cfg(any(feature = "cjk-japanese", feature = "cjk-korean"))]
use alloc::string::String;
#[cfg(any(feature = "cjk-japanese", feature = "cjk-korean"))]
use alloc::vec::Vec;
use std::sync::OnceLock;

use jieba_rs::Jieba;

use super::{TokenStream, Tokenizer};

/// Underlying segmenter selection.
///
/// New variants may be added in patch releases as additional
/// dictionary languages land. Match exhaustively only inside this
/// crate; downstream code should use a `_` arm.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CjkSegmenter {
    /// `jieba-rs` with the default Simplified-Chinese dictionary.
    Jieba,
    /// `lindera` + IPADIC (Japanese). Real morphological tokenization
    /// when the `cjk-japanese` feature is enabled; UAX-29 fallback
    /// otherwise.
    Lindera,
    /// `lindera` + ko-dic (Korean). Real morphological tokenization
    /// when the `cjk-korean` feature is enabled; UAX-29 fallback
    /// otherwise.
    LinderaKoDic,
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
    /// * `segmenter` — one of [`CjkSegmenter::Jieba`],
    ///   [`CjkSegmenter::Lindera`], or [`CjkSegmenter::LinderaKoDic`].
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
    ///
    /// Has no effect on the [`CjkSegmenter::Lindera`] /
    /// [`CjkSegmenter::LinderaKoDic`] variants.
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

// ── Lindera helpers (feature-gated) ─────────────────────────────────────

#[cfg(feature = "cjk-japanese")]
fn lindera_ipadic() -> &'static lindera::tokenizer::Tokenizer {
    use lindera::dictionary::load_dictionary;
    use lindera::mode::Mode;
    use lindera::segmenter::Segmenter;
    use lindera::tokenizer::Tokenizer as LinderaTokenizer;

    static TOKENIZER: OnceLock<LinderaTokenizer> = OnceLock::new();
    TOKENIZER.get_or_init(|| {
        let dictionary =
            load_dictionary("embedded://ipadic").expect("embedded IPADIC dictionary should load");
        let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
        LinderaTokenizer::new(segmenter)
    })
}

#[cfg(feature = "cjk-korean")]
fn lindera_kodic() -> &'static lindera::tokenizer::Tokenizer {
    use lindera::dictionary::load_dictionary;
    use lindera::mode::Mode;
    use lindera::segmenter::Segmenter;
    use lindera::tokenizer::Tokenizer as LinderaTokenizer;

    static TOKENIZER: OnceLock<LinderaTokenizer> = OnceLock::new();
    TOKENIZER.get_or_init(|| {
        let dictionary =
            load_dictionary("embedded://ko-dic").expect("embedded ko-dic dictionary should load");
        let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
        LinderaTokenizer::new(segmenter)
    })
}

#[cfg(any(feature = "cjk-japanese", feature = "cjk-korean"))]
fn lindera_segments(tokenizer: &lindera::tokenizer::Tokenizer, input: &str) -> Vec<String> {
    match tokenizer.tokenize(input) {
        Ok(toks) => toks
            .into_iter()
            .filter_map(|tok| {
                let s = tok.surface.as_ref().to_string();
                if s.trim().is_empty() { None } else { Some(s) }
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

// ── Trait impl ──────────────────────────────────────────────────────────

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

            #[cfg(feature = "cjk-japanese")]
            CjkSegmenter::Lindera => {
                let segs = lindera_segments(lindera_ipadic(), input);
                TokenStream::Owned(Box::new(segs.into_iter()))
            }
            #[cfg(not(feature = "cjk-japanese"))]
            CjkSegmenter::Lindera => uax29_fallback(input),

            #[cfg(feature = "cjk-korean")]
            CjkSegmenter::LinderaKoDic => {
                let segs = lindera_segments(lindera_kodic(), input);
                TokenStream::Owned(Box::new(segs.into_iter()))
            }
            #[cfg(not(feature = "cjk-korean"))]
            CjkSegmenter::LinderaKoDic => uax29_fallback(input),
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

            #[cfg(feature = "cjk-japanese")]
            CjkSegmenter::Lindera => {
                let segs = lindera_segments(lindera_ipadic(), input);
                for s in &segs {
                    f(s);
                }
            }
            #[cfg(not(feature = "cjk-japanese"))]
            CjkSegmenter::Lindera => uax29_fallback_callback(input, f),

            #[cfg(feature = "cjk-korean")]
            CjkSegmenter::LinderaKoDic => {
                let segs = lindera_segments(lindera_kodic(), input);
                for s in &segs {
                    f(s);
                }
            }
            #[cfg(not(feature = "cjk-korean"))]
            CjkSegmenter::LinderaKoDic => uax29_fallback_callback(input, f),
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
            CjkSegmenter::Lindera => Cow::Borrowed("cjk-lindera-ipadic"),
            CjkSegmenter::LinderaKoDic => Cow::Borrowed("cjk-lindera-ko-dic"),
        }
    }
}

/// UAX-29 word fallback used when the requested lindera dictionary
/// feature isn't enabled at compile time.
#[cfg(any(not(feature = "cjk-japanese"), not(feature = "cjk-korean")))]
fn uax29_fallback<'a>(input: &'a str) -> TokenStream<'a> {
    let it =
        unicode_segmentation::UnicodeSegmentation::unicode_words(input).filter(|s| !s.is_empty());
    TokenStream::Borrowed(Box::new(it))
}

#[cfg(any(not(feature = "cjk-japanese"), not(feature = "cjk-korean")))]
fn uax29_fallback_callback(input: &str, f: &mut dyn FnMut(&str)) {
    for w in unicode_segmentation::UnicodeSegmentation::unicode_words(input) {
        if !w.is_empty() {
            f(w);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "cjk-lindera-ipadic"
        );
        assert_eq!(
            CjkTokenizer::new(CjkSegmenter::LinderaKoDic).name(),
            "cjk-lindera-ko-dic"
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
    fn lindera_variant_runs_without_panic() {
        let t = CjkTokenizer::new(CjkSegmenter::Lindera);
        let toks = collect("私は日本語を勉強しています", &t);
        assert!(!toks.is_empty());
    }

    #[test]
    fn ko_dic_variant_runs_without_panic() {
        let t = CjkTokenizer::new(CjkSegmenter::LinderaKoDic);
        let toks = collect("안녕하세요 한국어로 만나서 반갑습니다", &t);
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

    #[cfg(feature = "cjk-japanese")]
    #[test]
    fn lindera_japanese_finds_grammatical_tokens() {
        let t = CjkTokenizer::new(CjkSegmenter::Lindera);
        // IPADIC produces multi-token segmentation of Japanese, unlike
        // the UAX-29 fallback which lumps codepoints.
        let toks = collect("関西国際空港限定", &t);
        assert!(
            toks.len() >= 2,
            "expected multi-token segmentation: {toks:?}"
        );
    }

    #[cfg(feature = "cjk-korean")]
    #[test]
    fn lindera_korean_finds_grammatical_tokens() {
        let t = CjkTokenizer::new(CjkSegmenter::LinderaKoDic);
        let toks = collect("안녕하세요 반갑습니다", &t);
        assert!(
            toks.len() >= 2,
            "expected multi-token segmentation: {toks:?}"
        );
    }
}
