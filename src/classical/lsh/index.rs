//! [`LshIndex`] — banded LSH over MinHash signatures.

use alloc::vec::Vec;

use hashbrown::HashMap;
use smallvec::SmallVec;

use crate::classical::minhash::{MinHashSig, jaccard};
use crate::error::{Error, Result};

/// Inline capacity for the per-band candidate list. Most bands hold at
/// most a handful of duplicates; once the count exceeds 4, we spill to
/// a heap allocation.
const CANDIDATE_INLINE: usize = 4;

/// Banded LSH index keyed by `u64` document id.
///
/// `H` is the MinHash signature width (e.g. 128); `bands * rows` must
/// equal `H` (enforced at construction).
///
/// # Memory
///
/// Storage cost is `bands * |inserted_ids|` u64 hash-table entries plus
/// the full signatures (one per id). For 1M docs at H=128 with the
/// default `bands=16, rows=8` layout that's ~16M hash-table entries
/// (~256 MiB) + 128 MiB of signatures = ~384 MiB total.
///
/// # Hashing
///
/// Each band's row slice is reduced to a 64-bit `xxh3` digest used as
/// the hash-table key. Hash collisions in a band are possible but rare
/// (`< 2^-32` for typical loads); they are filtered by the optional
/// post-verification in [`LshIndex::query_with_threshold`], which
/// recomputes the actual Jaccard similarity for each candidate.
pub struct LshIndex<const H: usize> {
    bands: usize,
    rows: usize,
    /// One open-addressed hash table per band: `band_key → list of doc ids`.
    tables: Vec<HashMap<u64, SmallVec<[u64; CANDIDATE_INLINE]>>>,
    /// Reverse map for query-time verification.
    sigs: HashMap<u64, MinHashSig<H>>,
}

impl<const H: usize> LshIndex<H> {
    /// Construct an empty index with the given band/row partition.
    ///
    /// Returns [`Error::Config`] if `bands * rows != H` or if either is zero.
    pub fn with_bands_rows(bands: usize, rows: usize) -> Result<Self> {
        if bands == 0 || rows == 0 {
            return Err(Error::Config("bands and rows must be > 0".into()));
        }
        if bands * rows != H {
            return Err(Error::Config(alloc::format!(
                "bands * rows ({} * {} = {}) must equal H = {}",
                bands,
                rows,
                bands * rows,
                H,
            )));
        }
        let mut tables = Vec::with_capacity(bands);
        for _ in 0..bands {
            tables.push(HashMap::new());
        }
        Ok(Self {
            bands,
            rows,
            tables,
            sigs: HashMap::new(),
        })
    }

    /// Number of bands.
    #[inline]
    #[must_use]
    pub fn bands(&self) -> usize {
        self.bands
    }

