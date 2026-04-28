//! Strip format-category and Bidi-control codepoints.
//!
//! These codepoints are invisible to humans but change byte sequences,
//! producing inputs that look identical but hash differently. Stripping
//! them defends against zero-width-joiner injection, BOM leakage, and
//! Trojan Source (CVE-2021-42574).

/// Returns true if `c` is a Unicode Bidi-control codepoint.
#[inline]
pub(super) fn is_bidi_control(c: char) -> bool {
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
pub(super) fn is_format(c: char) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zwsp_is_format() {
        assert!(is_format('\u{200B}'));
    }

    #[test]
    fn bom_is_format() {
        assert!(is_format('\u{FEFF}'));
    }

    #[test]
    fn rlo_is_bidi_control() {
        assert!(is_bidi_control('\u{202E}'));
    }

    #[test]
    fn variation_selector_16_is_format() {
        assert!(is_format('\u{FE0F}'));
    }

    #[test]
    fn tag_char_is_format() {
        assert!(is_format('\u{E0061}'));
    }

    #[test]
    fn ordinary_letters_are_neither() {
        for c in "héllo, мир!".chars() {
            assert!(!is_bidi_control(c));
            assert!(!is_format(c));
        }
    }
}
