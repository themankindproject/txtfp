//! [`LshIndex`] — banded LSH over MinHash signatures.

use alloc::vec::Vec;
use core::hash::{BuildHasherDefault, Hasher};

use hashbrown::HashMap;
use smallvec::SmallVec;

use crate::classical::minhash::{MinHashSig, jaccard};
use crate::error::{Error, Result};

/// Inline capacity for the per-band candidate list. Most bands hold at
/// most a handful of duplicates; once the count exceeds 4, we spill to
/// a heap allocation.
const CANDIDATE_INLINE: usize = 4;

/// Identity hasher for `u64` keys. The keys we store in the band tables
/// are already 64-bit `xxh3_64` digests (cryptographic-quality
/// non-cryptographic mixing), so re-hashing them through `ahash` /
/// `foldhash` is pure overhead. Calls to `write` other than the single
/// `write_u64` from `<u64 as Hash>::hash` are unsupported and panic
/// in debug to surface misuse early.
#[derive(Default, Clone, Copy)]
struct U64IdentityHasher(u64);

impl Hasher for U64IdentityHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
    #[inline]
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
    #[inline]
    fn write(&mut self, _bytes: &[u8]) {
        debug_assert!(false, "U64IdentityHasher only accepts u64 keys");
    }
}

type U64Hasher = BuildHasherDefault<U64IdentityHasher>;
type BandTable = HashMap<u64, SmallVec<[u64; CANDIDATE_INLINE]>, U64Hasher>;

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
    /// Keys are 64-bit `xxh3_64` digests — already cryptographic-quality
    /// distributed — so an identity hasher avoids re-mixing them.
    tables: Vec<BandTable>,
    /// Reverse map for query-time verification. Keys here are caller-
    /// supplied document ids which are often sequential / dense, so
    /// hashbrown's default mix function is used (identity hashing
    /// catastrophically clusters sequential u64s).
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
        // `checked_mul` rather than a bare `bands * rows`: an overflowing
        // product would wrap in release builds and could accidentally
        // equal `H`, bypassing the validation before the (infeasible)
        // `Vec::with_capacity(bands)` allocation.
        let product = bands.checked_mul(rows).ok_or_else(|| {
            Error::Config(alloc::format!("bands * rows overflow: {bands} * {rows}"))
        })?;
        if product != H {
            return Err(Error::Config(alloc::format!(
                "bands * rows ({bands} * {rows} = {product}) must equal H = {H}"
            )));
        }
        let mut tables = Vec::with_capacity(bands);
        for _ in 0..bands {
            tables.push(BandTable::with_hasher(U64Hasher::default()));
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

    /// All document ids currently in the index, in arbitrary order.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "lsh")]
    /// # {
    /// use txtfp::{LshIndex, MinHashSig};
    /// let mut idx = LshIndex::<128>::with_bands_rows(16, 8).unwrap();
    /// idx.insert(7, MinHashSig::empty());
    /// idx.insert(11, MinHashSig::empty());
    /// let mut ids: Vec<u64> = idx.ids().collect();
    /// ids.sort_unstable();
    /// assert_eq!(ids, vec![7, 11]);
    /// # }
    /// ```
    pub fn ids(&self) -> impl Iterator<Item = u64> + '_ {
        self.sigs.keys().copied()
    }

    /// Iterate `(id, signature)` pairs in arbitrary order.
    ///
    /// Useful for dumps and for integrators that need to re-verify a
    /// column without reconstructing the index.
    pub fn iter(&self) -> impl Iterator<Item = (u64, &MinHashSig<H>)> + '_ {
        self.sigs.iter().map(|(id, sig)| (*id, sig))
    }

    /// Insert a signature under `id`.
    ///
    /// If `id` already exists, the prior signature is scrubbed from
    /// every band table before the new one is banded. Re-inserting the
    /// same `(id, sig)` pair is idempotent.
    ///
    /// # Arguments
    ///
    /// * `id` — caller-supplied document identifier.
    /// * `sig` — MinHash signature with the same `H` as the index.
    ///
    /// # Performance
    ///
    /// `O(bands)` per call: one band-key hash + one hash-table insert
    /// per band. With mimalloc as the global allocator, this runs at
    /// ~500K signatures/sec for the default `H = 128` partition.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "lsh")]
    /// # {
    /// use txtfp::{LshIndex, MinHashSig};
    /// let mut idx = LshIndex::<128>::with_bands_rows(16, 8).unwrap();
    /// idx.insert(42, MinHashSig::empty());
    /// assert_eq!(idx.len(), 1);
    /// # }
    /// ```
    pub fn insert(&mut self, id: u64, sig: MinHashSig<H>) {
        // If id is being replaced, take the old signature out first and
        // scrub its band entries — a single reverse-map lookup, where
        // `contains_key` + `remove` would probe the map twice.
        if let Some(old) = self.sigs.remove(&id) {
            self.scrub_bands(id, &old);
        }
        for (band, table) in self.tables.iter_mut().enumerate() {
            let key = band_key(&sig, band, self.rows);
            table.entry(key).or_default().push(id);
        }
        self.sigs.insert(id, sig);
    }

    /// Drop `id`'s entries from every band table it participates in,
    /// under the *old* signature `sig`. Empty bucket lists are removed
    /// so memory stays bounded. Shared by [`Self::insert`] (replace
    /// path) and [`Self::remove`].
    fn scrub_bands(&mut self, id: u64, sig: &MinHashSig<H>) {
        for (band, table) in self.tables.iter_mut().enumerate() {
            let key = band_key(sig, band, self.rows);
            if let Some(list) = table.get_mut(&key) {
                list.retain(|v| *v != id);
                if list.is_empty() {
                    table.remove(&key);
                }
            }
        }
    }

    /// Bulk-insert a batch of `(id, sig)` pairs in parallel across the
    /// rayon thread pool.
    ///
    /// Work is sharded by band: each rayon worker owns exactly one
    /// band's hash table for the duration of the call, so there is no
    /// per-band contention. The reverse `id → sig` map is filled
    /// serially (it's the cheap part).
    ///
    /// # Constraints
    ///
    /// - **Ids must not already exist in the index.** Replacement is
    ///   not supported here; call [`LshIndex::remove`] first if you
    ///   need to overwrite. Pre-existing ids trigger a `debug_assert!`
    ///   panic in debug builds and silently corrupt the index in
    ///   release. For mixed insert/replace traffic, use
    ///   [`LshIndex::insert`] in a serial loop.
    /// - **Ids within `items` must be unique.** Duplicates within the
    ///   batch place the same id in multiple band buckets.
    ///
    /// # Performance
    ///
    /// `O(N × bands / cores)` band-key + bucket-insert work + serial
    /// `O(N)` reverse-map fill. Best speedup approaches `min(bands,
    /// cores)` — for the default `(b = 16, r = 8)` partition, 16+
    /// cores saturate the per-band pool.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(all(feature = "lsh", feature = "parallel"))]
    /// # fn demo() -> Result<(), txtfp::Error> {
    /// use txtfp::{
    ///     Canonicalizer, Fingerprinter, LshIndex,
    ///     MinHashFingerprinter, ShingleTokenizer, WordTokenizer,
    /// };
    ///
    /// let canon = Canonicalizer::default();
    /// let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
    /// let fp = MinHashFingerprinter::<_, 128>::new(canon, tok);
    ///
    /// let docs = ["alpha beta gamma", "delta epsilon zeta", "eta theta iota"];
    /// let pairs: Vec<_> = docs
    ///     .iter()
    ///     .enumerate()
    ///     .map(|(i, d)| Ok((i as u64, fp.fingerprint(d)?)))
    ///     .collect::<Result<_, txtfp::Error>>()?;
    ///
    /// let mut idx = LshIndex::<128>::with_bands_rows(16, 8)?;
    /// idx.extend_par(pairs);
    /// assert_eq!(idx.len(), 3);
    /// # Ok(()) }
    /// ```
    #[cfg(feature = "parallel")]
    #[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
    pub fn extend_par<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = (u64, MinHashSig<H>)>,
    {
        let items: alloc::vec::Vec<(u64, MinHashSig<H>)> = items.into_iter().collect();
        // The infallible variant documents "ids must not already exist"
        // as a contract; surface violations loudly in debug builds.
        debug_assert!(
            items.iter().all(|(id, _)| !self.sigs.contains_key(id)),
            "LshIndex::extend_par: id already exists; remove() first"
        );
        self.extend_par_inner(&items);
    }

    /// Shared body of [`LshIndex::extend_par`] and
    /// [`LshIndex::try_extend_par`]: serial reverse-map fill, then
    /// parallel per-band insertion sharded across the rayon pool.
    #[cfg(feature = "parallel")]
    fn extend_par_inner(&mut self, items: &[(u64, MinHashSig<H>)]) {
        use rayon::prelude::*;

        // Serial reverse-map fill (cheap; bounded by N hashtable inserts).
        for (id, sig) in items {
            self.sigs.insert(*id, *sig);
        }

        // Parallel per-band insertion. Each rayon worker takes one
        // band table and walks the full items slice under it — the
        // tables are disjoint so this is contention-free.
        let rows = self.rows;
        self.tables
            .par_iter_mut()
            .enumerate()
            .for_each(|(band, table)| {
                for (id, sig) in items {
                    let key = band_key(sig, band, rows);
                    table.entry(key).or_default().push(*id);
                }
            });
    }

    /// Fallible variant of [`LshIndex::extend_par`].
    ///
    /// Validates that **no** id in `items` already exists in the index
    /// before touching any state, and returns [`Error::InvalidInput`]
    /// naming the first offending id instead of corrupting the index.
    /// Ids within `items` must still be unique (not re-checked
    /// validation — duplicates within a batch place the same id in
    /// multiple band buckets).
    ///
    /// See [`LshIndex::extend_par`] for the performance contract —
    /// identical except for the pre-pass over the batch.
    #[cfg(feature = "parallel")]
    #[cfg_attr(docsrs, doc(cfg(feature = "parallel")))]
    pub fn try_extend_par<I>(&mut self, items: I) -> Result<()>
    where
        I: IntoIterator<Item = (u64, MinHashSig<H>)>,
    {
        let items: alloc::vec::Vec<(u64, MinHashSig<H>)> = items.into_iter().collect();
        // Validate the whole batch up front so a rejection never
        // leaves the index half-mutated.
        if let Some((id, _)) = items.iter().find(|(id, _)| self.sigs.contains_key(id)) {
            return Err(Error::InvalidInput(alloc::format!(
                "LshIndex::try_extend_par: id {id} already exists; remove() first"
            )));
        }
        self.extend_par_inner(&items);
        Ok(())
    }

    /// Remove `id` from the index.
    ///
    /// Scrubs the id from every band table whose key it currently
    /// participates in. Empty bucket lists are dropped from the table
    /// to keep memory bounded.
    ///
    /// # Returns
    ///
    /// `Some(sig)` with the signature that was stored, or `None` if
    /// `id` was not present.
    pub fn remove(&mut self, id: u64) -> Option<MinHashSig<H>> {
        let sig = self.sigs.remove(&id)?;
        self.scrub_bands(id, &sig);
        Some(sig)
    }

    /// Return ids whose signature collides with `sig` in **at least one**
    /// band. Result is deduplicated.
    ///
    /// This is the cheap, recall-tuned variant: it returns hash-bucket
    /// candidates without verifying the actual Jaccard. Use
    /// [`LshIndex::query_with_threshold`] for precision-tuned retrieval.
    ///
    /// # Arguments
    ///
    /// * `sig` — query signature.
    ///
    /// # Returns
    ///
    /// `Vec<u64>` of candidate ids in **ascending order**. Duplicates are
    /// removed (an id colliding in multiple bands is reported once).
    /// The order is incidental — callers should treat the order as
    /// unspecified beyond "ascending if you must compare two query
    /// outputs without re-sorting".
    ///
    /// # Performance
    ///
    /// `O(bands)` band-key hashes + `O(N log N)` sort/dedup over the
    /// concatenated candidate lists, where `N` is total candidates
    /// across bands. The sort path is cache-friendly and beats a
    /// per-call `HashSet` allocation for the typical load (≤ a few
    /// hundred candidates). Sub-millisecond on 1M-doc indices for the
    /// production `(b=16, r=8)` partition.
    #[must_use]
    pub fn query(&self, sig: &MinHashSig<H>) -> Vec<u64> {
        // Pre-size assuming each matched band contributes a handful of
        // ids; in adversarial-collision corpora this still fills before
        // the first re-allocation.
        let mut out: Vec<u64> = Vec::with_capacity(self.bands * 2);

        for (band, table) in self.tables.iter().enumerate() {
            let key = band_key(sig, band, self.rows);
            if let Some(list) = table.get(&key) {
                out.extend_from_slice(list);
            }
        }

        // sort_unstable + dedup: O(N log N) but cache-friendly; a single
        // contiguous Vec walk beats per-id hash-table inserts at the
        // candidate counts LSH actually produces. No HashSet allocation,
        // no rehashing across bucket boundaries.
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Return ids whose signature is at least `threshold` Jaccard-similar
    /// to `sig`.
    ///
    /// Internally calls [`query`] and then re-checks each candidate's
    /// actual Jaccard via [`crate::jaccard`], dropping any that fall
    /// below the threshold.
    ///
    /// # Arguments
    ///
    /// * `sig` — query signature.
    /// * `threshold` — minimum acceptable Jaccard. Values outside
    ///   `[0.0, 1.0]` are clamped.
    ///
    /// # Returns
    ///
    /// `Vec<u64>` of ids with `jaccard(sig, stored) >= threshold`.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "lsh")]
    /// # fn demo() -> Result<(), txtfp::Error> {
    /// use txtfp::{
    ///     Canonicalizer, Fingerprinter, LshIndex,
    ///     MinHashFingerprinter, ShingleTokenizer, WordTokenizer,
    /// };
    ///
    /// let canon = Canonicalizer::default();
    /// let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
    /// let fp = MinHashFingerprinter::<_, 128>::new(canon, tok);
    ///
    /// let mut idx = LshIndex::<128>::with_bands_rows(64, 2)?;
    /// idx.insert(1, fp.fingerprint("the quick brown fox jumps over the lazy dog at noon")?);
    ///
    /// let probe = fp.fingerprint("the quick brown fox jumps over the lazy dog at noon")?;
    /// let strict = idx.query_with_threshold(&probe, 0.95);
    /// assert!(strict.contains(&1));
    /// # Ok(()) }
    /// ```
    ///
    /// [`query`]: LshIndex::query
    #[must_use]
    pub fn query_with_threshold(&self, sig: &MinHashSig<H>, threshold: f32) -> Vec<u64> {
        let mut candidates = self.query(sig);
        let threshold = threshold.clamp(0.0, 1.0);
        // In-place filter avoids a second Vec allocation. Order is
        // preserved (ascending), matching `query()`'s contract.
        candidates.retain(|id| {
            self.sigs
                .get(id)
                .map(|other| jaccard(sig, other) >= threshold)
                .unwrap_or(false)
        });
        candidates
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
            ShingleTokenizer {
                k: 5,
                inner: WordTokenizer,
            },
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
    fn rejects_wrapping_product() {
        // (2^63 + 64) * 2 wraps to exactly 128 in release builds because
        // overflow checks are off; the checked_mul validation must catch
        // it instead of panicking inside Vec::with_capacity.
        let r = LshIndex::<128>::with_bands_rows((1usize << 63) | 64, 2);
        assert!(matches!(r, Err(Error::Config(_))));
        let r = LshIndex::<128>::with_bands_rows(usize::MAX, 2);
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
    fn ids_and_iter_enumerate_stored_docs() {
        let mut idx = make();
        let f = fp();
        idx.insert(7, f.fingerprint("alpha beta gamma").unwrap());
        idx.insert(11, f.fingerprint("delta epsilon zeta").unwrap());
        let mut ids: Vec<u64> = idx.ids().collect();
        ids.sort_unstable();
        assert_eq!(ids, vec![7, 11]);

        let mut pairs: Vec<_> = idx.iter().map(|(id, sig)| (id, sig.hashes[0])).collect();
        pairs.sort_by_key(|(id, _)| *id);
        assert_eq!(pairs.len(), 2);
    }

    #[test]
    fn self_query_hits() {
        let mut idx = make();
        let f = fp();
        let s = f
            .fingerprint("the quick brown fox jumps over the lazy dog")
            .unwrap();
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
        let s1 = f
            .fingerprint("the quick brown fox jumps over the lazy dog")
            .unwrap();
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

    #[cfg(feature = "parallel")]
    #[test]
    fn extend_par_matches_serial_insert() {
        let f = fp();
        let docs: alloc::vec::Vec<alloc::string::String> = (0..200)
            .map(|i| alloc::format!("the quick brown fox jumps over the lazy dog {i}"))
            .collect();
        let sigs: alloc::vec::Vec<_> = docs.iter().map(|d| f.fingerprint(d).unwrap()).collect();

        // Serial baseline.
        let mut serial = make();
        for (i, sig) in sigs.iter().enumerate() {
            serial.insert(i as u64, *sig);
        }

        // Parallel build.
        let mut parallel = make();
        let pairs: alloc::vec::Vec<_> = sigs
            .iter()
            .enumerate()
            .map(|(i, sig)| (i as u64, *sig))
            .collect();
        parallel.extend_par(pairs);

        assert_eq!(parallel.len(), serial.len());
        for i in 0..200u64 {
            assert_eq!(parallel.get(i), serial.get(i));
            // Same set of candidates returned for every probe.
            let mut p = parallel.query(serial.get(i).unwrap());
            let mut s = serial.query(serial.get(i).unwrap());
            p.sort_unstable();
            s.sort_unstable();
            assert_eq!(p, s, "candidate set differs for id {i}");
        }
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn try_extend_par_rejects_existing_ids() {
        let mut idx = make();
        let f = fp();
        idx.insert(1, f.fingerprint("alpha beta gamma").unwrap());
        let pairs: Vec<(u64, MinHashSig<128>)> = alloc::vec![
            (1, crate::classical::minhash::MinHashSig::empty()),
            (2, crate::classical::minhash::MinHashSig::empty())
        ];
        let r = idx.try_extend_par(pairs);
        assert!(matches!(r, Err(Error::InvalidInput(_))));
        // Rejected atomically: nothing from the batch was inserted.
        assert_eq!(idx.len(), 1);
    }

    #[cfg(feature = "parallel")]
    #[test]
    fn try_extend_par_matches_serial_insert() {
        let f = fp();
        let docs: alloc::vec::Vec<alloc::string::String> = (0..100)
            .map(|i| alloc::format!("the quick brown fox jumps over the lazy dog {i}"))
            .collect();
        let sigs: alloc::vec::Vec<_> = docs.iter().map(|d| f.fingerprint(d).unwrap()).collect();

        // Serial baseline.
        let mut serial = make();
        for (i, sig) in sigs.iter().enumerate() {
            serial.insert(i as u64, *sig);
        }

        // Parallel build via the fallible variant.
        let mut parallel = make();
        let pairs: alloc::vec::Vec<_> = sigs
            .iter()
            .enumerate()
            .map(|(i, sig)| (i as u64, *sig))
            .collect();
        parallel.try_extend_par(pairs).unwrap();

        assert_eq!(parallel.len(), serial.len());
        for i in 0..100u64 {
            assert_eq!(parallel.get(i), serial.get(i));
            let mut p = parallel.query(serial.get(i).unwrap());
            let mut s = serial.query(serial.get(i).unwrap());
            p.sort_unstable();
            s.sort_unstable();
            assert_eq!(p, s, "candidate set differs for id {i}");
        }
    }

    #[test]
    fn threshold_filter_drops_far_candidates() {
        let mut idx = make();
        let f = fp();
        let s1 = f
            .fingerprint("the quick brown fox jumps over the lazy dog")
            .unwrap();
        let s2 = f
            .fingerprint("the quick brown fox leaps over the lazy dog")
            .unwrap();
        idx.insert(1, s1);
        idx.insert(2, s2);

        let strict = idx.query_with_threshold(&s1, 0.95);
        assert!(strict.contains(&1));
        // s2's true Jaccard against s1 is well below 0.95.
        assert!(!strict.contains(&2));
    }
}
