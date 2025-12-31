//! Canonicalization pipeline.
//!
//! The default pipeline is a five-stage transformation that produces a
//! `String` suitable for downstream tokenization:
//!
//! ```text
//! decode → NFKC normalize → simple casefold → drop Cf + Bidi controls
//!        → optional UTS #39 confusable skeleton (security feature)
//! ```
//!
//! [`Canonicalizer`] is `Send + Sync` and holds no mutable state — share
//! one across threads.
//!
//! # Example
//!
//! ```
//! use txtfp::canonical::Canonicalizer;
//!
//! let c = Canonicalizer::default();
//! // ZWSP and casing are erased.
//! let a = c.canonicalize("Hello\u{200B}World");
//! assert_eq!(a, "helloworld");
//! ```

use alloc::string::String;

mod bidi;
mod casefold;
mod normalize;

#[cfg(feature = "security")]
#[cfg_attr(docsrs, doc(cfg(feature = "security")))]
mod confusable;

/// Unicode normalization form selection.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Normalization {
    /// Apply NFC. Composing-only; leaves compatibility variants intact
    /// (e.g. `ﬁ` ligature stays a ligature).
    Nfc,
    /// Apply NFKC. Compatibility-decomposing then composing — collapses
    /// ligatures, full-width forms, and superscripts. **The default**, and
    /// the right choice for fingerprinting.
    Nfkc,
    /// Skip normalization. Reserved for callers that already canonicalize
    /// upstream.
    None,
}

/// Case-folding strategy.
///
/// `txtfp` only ships `Simple` because locale-aware folds (Turkish dotless
/// I, Azeri) destroy reproducibility across machines.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum CaseFold {
    /// Skip case folding entirely.
    None,
    /// Default Unicode case fold via [`caseless`]. Locale-independent.
    Simple,
}

/// Builder controlling the steps of [`Canonicalizer::canonicalize`].
///
/// Construct with [`CanonicalizerBuilder::default`] for the production
/// pipeline (`NFKC`, simple casefold, Bidi + format strip).
#[derive(Clone, Debug)]
pub struct CanonicalizerBuilder {
    /// Unicode normalization form to apply.
    pub normalization: Normalization,
    /// Case-folding strategy.
    pub case_fold: CaseFold,
    /// Strip Bidi-control codepoints (defends against
    /// [Trojan Source](https://trojansource.codes/), CVE-2021-42574).
    pub strip_bidi: bool,
    /// Strip the format (`Cf`) general category — zero-widths, BOM,
    /// variation selectors, tag chars.
    pub strip_format: bool,
    /// Apply the UTS #39 confusable skeleton on top of NFKC. Available
    /// only with the `security` feature.
    pub apply_confusable: bool,
}

impl Default for CanonicalizerBuilder {
    fn default() -> Self {
        Self {
            normalization: Normalization::Nfkc,
            case_fold: CaseFold::Simple,
            strip_bidi: true,
            strip_format: true,
            apply_confusable: false,
        }
    }
}

impl CanonicalizerBuilder {
    /// Finish the builder and produce a stateless [`Canonicalizer`].
    #[inline]
    #[must_use]
    pub fn build(self) -> Canonicalizer {
        Canonicalizer { cfg: self }
    }
}

/// Stateless text canonicalizer.
///
/// `Canonicalizer` instances are cheap to construct, hold no mutable
/// state, and are safe to share across threads.
#[derive(Clone, Debug)]
pub struct Canonicalizer {
    cfg: CanonicalizerBuilder,
}

impl Default for Canonicalizer {
    #[inline]
    fn default() -> Self {
        CanonicalizerBuilder::default().build()
    }
}

impl Canonicalizer {
    /// Construct a canonicalizer from an explicit builder.
    #[inline]
    #[must_use]
    pub fn new(builder: CanonicalizerBuilder) -> Self {
        builder.build()
    }

    /// Borrow the builder this instance was constructed from.
    #[inline]
    #[must_use]
    pub fn config(&self) -> &CanonicalizerBuilder {
        &self.cfg
    }

    /// Canonicalize `input` per the configured pipeline.
    ///
    /// Cost is `O(n)` in the input length. The output is at most a
    /// constant factor larger than the input (Unicode caps NFKC expansion
    /// at 18× per codepoint; in practice 1.05–1.2× for natural text).
    ///
    /// # Fast path
    ///
    /// When the input is pure ASCII and the configuration is the
    /// production default (`NFKC`, simple casefold, bidi+format strip,
    /// no confusable skeleton), this method skips Unicode normalization
    /// entirely and falls through to a single-pass ASCII lowercase. NFC,
    /// NFKC, simple casefold, and the strip phases are all no-ops on
    /// ASCII codepoints, so the fast path is byte-stable with the slow
    /// path.
    #[must_use]
    pub fn canonicalize(&self, input: &str) -> String {
        if self.is_default_pipeline() && input.is_ascii() {
            return input.to_ascii_lowercase();
        }

        // 1. Normalization.
        let mut buf: String = match self.cfg.normalization {
            Normalization::Nfc => normalize::nfc(input),
            Normalization::Nfkc => normalize::nfkc(input),
            Normalization::None => String::from(input),
        };

        // 2. Bidi / format strip.
        if self.cfg.strip_bidi || self.cfg.strip_format {
            buf = bidi::strip(&buf, self.cfg.strip_bidi, self.cfg.strip_format);
        }

        // 3. Casefold.
        if matches!(self.cfg.case_fold, CaseFold::Simple) {
            buf = casefold::simple(&buf);
        }

        // 4. Confusable skeleton (security feature).
        #[cfg(feature = "security")]
        {
            if self.cfg.apply_confusable {
                buf = confusable::skeleton(&buf);
            }
        }
        #[cfg(not(feature = "security"))]
        {
            // Builder permits the bool but the feature is off; ignore.
            let _ = self.cfg.apply_confusable;
        }

        buf
    }

