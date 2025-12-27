//! Proptest invariants for the canonicalizer.

use proptest::prelude::*;
use txtfp::Canonicalizer;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    /// `canonicalize(canonicalize(x)) == canonicalize(x)`.
    #[test]
    fn idempotence(s in any::<String>()) {
        let c = Canonicalizer::default();
        let a = c.canonicalize(&s);
        let b = c.canonicalize(&a);
        prop_assert_eq!(a, b);
    }

    /// Output never contains stripped Bidi or format codepoints.
    #[test]
    fn no_bidi_or_format_in_output(s in any::<String>()) {
        let c = Canonicalizer::default();
        let out = c.canonicalize(&s);
        for ch in out.chars() {
            prop_assert!(
                !matches!(
                    ch,
                    '\u{202A}'..='\u{202E}'
                    | '\u{2066}'..='\u{2069}'
                    | '\u{200B}'..='\u{200F}'
                    | '\u{2060}'..='\u{2064}'
                    | '\u{FEFF}'
                    | '\u{FE00}'..='\u{FE0F}'
                ),
                "stripped codepoint U+{:04X} survived: {:?}",
                ch as u32,
                out
            );
        }
    }

    /// Bounded blow-up: output is at most ~4× input length.
    /// (NFKC's worst case is 18× per codepoint but that's pathological;
    /// natural strings cluster well below 2×.)
    #[test]
    fn bounded_size_blowup(s in any::<String>()) {
        let c = Canonicalizer::default();
        let out = c.canonicalize(&s);
        let limit = (s.len() * 18).max(64);
        prop_assert!(out.len() <= limit);
    }
}
