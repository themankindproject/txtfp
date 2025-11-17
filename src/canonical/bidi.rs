//! Strip format-category and Bidi-control codepoints.
//!
//! These codepoints are invisible to humans but change byte sequences,
//! producing inputs that look identical but hash differently. Stripping
//! them defends against zero-width-joiner injection, BOM leakage, and
//! Trojan Source (CVE-2021-42574).

use alloc::string::String;

/// Returns true if `c` is a Unicode Bidi-control codepoint.
#[inline]
fn is_bidi_control(c: char) -> bool {
    matches!(
        c,
        '\u{202A}'..='\u{202E}' // LRE, RLE, PDF, LRO, RLO
        | '\u{2066}'..='\u{2069}' // LRI, RLI, FSI, PDI
        | '\u{200E}'              // LRM
        | '\u{200F}'              // RLM
    )
}

/// Returns true if `c` belongs to the Unicode general category `Cf`
/// (format) per Unicode 16, treated as "stripable" by `txtfp`.
///
/// We hand-curate this list rather than depending on a `unicode-properties`
/// crate, because the set is stable across Unicode versions and small
/// enough to inline.
#[inline]
fn is_format(c: char) -> bool {
    matches!(
        c,
        // ZWSP, ZWNJ, ZWJ, LRM, RLM
        '\u{200B}'..='\u{200F}'
        // WJ, function-application, …, invisible-times, invisible-comma
        | '\u{2060}'..='\u{2064}'
        // ALM
        | '\u{061C}'
        // Mongolian free variation selectors
        | '\u{180B}'..='\u{180E}'
        | '\u{180F}'
        // BOM / ZWNBSP
        | '\u{FEFF}'
        // Variation selectors VS1..VS16
        | '\u{FE00}'..='\u{FE0F}'
        // Variation selectors supplement VS17..VS256
        | '\u{E0100}'..='\u{E01EF}'
        // Tag characters (used in flag emoji etc.)
        | '\u{E0001}'
        | '\u{E0020}'..='\u{E007F}'
        // Bidi controls (also reach is_bidi_control, included here so a
        // caller passing strip_format=true alone still removes them).
        | '\u{202A}'..='\u{202E}'
        | '\u{2066}'..='\u{2069}'
    )
}

/// Drop Bidi controls and/or format characters per the flags.
///
/// Allocates a new `String` of at most `input.len()` bytes.
pub(super) fn strip(input: &str, drop_bidi: bool, drop_format: bool) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.chars() {
        let is_bidi = is_bidi_control(c);
        let is_fmt = is_format(c);
        if drop_bidi && is_bidi {
            continue;
        }
        if drop_format && is_fmt {
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_zwsp() {
        assert_eq!(strip("a\u{200B}b", false, true), "ab");
    }

    #[test]
    fn drops_bom() {
        assert_eq!(strip("\u{FEFF}hello", false, true), "hello");
    }

    #[test]
    fn drops_rlo() {
        assert_eq!(strip("admin\u{202E}", true, false), "admin");
    }

    #[test]
    fn drops_variation_selector_16() {
        assert_eq!(strip("a\u{FE0F}", false, true), "a");
    }

    #[test]
    fn drops_tag_char() {
        assert_eq!(strip("\u{E0061}", false, true), "");
    }

    #[test]
    fn keeps_normal_chars() {
        let s = "héllo, мир!";
        assert_eq!(strip(s, true, true), s);
    }

    #[test]
    fn flags_are_independent() {
        // strip_bidi only — Cf-but-not-bidi survives.
        assert_eq!(strip("a\u{200B}\u{202E}b", true, false), "a\u{200B}b");
    }

    #[test]
    fn empty_passes_through() {
        assert_eq!(strip("", true, true), "");
    }
}
