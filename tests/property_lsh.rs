//! Property tests for the banded LSH index.
//!
//! The index's `query_with_threshold` must agree with a brute-force
//! O(N) Jaccard scan for *random* signatures — unit tests only cover
//! hand-picked corpora, which can hide band-collision accounting bugs.

use proptest::prelude::*;
use txtfp::{LshIndex, MinHashSig, jaccard};

const H: usize = 32;

fn sig_of(hashes: &[u64; H]) -> MinHashSig<H> {
    MinHashSig {
        schema: 1,
        _pad: [0; 6],
        hashes: *hashes,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// `query_with_threshold` returns exactly the ids a full scan would
    /// (same ascending, deduped order).
    #[test]
    fn query_with_threshold_matches_brute_force(
        sigs in prop::collection::vec(prop::array::uniform32(any::<u64>()), 0..24),
        q in prop::array::uniform32(any::<u64>()),
    ) {
        let mut idx = LshIndex::<H>::with_bands_rows(8, 4).unwrap();
        for (i, s) in sigs.iter().enumerate() {
            idx.insert(i as u64, sig_of(s));
        }
        let query = sig_of(&q);

        let threshold = 0.5;
        let got = idx.query_with_threshold(&query, threshold);

        let mut brute: Vec<u64> = sigs
            .iter()
            .enumerate()
            .filter(|(_, s)| jaccard(&query, &sig_of(s)) >= threshold)
            .map(|(i, _)| i as u64)
            .collect();
        brute.sort_unstable();
        assert_eq!(got, brute, "sigs = {sigs:?}");
    }

    /// The unfiltered `query` is a superset of the threshold-filtered
    /// one, and both are strictly ascending with no duplicates.
    #[test]
    fn query_is_sorted_deduped_superset(
        sigs in prop::collection::vec(prop::array::uniform32(any::<u64>()), 0..24),
        q in prop::array::uniform32(any::<u64>()),
    ) {
        let mut idx = LshIndex::<H>::with_bands_rows(8, 4).unwrap();
        for (i, s) in sigs.iter().enumerate() {
            idx.insert(i as u64, sig_of(s));
        }
        let query = sig_of(&q);

        let filtered = idx.query_with_threshold(&query, 0.5);
        let all = idx.query(&query);

        for id in &filtered {
            assert!(all.contains(id), "filtered id {id} missing from query()");
        }
        assert!(
            all.windows(2).all(|w| w[0] < w[1]),
            "query() not strictly sorted: {all:?}"
        );
    }
}
