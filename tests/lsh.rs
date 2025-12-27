//! LSH end-to-end: build a synthetic corpus, sketch with MinHash,
//! insert into LSH, query.

#![cfg(feature = "lsh")]

use txtfp::{
    Canonicalizer, Fingerprinter, LshIndex, LshIndexBuilder, MinHashFingerprinter,
    ShingleTokenizer, WordTokenizer,
};

fn make_corpus() -> Vec<(u64, &'static str)> {
    vec![
        (1, "the quick brown fox jumps over the lazy dog at noon"),
        (2, "the quick brown fox jumps over the lazy dog at dusk"),
        (3, "the quick brown fox jumps over the lazy dog at dawn"),
        (
            4,
            "a completely unrelated paragraph about astronomy and stars",
        ),
        (5, "yet another distinct piece of text describing kittens"),
        (6, "the quick brown fox leaps over the lazy dog at noon"),
    ]
}

fn fp() -> MinHashFingerprinter<ShingleTokenizer<WordTokenizer>, 128> {
    MinHashFingerprinter::<_, 128>::new(
        Canonicalizer::default(),
        ShingleTokenizer {
            k: 5,
            inner: WordTokenizer,
        },
    )
}

#[test]
fn for_threshold_picks_valid_partition_at_high_recall() {
    let b = LshIndexBuilder::for_threshold(0.5, 128).unwrap();
    assert_eq!(b.bands * b.rows, 128);
}

#[test]
fn end_to_end_recall_high_for_close_pairs() {
    let f = fp();
    let mut idx: LshIndex<128> = LshIndex::with_bands_rows(64, 2).unwrap();
    let corpus = make_corpus();
    for (id, text) in &corpus {
        idx.insert(*id, f.fingerprint(text).unwrap());
    }

    // Probe with a near-duplicate of doc 1; expect doc 1, 2, 3, 6 at
    // least (all share most of the leading shingles).
    let probe = f
        .fingerprint("the quick brown fox jumps over the lazy dog at noon today")
        .unwrap();
    let hits = idx.query(&probe);
    for expected in [1u64, 2, 3, 6] {
        assert!(
            hits.contains(&expected),
            "missed near-duplicate id={expected}: hits={hits:?}"
        );
    }
}

#[test]
fn end_to_end_precision_drops_unrelated() {
    let f = fp();
    let mut idx: LshIndex<128> = LshIndexBuilder::for_threshold(0.85, 128).unwrap().build();
    let corpus = make_corpus();
    for (id, text) in &corpus {
        idx.insert(*id, f.fingerprint(text).unwrap());
    }

    let probe = f.fingerprint(corpus[0].1).unwrap();
    let hits = idx.query(&probe);
    // Should hit itself.
    assert!(hits.contains(&corpus[0].0));
    // Should NOT hit completely unrelated docs (4, 5).
    assert!(!hits.contains(&4));
    assert!(!hits.contains(&5));
}

#[test]
fn threshold_filter_post_verifies_jaccard() {
    let f = fp();
    let mut idx: LshIndex<128> = LshIndex::with_bands_rows(64, 2).unwrap();
    let corpus = make_corpus();
    for (id, text) in &corpus {
        idx.insert(*id, f.fingerprint(text).unwrap());
    }

    let probe = f.fingerprint(corpus[0].1).unwrap();
    let strict = idx.query_with_threshold(&probe, 0.95);
    // At Jaccard >= 0.95 only the exact match qualifies.
    assert_eq!(strict, vec![corpus[0].0]);
}

#[test]
fn remove_round_trip() {
    let f = fp();
    let mut idx: LshIndex<128> = LshIndex::with_bands_rows(64, 2).unwrap();
    let sig = f.fingerprint("the quick brown fox").unwrap();
    idx.insert(99, sig);
    assert!(idx.query(&sig).contains(&99));
    let _ = idx.remove(99);
    assert!(idx.query(&sig).is_empty());
}
