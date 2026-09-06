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

use unicode_normalization::UnicodeNormalization;

mod bidi;
mod casefold;

/// True if `c` should be dropped under the configured strip flags.
///
/// Used to *pre*-filter the char stream before NFC/NFKC reordering.
/// Stripping after normalization breaks idempotence when a bidi or
/// format char (a ccc=0 "starter") sits between combining marks:
/// NFC respects the format char as a sequence boundary on the first
/// pass, then post-strip removes it, and a second `canonicalize` call
/// merges what were two combining sequences into one and re-sorts by
/// canonical combining class — producing a different output.
#[inline]
fn should_drop(c: char, drop_bidi: bool, drop_fmt: bool) -> bool {
    (drop_bidi && bidi::is_bidi_control(c)) || (drop_fmt && bidi::is_format(c))
}

#[cfg(feature = "security")]
#[cfg_attr(docsrs, doc(cfg(feature = "security")))]
mod confusable;

/// Unicode normalization form selection.
///
/// # Variants
///
/// | Variant  | Use case                                                       |
/// | -------- | -------------------------------------------------------------- |
/// | [`Nfc`]  | Strict equivalence; preserves full-width and other compat forms.|
/// | [`Nfkc`] | Compat folding (full-width → ASCII, ﬁ → fi). **Default.**      |
/// | [`None`] | Caller has already normalized upstream.                        |
///
/// [`Nfc`]: Self::Nfc
/// [`Nfkc`]: Self::Nfkc
/// [`None`]: Self::None
///
/// # Example
///
/// ```
/// use txtfp::{Canonicalizer, CanonicalizerBuilder, Normalization};
///
/// let nfc = CanonicalizerBuilder { normalization: Normalization::Nfc, ..Default::default() }
///     .build();
/// let nfkc = Canonicalizer::default();                               // NFKC by default
///
/// // Full-width letters (U+FF21..) collapse to ASCII only under NFKC.
/// // NFC preserves the full-width codepoint (case-folded to lowercase
/// // full-width); NFKC compat-decomposes to plain ASCII.
/// assert_ne!(nfc.canonicalize("ＡＢＣ"),  "abc");
/// assert_eq!(nfkc.canonicalize("ＡＢＣ"), "abc");
/// ```
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
/// `txtfp` only ships [`Simple`] because locale-aware folds (Turkish
/// dotless I, Azeri) destroy reproducibility across machines: the same
/// input would produce different fingerprints depending on the host
/// locale.
///
/// # Example
///
/// ```
/// use txtfp::{Canonicalizer, CanonicalizerBuilder, CaseFold};
///
/// let folded = Canonicalizer::default().canonicalize("HELLO");
/// assert_eq!(folded, "hello");
///
/// let preserved = CanonicalizerBuilder { case_fold: CaseFold::None, ..Default::default() }
///     .build()
///     .canonicalize("HELLO");
/// // Case is preserved; only NFKC + Bidi/format strip ran.
/// assert_eq!(preserved, "HELLO");
/// ```
///
/// [`Simple`]: Self::Simple
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
    ///
    /// `strip_format` is a **superset** of [`Self::strip_bidi`]: the
    /// `Cf` category includes the Bidi-control codepoints, so setting
    /// `strip_format = true` always removes them, even when
    /// `strip_bidi = false`. To keep Bidi controls and strip only the
    /// remaining `Cf` codepoints, you would need to drop `strip_format`
    /// and apply your own filter — that path is not exposed.
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
    ///
    /// # Returns
    ///
    /// A `Canonicalizer` configured with the builder's fields. The
    /// resulting canonicalizer is `Send + Sync` and cheap to clone.
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::{CanonicalizerBuilder, Normalization};
    ///
    /// let c = CanonicalizerBuilder {
    ///     normalization: Normalization::Nfc,
    ///     ..Default::default()
    /// }
    /// .build();
    /// assert_eq!(c.config_string(), "nfc-cf-simple-bidi-fmt");
    /// ```
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
    ///
    /// # Arguments
    ///
    /// * `builder` — the [`CanonicalizerBuilder`] whose configuration the
    ///   new instance should adopt.
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::{Canonicalizer, CanonicalizerBuilder};
    ///
    /// let c = Canonicalizer::new(CanonicalizerBuilder::default());
    /// assert_eq!(c.canonicalize("Hello\u{200B}World"), "helloworld");
    /// ```
    #[inline]
    #[must_use]
    pub fn new(builder: CanonicalizerBuilder) -> Self {
        builder.build()
    }

    /// Borrow the builder this instance was constructed from.
    ///
    /// Useful when you want to inspect or clone-and-tweak an existing
    /// canonicalizer's configuration without reconstructing it from
    /// scratch.
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
    /// Two ASCII fast paths sit ahead of the full Unicode pipeline,
    /// both gated on the production default configuration (`NFKC`,
    /// simple casefold, bidi+format strip, no confusable skeleton):
    ///
    /// 1. **Pure ASCII** — single-pass [`to_ascii_lowercase`]. NFC,
    ///    NFKC, simple casefold, and the strip phases are all no-ops on
    ///    ASCII codepoints, so the output is byte-stable with the slow
    ///    path.
    /// 2. **ASCII + droppable format/bidi codepoints** — covers
    ///    BOM-prefixed text, ZWSP injection, RLO Trojan-Source attacks,
    ///    variation selectors on ASCII bases, etc. Common in CSV / web
    ///    text where a UTF-8 BOM precedes otherwise-ASCII content.
    ///    Equivalent to the slow pipeline on these inputs (NFKC is the
    ///    identity on ASCII + isolated format/bidi chars; the strip
    ///    phase removes them; casefold simple on ASCII = `to_ascii_lowercase`).
    ///
    /// Anything that needs real Unicode work (non-ASCII letters,
    /// compat-form decomposition, multi-char folds) falls through to
    /// the full pipeline.
    ///
    /// [`to_ascii_lowercase`]: str::to_ascii_lowercase
    #[must_use]
    pub fn canonicalize(&self, input: &str) -> String {
        let mut out = String::with_capacity(input.len() + (input.len() >> 4));
        self.canonicalize_into(input, &mut out);
        out
    }

    /// Canonicalize `input` into `out`, reusing the caller's buffer.
    ///
    /// `out` is cleared first and its existing allocation is retained,
    /// so corpus loops that canonicalize many documents save one
    /// allocation per document:
    ///
    /// ```
    /// use txtfp::Canonicalizer;
    ///
    /// let c = Canonicalizer::default();
    /// let mut buf = String::new();
    /// for doc in ["Hello World", "Second doc"] {
    ///     c.canonicalize_into(doc, &mut buf);
    ///     assert_eq!(buf, if doc == "Hello World" { "hello world" } else { "second doc" });
    /// }
    /// ```
    ///
    /// Semantics are identical to [`Canonicalizer::canonicalize`]; the
    /// fast paths are byte-identical with the single-pass behaviour of
    /// that method.
    pub fn canonicalize_into(&self, input: &str, out: &mut String) {
        out.clear();
        if self.is_default_pipeline() {
            if input.is_ascii() {
                // Bulk copy + in-place byte lowercasing. Byte-identical
                // to the per-char map: every codepoint is ASCII, and
                // `make_ascii_lowercase` is exactly `char::to_ascii_lowercase`
                // applied byte-wise — but as one memcpy plus one
                // vectorized pass instead of a per-char encode loop.
                out.push_str(input);
                out.make_ascii_lowercase();
                return;
            }
            // Pre-scan: if every non-ASCII char is a droppable bidi or
            // format codepoint, we can fast-path by dropping them and
            // ASCII-lowercasing the rest. `chars().all` short-circuits
            // on the first disqualifying char so mixed Unicode falls
            // through immediately.
            if input
                .chars()
                .all(|c| c.is_ascii() || bidi::is_bidi_control(c) || bidi::is_format(c))
            {
                // Single byte-scan copy: ASCII runs between droppable
                // codepoints are memcpy'd whole (the pre-scan proved the
                // only non-ASCII chars are droppable, so any multi-byte
                // sequence starting here is one of them — skip it by its
                // continuation bytes). Slices land on char boundaries by
                // construction: lead bytes and EOF both are boundaries,
                // and checked slicing re-verifies.
                let bytes = input.as_bytes();
                let mut last = 0;
                let mut i = 0;
                while i < bytes.len() {
                    if bytes[i] < 0x80 {
                        i += 1;
                        continue;
                    }
                    let mut j = i + 1;
                    while j < bytes.len() && (bytes[j] & 0xC0) == 0x80 {
                        j += 1;
                    }
                    out.push_str(&input[last..i]);
                    last = j;
                    i = j;
                }
                out.push_str(&input[last..]);
                out.make_ascii_lowercase();
                return;
            }
        }

        // 1+2. Bidi/format strip *fused into* normalization, with
        // strip happening *upstream* of NFC/NFKC. Stripping after
        // normalization is unsound (see `should_drop`): when a bidi
        // or format codepoint sits between two combining marks, NFC
        // treats it as a sequence boundary on the first pass, post-
        // strip removes it, and a second `canonicalize` call then
        // merges-and-reorders the marks — breaking idempotence.
        // Pre-filter via `Iterator::filter` keeps the single-pass
        // streaming property (no intermediate `String`).
        let drop_bidi = self.cfg.strip_bidi;
        let drop_fmt = self.cfg.strip_format;
        let stripped = input
            .chars()
            .filter(|&c| !should_drop(c, drop_bidi, drop_fmt));
        match self.cfg.normalization {
            Normalization::Nfkc => out.extend(stripped.nfkc()),
            Normalization::Nfc => out.extend(stripped.nfc()),
            Normalization::None => out.extend(stripped),
        }

        // 3. Casefold over the fused result. Kept as a separate
        // whole-string call so multi-char folds (German `ß` → `ss`,
        // Greek final-sigma) match the reference output exactly.
        //
        // After folding we *re-normalize*: simple casefold can expand
        // a single starter codepoint into a starter + combining mark
        // (e.g. `İ` U+0130 → `i` + U+0307, ccc=230). If another mark
        // with smaller ccc follows in the original text, the folded
        // sequence is no longer in canonical order, and a second
        // `canonicalize` call would reorder it — breaking idempotence.
        // This is the standard UAX #15 NFKC_Casefold construction
        // (NFKC → toCasefold → NFKC); the second normalization is a
        // local repair and is a no-op on inputs without expanding
        // folds adjacent to combining marks (the common case).
        if matches!(self.cfg.case_fold, CaseFold::Simple) {
            let folded = casefold::simple(out);
            *out = folded;
            // Re-normalize only if the fold produced non-ASCII (expanding
            // folds like İ → i + combining dot). ASCII is already in
            // canonical form — skip the O(n) second normalization pass.
            if !out.is_ascii() {
                let tmp = core::mem::take(out);
                *out = match self.cfg.normalization {
                    Normalization::Nfkc => tmp.nfkc().collect(),
                    Normalization::Nfc => tmp.nfc().collect(),
                    Normalization::None => tmp,
                };
            }
        }

        // 4. Confusable skeleton (security feature).
        #[cfg(feature = "security")]
        {
            if self.cfg.apply_confusable {
                let skeleton = confusable::skeleton(out);
                *out = skeleton;
            }
        }
        #[cfg(not(feature = "security"))]
        {
            // Builder permits the bool but the feature is off; ignore.
            let _ = self.cfg.apply_confusable;
        }
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
    /// The format is a concatenation of `<normalization>-cf-<casefold>`
    /// followed by optional `-bidi`, `-fmt`, `-conf` segments depending
    /// on which strip steps are enabled. Frozen for v0.1.x.
    ///
    /// # Returns
    ///
    /// A `String` such as `"nfkc-cf-simple-bidi-fmt"` (the default
    /// configuration).
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::{Canonicalizer, CanonicalizerBuilder, Normalization};
    ///
    /// assert_eq!(Canonicalizer::default().config_string(), "nfkc-cf-simple-bidi-fmt");
    ///
    /// let c = CanonicalizerBuilder {
    ///     normalization: Normalization::Nfc,
    ///     strip_bidi: false,
    ///     ..Default::default()
    /// }
    /// .build();
    /// assert_eq!(c.config_string(), "nfc-cf-simple-fmt");
    /// ```
    ///
    /// Used by [`crate::config_hash`] so a stored fingerprint can be
    /// compared safely against a query fingerprint produced with the
    /// same canonicalizer.
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
///
/// Equivalent to `Canonicalizer::default().canonicalize(input)`. Prefer
/// the struct method when canonicalizing many inputs in a loop — it
/// avoids re-constructing the [`Canonicalizer`] each call (constructors
/// are cheap, but not free).
///
/// # Arguments
///
/// * `input` — UTF-8 text to canonicalize.
///
/// # Returns
///
/// The canonicalized form. Output length is at most `18 × input.len()`
/// (Unicode-spec-mandated NFKC expansion bound; in practice 1.05–1.2×
/// for natural text).
///
/// # Example
///
/// ```
/// use txtfp::canonicalize;
///
/// assert_eq!(canonicalize("Hello\u{200B}World"), "helloworld");
/// ```
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
    fn canonicalize_into_matches_canonicalize() {
        let c = Canonicalizer::default();
        let mut buf = String::new();
        for input in [
            "",
            "Hello World",
            "ＡＢＣ ﬁle",
            "café résumé",
            "admin\u{202E}drow",
            "a\u{FE0F}b",
            "Façade — Ｔｅｓｔ\u{202E}rev\u{200B}",
            "İ\u{329}",
        ] {
            c.canonicalize_into(input, &mut buf);
            assert_eq!(buf, c.canonicalize(input), "input = {input:?}");
        }
    }

    #[test]
    fn canonicalize_into_reuses_capacity() {
        let c = Canonicalizer::default();
        let mut buf = String::with_capacity(1024);
        c.canonicalize_into("Hello World", &mut buf);
        assert!(buf.capacity() >= 1024, "buffer reallocation occurred");
        assert_eq!(buf, "hello world");
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
    fn idempotence_with_expanding_casefold_before_combining_mark() {
        // Regression for the second v0.2.1 fuzzer crash. Layout:
        // `İ` U+0130 (LATIN CAPITAL I WITH DOT ABOVE) — `\u{329}`
        // (COMBINING VERTICAL LINE BELOW, ccc=220).
        //
        // Simple casefold expands `İ` → `i` + U+0307 (COMBINING DOT
        // ABOVE, ccc=230). Without a post-casefold normalization, the
        // resulting `i \u{307} \u{329}` (230 then 220) is not in
        // canonical order, and a second `canonicalize` call reorders
        // to `i \u{329} \u{307}` (220 then 230), breaking idempotence.
        let c = Canonicalizer::default();
        let input = "İ\u{329}";
        let a = c.canonicalize(input);
        assert_eq!(c.canonicalize(&a), a);
    }

    #[test]
    fn idempotence_under_normalization_none_with_casefold() {
        // Sister coverage to the two fuzzer-found bugs: with NFC/NFKC
        // disabled, the post-casefold re-normalization is a no-op, so
        // idempotence has to rely on (a) strip being idempotent and
        // (b) simple casefold being idempotent on its own output.
        // Both hold; this test pins them so a future change to either
        // helper can't quietly break the `Normalization::None` path.
        let c = CanonicalizerBuilder {
            normalization: Normalization::None,
            ..Default::default()
        }
        .build();
        for input in &[
            "İ\u{329}",
            "\u{6e4}\u{202a}\u{6e4}\u{6ea}-\u{2}\u{3}",
            "\n(\u{b}462ǉİ\u{329}",
        ] {
            let a = c.canonicalize(input);
            assert_eq!(c.canonicalize(&a), a, "input = {input:?}");
        }
    }

    #[cfg(feature = "security")]
    #[test]
    fn idempotence_with_confusable_skeleton() {
        // The confusable skeleton (UTS #39 §4) outputs NFD. When it
        // runs as the final pipeline step, the canonicalize result
        // ends in NFD; on a subsequent call NFKC re-composes to NFC,
        // but the trailing skeleton step decomposes back to NFD —
        // the pipeline self-stabilizes. Pin that behavior.
        let c = CanonicalizerBuilder {
            apply_confusable: true,
            ..Default::default()
        }
        .build();
        for input in &[
            "café",     // precomposed letter
            "naïveté",  // multiple precomposed
            "раураl",   // Cyrillic homoglyph
            "İ\u{329}", // expanding-fold + combining mark
            "\u{6e4}\u{202a}\u{6e4}\u{6ea}-\u{2}\u{3}",
        ] {
            let a = c.canonicalize(input);
            assert_eq!(c.canonicalize(&a), a, "input = {input:?}");
        }
    }

    #[test]
    fn idempotence_with_format_char_between_combining_marks() {
        // Regression for the v0.2.1 fuzzer crash
        // (canonicalize/crash-29794a7407ffdce66af1ba8e08ad8b4a11345c61).
        //
        // Layout: ARABIC SMALL HIGH MADDA (ccc=230) — LRE (bidi/format,
        // ccc=0) — ARABIC SMALL HIGH MADDA (ccc=230) — ARABIC EMPTY
        // CENTRE LOW STOP (ccc=220). Pre-fix, the first pass treated
        // U+202A as a sequence boundary, then stripped it; the second
        // pass reordered the resulting marks by ccc, producing a
        // different output. Post-fix, strip happens before NFC so the
        // first pass already sees one combining sequence.
        let c = Canonicalizer::default();
        let input = "\u{6e4}\u{202a}\u{6e4}\u{6ea}-\u{2}\u{3}";
        let a = c.canonicalize(input);
        assert_eq!(c.canonicalize(&a), a);
    }

    #[test]
    fn config_round_trip_via_to_string() {
        let s = Canonicalizer::default().config_string();
        // Returned value owns its memory.
        let _: String = s.to_string();
    }
}
