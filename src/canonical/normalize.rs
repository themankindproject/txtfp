//! NFC / NFKC wrappers over [`unicode_normalization`].

use alloc::string::String;
use unicode_normalization::UnicodeNormalization;

/// Apply NFC.
#[inline]
pub(super) fn nfc(input: &str) -> String {
    input.nfc().collect()
}

/// Apply NFKC.
#[inline]
pub(super) fn nfkc(input: &str) -> String {
    input.nfkc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfc_composes_combining_marks() {
        // 'e' + combining acute → é (precomposed).
        let composed = nfc("e\u{0301}");
        assert_eq!(composed, "é");
    }

    #[test]
    fn nfkc_collapses_compat_form() {
        assert_eq!(nfkc("ﬁ"), "fi");
    }

    #[test]
    fn nfc_leaves_ligature_alone() {
        assert_eq!(nfc("ﬁ"), "ﬁ");
    }

    #[test]
    fn nfkc_handles_full_width_digits() {
        assert_eq!(nfkc("１２３"), "123");
    }

    #[test]
    fn nfc_idempotent() {
        let s = nfc("café");
        assert_eq!(nfc(&s), s);
    }
}
