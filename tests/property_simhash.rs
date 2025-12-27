//! Proptest invariants for SimHash.

use proptest::collection::vec;
use proptest::prelude::*;
use txtfp::{Canonicalizer, Fingerprinter, SimHashFingerprinter, WordTokenizer, hamming};

fn fp() -> SimHashFingerprinter<WordTokenizer> {
    SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer)
}

fn ascii_word() -> impl Strategy<Value = String> {
    "[a-z]{3,8}".prop_map(|s| s)
}

fn ascii_doc() -> impl Strategy<Value = String> {
    vec(ascii_word(), 8..40).prop_map(|ws| ws.join(" "))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn self_distance_is_zero(doc in ascii_doc()) {
        let s = fp().fingerprint(&doc).unwrap();
        prop_assert_eq!(hamming(s, s), 0);
    }

    #[test]
    fn deterministic(doc in ascii_doc()) {
        let f = fp();
        let a = f.fingerprint(&doc).unwrap();
        let b = f.fingerprint(&doc).unwrap();
        prop_assert_eq!(a, b);
    }

    #[test]
    fn hamming_bounded(a in ascii_doc(), b in ascii_doc()) {
        let sa = fp().fingerprint(&a).unwrap();
        let sb = fp().fingerprint(&b).unwrap();
        let h = hamming(sa, sb);
        prop_assert!(h <= 64);
    }

    #[test]
    fn hamming_symmetric(a in ascii_doc(), b in ascii_doc()) {
        let sa = fp().fingerprint(&a).unwrap();
        let sb = fp().fingerprint(&b).unwrap();
        prop_assert_eq!(hamming(sa, sb), hamming(sb, sa));
    }
}
