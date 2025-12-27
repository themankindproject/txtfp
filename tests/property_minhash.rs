//! Proptest invariants for MinHash.
//!
//! Properties asserted:
//!
//! 1. **Self-similarity**: `jaccard(s, s) == 1.0` for any signature `s`.
//! 2. **Symmetry**: `jaccard(a, b) == jaccard(b, a)`.
//! 3. **Bounds**: `0.0 <= jaccard(a, b) <= 1.0`.
//! 4. **Permutation invariance** (set-style tokenizer): shuffling word
//!    order leaves the signature byte-identical when the inner tokenizer
//!    is `WordTokenizer` (no shingles).
//! 5. **Duplicate insensitivity**: doubling every word leaves the
//!    signature byte-identical.
//! 6. **Determinism**: re-fingerprinting the same input produces the
//!    same signature.
//! 7. **Streaming/offline parity**: the same input split into arbitrary
//!    chunks through the streaming sketcher matches the offline
//!    fingerprinter exactly.

use proptest::collection::vec;
use proptest::prelude::*;
use txtfp::{
    Canonicalizer, Fingerprinter, MinHashFingerprinter, MinHashStreaming, ShingleTokenizer,
    StreamingFingerprinter, WordTokenizer, jaccard,
};

fn fp_words() -> MinHashFingerprinter<WordTokenizer, 64> {
    MinHashFingerprinter::<_, 64>::new(Canonicalizer::default(), WordTokenizer)
}

fn fp_shingles() -> MinHashFingerprinter<ShingleTokenizer<WordTokenizer>, 64> {
    MinHashFingerprinter::<_, 64>::new(
        Canonicalizer::default(),
        ShingleTokenizer {
            k: 3,
            inner: WordTokenizer,
        },
    )
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
    fn self_similarity_is_one(doc in ascii_doc()) {
        let sig = fp_shingles().fingerprint(&doc).unwrap();
        prop_assert!((jaccard(&sig, &sig) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn jaccard_is_symmetric(a in ascii_doc(), b in ascii_doc()) {
        let sa = fp_shingles().fingerprint(&a).unwrap();
        let sb = fp_shingles().fingerprint(&b).unwrap();
        prop_assert_eq!(jaccard(&sa, &sb), jaccard(&sb, &sa));
    }

    #[test]
    fn jaccard_bounded(a in ascii_doc(), b in ascii_doc()) {
        let sa = fp_shingles().fingerprint(&a).unwrap();
        let sb = fp_shingles().fingerprint(&b).unwrap();
        let j = jaccard(&sa, &sb);
        prop_assert!((0.0..=1.0).contains(&j));
    }

    #[test]
    fn permutation_invariance(words in vec(ascii_word(), 8..30)) {
        // Word-only tokenizer => set semantics, order irrelevant.
        let original = words.join(" ");
        let mut shuffled = words;
        shuffled.reverse();
        let shuffled = shuffled.join(" ");

        let so = fp_words().fingerprint(&original).unwrap();
        let sr = fp_words().fingerprint(&shuffled).unwrap();
        prop_assert_eq!(so, sr);
    }

    #[test]
    fn duplicate_insensitivity(doc in ascii_doc()) {
        // Doubling each token cannot change the set, hence cannot change
        // the signature, when using the word tokenizer.
        let words: Vec<&str> = doc.split_whitespace().collect();
        let doubled = words
            .iter()
            .flat_map(|w| [*w, *w])
            .collect::<Vec<_>>()
            .join(" ");

        let s_orig = fp_words().fingerprint(&doc).unwrap();
        let s_dbl = fp_words().fingerprint(&doubled).unwrap();
        prop_assert_eq!(s_orig, s_dbl);
    }

    #[test]
    fn deterministic(doc in ascii_doc()) {
        let f = fp_shingles();
        let a = f.fingerprint(&doc).unwrap();
        let b = f.fingerprint(&doc).unwrap();
        prop_assert_eq!(a, b);
    }

    #[test]
    fn streaming_offline_parity(
        doc in ascii_doc(),
        chunk_sizes in vec(1usize..32, 1..16),
    ) {
        let bytes = doc.as_bytes();
        let mut s = MinHashStreaming::<_, 64>::new(MinHashFingerprinter::<_, 64>::new(
            Canonicalizer::default(),
            ShingleTokenizer { k: 3, inner: WordTokenizer },
        ));

        let mut cursor = 0;
        for &csz in &chunk_sizes {
            if cursor >= bytes.len() {
                break;
            }
            let end = (cursor + csz).min(bytes.len());
            // Walk back to a UTF-8 boundary.
            let mut adj = end;
            while adj > cursor && !doc.is_char_boundary(adj) {
                adj -= 1;
            }
            if adj == cursor {
                cursor = end;
                continue;
            }
            s.update(&bytes[cursor..adj]).unwrap();
            cursor = adj;
        }
        if cursor < bytes.len() {
            s.update(&bytes[cursor..]).unwrap();
        }

        let stream_sig = s.finalize().unwrap();
        let offline_sig = fp_shingles().fingerprint(&doc).unwrap();
        prop_assert_eq!(stream_sig, offline_sig);
    }
}
