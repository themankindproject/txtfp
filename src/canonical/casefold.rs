//! Default Unicode case fold via [`caseless`].
//!
//! Locale-independent. We deliberately do not expose Turkish or Azeri
//! folds — they break reproducibility across machines.

use alloc::string::String;
use caseless::default_case_fold_str;

/// Apply the simple Unicode case fold.
#[inline]
pub(super) fn simple(input: &str) -> String {
    default_case_fold_str(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_upper_lowers() {
        assert_eq!(simple("HELLO"), "hello");
    }

    #[test]
    fn german_eszett() {
        assert_eq!(simple("STRASSE"), "strasse");
        assert_eq!(simple("STRAßE"), "strasse");
    }

    #[test]
    fn greek_sigma() {
        // Capital sigma → lowercase sigma (not final-sigma; case-fold
        // collapses to the non-final form).
        assert_eq!(simple("ΣΟΦΙΑ"), "σοφια");
    }

    #[test]
    fn idempotent() {
        let s = simple("ΩMega");
        assert_eq!(simple(&s), s);
    }
}