    /// Rows per band.
    #[inline]
    #[must_use]
    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Number of distinct ids in the index.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.sigs.len()
    }

    /// True if the index has no entries.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sigs.is_empty()
    }

    /// Borrow the signature stored under `id`, if any.
    #[inline]
    #[must_use]
    pub fn get(&self, id: u64) -> Option<&MinHashSig<H>> {
        self.sigs.get(&id)
    }

    /// Insert a signature under `id`. Replaces any prior signature with
    /// the same id (it is also re-banded).
    pub fn insert(&mut self, id: u64, sig: MinHashSig<H>) {
        // If id is being replaced, scrub its old band entries first.
        if self.sigs.contains_key(&id) {
            self.remove(id);
        }
        for (band, table) in self.tables.iter_mut().enumerate() {
            let key = band_key(&sig, band, self.rows);
            table.entry(key).or_default().push(id);
        }
        self.sigs.insert(id, sig);
    }

    /// Remove `id` from the index, returning its signature if present.
    pub fn remove(&mut self, id: u64) -> Option<MinHashSig<H>> {
        let sig = self.sigs.remove(&id)?;
        for (band, table) in self.tables.iter_mut().enumerate() {
            let key = band_key(&sig, band, self.rows);
            if let Some(list) = table.get_mut(&key) {
                list.retain(|v| *v != id);
                if list.is_empty() {
                    table.remove(&key);
                }
            }
        }
        Some(sig)
    }

    /// Return ids whose signature collides with `sig` in **at least one**
    /// band. Result is deduplicated.
    ///
    /// This is the cheap, recall-tuned variant: it returns hash-bucket
    /// candidates without verifying the actual Jaccard. Use
    /// [`LshIndex::query_with_threshold`] for precision-tuned retrieval.
    #[must_use]
    pub fn query(&self, sig: &MinHashSig<H>) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        let mut seen: HashMap<u64, ()> = HashMap::new();

        for (band, table) in self.tables.iter().enumerate() {
            let key = band_key(sig, band, self.rows);
            if let Some(list) = table.get(&key) {
                for &id in list {
                    if seen.insert(id, ()).is_none() {
                        out.push(id);
                    }
                }
            }
        }
        out
    }

    /// Return ids whose signature is at least `threshold` Jaccard-similar
    /// to `sig`.
    ///
    /// Internally calls [`query`] and then re-checks each candidate's
    /// actual Jaccard, dropping any that fall below the threshold.
    ///
    /// `threshold` should be in `[0.0, 1.0]`; values outside that range
    /// are treated as the corresponding endpoint.
    ///
    /// [`query`]: LshIndex::query
    #[must_use]
    pub fn query_with_threshold(&self, sig: &MinHashSig<H>, threshold: f32) -> Vec<u64> {
        let candidates = self.query(sig);
        let threshold = threshold.clamp(0.0, 1.0);
        candidates
            .into_iter()
            .filter(|id| {
                self.sigs
                    .get(id)
                    .map(|other| jaccard(sig, other) >= threshold)
                    .unwrap_or(false)
            })
            .collect()
    }
}

// SAFETY-equivalent: hashbrown's HashMap and smallvec's SmallVec are
// `Send + Sync` whenever their element types are. We don't add interior
// mutability, so the auto-derived Send/Sync from the field types is
// correct here.