    /// True if the configuration is the production default — used to
    /// gate the ASCII fast path in [`Canonicalizer::canonicalize`].
    #[inline]
    fn is_default_pipeline(&self) -> bool {
        matches!(self.cfg.normalization, Normalization::Nfkc)
            && matches!(self.cfg.case_fold, CaseFold::Simple)
            && self.cfg.strip_bidi
            && self.cfg.strip_format
            && !self.cfg.apply_confusable
    }

    /// Stable string identifier for the canonicalizer's config.
    ///
    /// Used by [`crate::FingerprintMetadata::config_hash`] so a stored
    /// fingerprint can be compared safely against a query fingerprint
    /// produced with the same canonicalizer.
    #[must_use]
    pub fn config_string(&self) -> String {
        let mut s = String::with_capacity(32);
        s.push_str(match self.cfg.normalization {
            Normalization::Nfc => "nfc",
            Normalization::Nfkc => "nfkc",
            Normalization::None => "none",
        });
        s.push('-');
        s.push_str(match self.cfg.case_fold {
            CaseFold::Simple => "cf-simple",
            CaseFold::None => "cf-none",
        });
        if self.cfg.strip_bidi {
            s.push_str("-bidi");
        }
        if self.cfg.strip_format {
            s.push_str("-fmt");
        }
        if self.cfg.apply_confusable {
            s.push_str("-conf");
        }
        s
    }
}

/// Convenience: canonicalize `input` with the default pipeline.
#[inline]
#[must_use]
pub fn canonicalize(input: &str) -> String {
    Canonicalizer::default().canonicalize(input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn default_lowercases_and_strips_zwsp() {
        let c = Canonicalizer::default();
        assert_eq!(c.canonicalize("Hello\u{200B}World"), "helloworld");
    }

    #[test]
    fn nfkc_collapses_full_width() {
        let c = Canonicalizer::default();
        assert_eq!(c.canonicalize("ＡＢＣ"), "abc");
    }

    #[test]
    fn nfkc_collapses_ligature() {
        let c = Canonicalizer::default();
        assert_eq!(c.canonicalize("ﬁle"), "file");
    }

    #[test]
    fn idempotence() {
        let c = Canonicalizer::default();
        let a = c.canonicalize("Façade — Ｔｅｓｔ\u{202E}rev\u{200B}");
        let b = c.canonicalize(&a);
        assert_eq!(a, b);
    }

    #[test]
    fn config_string_is_stable() {
        let c = Canonicalizer::default();
        assert_eq!(c.config_string(), "nfkc-cf-simple-bidi-fmt");
    }

    #[test]
    fn convenience_function_matches_default() {
        let direct = canonicalize("Mixed CASE");
        let viaobj = Canonicalizer::default().canonicalize("Mixed CASE");
        assert_eq!(direct, viaobj);
    }

    #[test]
    fn none_normalization_passes_through() {
        let c = CanonicalizerBuilder {
            normalization: Normalization::None,
            case_fold: CaseFold::None,
            strip_bidi: false,
            strip_format: false,
            apply_confusable: false,
        }
        .build();
        assert_eq!(c.canonicalize("HéLLo"), "HéLLo");
    }

    #[test]
    fn bidi_strip_kills_rlo() {
        let c = Canonicalizer::default();
        // RLO injection (Trojan Source).
        let s = c.canonicalize("admin\u{202E}gnirts");
        assert!(!s.contains('\u{202E}'));
    }

    #[test]
    fn casefold_does_not_use_turkish_locale() {
        // Default fold maps Turkish capital İ via the simple fold, not
        // the Turkish-locale fold. The test asserts the simple-fold result.
        let c = Canonicalizer::default();
        let folded = c.canonicalize("İ");
        // Expect i + combining-dot-above, *not* Turkish dotless 'ı'.
        assert!(folded.contains('i'));
        assert!(!folded.contains('ı'), "got: {folded:?}");
    }

    #[test]
    fn config_string_reflects_overrides() {
        let c = CanonicalizerBuilder {
            normalization: Normalization::Nfc,
            case_fold: CaseFold::None,
            strip_bidi: false,
            strip_format: true,
            apply_confusable: false,
        }
        .build();
        assert_eq!(c.config_string(), "nfc-cf-none-fmt");
    }

    #[test]
    fn canonicalizer_is_send_sync() {
        fn assert_traits<T: Send + Sync>() {}
        assert_traits::<Canonicalizer>();
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert_eq!(canonicalize(""), "");
    }

    #[test]
    fn variation_selector_is_stripped() {
        let c = Canonicalizer::default();
        // U+FE0F is a Cf-category variation selector.
        assert_eq!(c.canonicalize("a\u{FE0F}"), "a");
    }

    #[test]
    fn idempotence_on_arabic() {
        let c = Canonicalizer::default();
        let a = c.canonicalize("الْعَرَبِيَّة");
        assert_eq!(c.canonicalize(&a), a);
    }

    #[test]
    fn config_round_trip_via_to_string() {
        let s = Canonicalizer::default().config_string();
        // Returned value owns its memory.
        let _: String = s.to_string();
    }
}