/// Hash a band of `rows` u64s into a 64-bit bucket key.
///
/// Uses `xxh3_64`. We cast the band slice to bytes via `bytemuck` —
/// `[u64]` is `bytemuck::Pod` so this is zero-copy.
fn band_key<const H: usize>(sig: &MinHashSig<H>, band: usize, rows: usize) -> u64 {
    let start = band * rows;
    let end = start + rows;
    debug_assert!(end <= H, "band slice out of range");
    let slice = &sig.hashes[start..end];
    let bytes = bytemuck::cast_slice::<u64, u8>(slice);
    xxhash_rust::xxh3::xxh3_64(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::Canonicalizer;
    use crate::classical::Fingerprinter;
    use crate::classical::minhash::MinHashFingerprinter;
    use crate::tokenize::{ShingleTokenizer, WordTokenizer};

    fn make() -> LshIndex<128> {
        LshIndex::<128>::with_bands_rows(16, 8).unwrap()
    }

    fn fp() -> MinHashFingerprinter<ShingleTokenizer<WordTokenizer>, 128> {
        MinHashFingerprinter::<_, 128>::new(
            Canonicalizer::default(),
            ShingleTokenizer { k: 5, inner: WordTokenizer },
        )
    }

    #[test]
    fn rejects_mismatched_h() {
        let r = LshIndex::<128>::with_bands_rows(7, 9);
        assert!(matches!(r, Err(Error::Config(_))));
    }

    #[test]
    fn rejects_zero_dimensions() {
        let r = LshIndex::<128>::with_bands_rows(0, 128);
        assert!(matches!(r, Err(Error::Config(_))));
        let r = LshIndex::<128>::with_bands_rows(128, 0);
        assert!(matches!(r, Err(Error::Config(_))));
    }

    #[test]
    fn empty_index() {
        let idx = make();
        assert!(idx.is_empty());
        assert_eq!(idx.len(), 0);
        assert_eq!(idx.bands(), 16);
        assert_eq!(idx.rows(), 8);
    }

    #[test]
    fn insert_and_get() {
        let mut idx = make();
        let f = fp();
        let s = f.fingerprint("the quick brown fox jumps").unwrap();
        idx.insert(42, s);
        assert_eq!(idx.len(), 1);
        assert_eq!(idx.get(42), Some(&s));
        assert!(idx.get(43).is_none());
    }

    #[test]
    fn self_query_hits() {
        let mut idx = make();
        let f = fp();
        let s = f.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
        idx.insert(7, s);
        let neighbours = idx.query(&s);
        assert_eq!(neighbours, alloc::vec![7]);
    }

    #[test]
    fn near_duplicate_is_a_candidate() {
        // Use a recall-tuned partition (b=64, r=2) so a true Jaccard of
        // ~0.6 collides with probability ~1.0. The default (b=16, r=8)
        // is precision-tuned for Jaccard ≥ 0.85 and would miss this
        // pair by design.
        let mut idx = LshIndex::<128>::with_bands_rows(64, 2).unwrap();
        let f = fp();
        let s1 = f
            .fingerprint("the quick brown fox jumps over the lazy dog at noon today")
            .unwrap();
        let s2 = f
            .fingerprint("the quick brown fox jumps over the lazy dog at dusk today")
            .unwrap();
        idx.insert(1, s1);
        idx.insert(2, s2);
        let mut hits = idx.query(&s1);
        hits.sort();
        assert!(hits.contains(&1));
        assert!(hits.contains(&2), "near-duplicate missed: {hits:?}");
    }

    #[test]
    fn dissimilar_doc_does_not_collide() {
        let mut idx = make();
        let f = fp();
        let s1 = f.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
        let s2 = f
            .fingerprint("astronomers detect cosmic background radiation in space")
            .unwrap();
        idx.insert(1, s1);
        idx.insert(2, s2);
        let hits = idx.query(&s1);
        assert!(hits.contains(&1));
        // With Jaccard ~ 0 and (b=16, r=8) the collision probability is
        // astronomically small.
        assert!(!hits.contains(&2), "false positive: {hits:?}");
    }

    #[test]
    fn dedup_repeat_inserts() {
        let mut idx = make();
        let f = fp();
        let s = f.fingerprint("the quick brown fox").unwrap();
        idx.insert(1, s);
        idx.insert(1, s);
        idx.insert(1, s);
        assert_eq!(idx.len(), 1);
        let hits = idx.query(&s);
        assert_eq!(hits, alloc::vec![1]);
    }

    #[test]
    fn replace_changes_signature() {
        let mut idx = make();
        let f = fp();
        let s1 = f.fingerprint("alpha beta gamma delta epsilon").unwrap();
        let s2 = f.fingerprint("zeta eta theta iota kappa").unwrap();
        idx.insert(1, s1);
        idx.insert(1, s2);
        assert_eq!(idx.get(1), Some(&s2));
        // Querying with s2 finds id 1, querying with s1 should not.
        assert_eq!(idx.query(&s2), alloc::vec![1]);
        let hits = idx.query(&s1);
        assert!(!hits.contains(&1), "old bands not scrubbed: {hits:?}");
    }

    #[test]
    fn remove_takes_signature_out() {
        let mut idx = make();
        let f = fp();
        let s = f.fingerprint("the quick brown fox").unwrap();
        idx.insert(1, s);
        let removed = idx.remove(1);
        assert_eq!(removed, Some(s));
        assert!(idx.is_empty());
        assert!(idx.query(&s).is_empty());
    }

    #[test]
    fn remove_missing_returns_none() {
        let mut idx = make();
        assert!(idx.remove(99).is_none());
    }

    #[test]
    fn threshold_filter_drops_far_candidates() {
        let mut idx = make();
        let f = fp();
        let s1 = f.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
        let s2 = f.fingerprint("the quick brown fox leaps over the lazy dog").unwrap();
        idx.insert(1, s1);
        idx.insert(2, s2);

        let strict = idx.query_with_threshold(&s1, 0.95);
        assert!(strict.contains(&1));
        // s2's true Jaccard against s1 is well below 0.95.
        assert!(!strict.contains(&2));
    }
}
