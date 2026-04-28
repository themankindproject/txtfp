//! Offline SimHash fingerprinter.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;

use crate::canonical::Canonicalizer;
use crate::classical::Fingerprinter;
use crate::classical::hash::{HashFamily, hash128};
use crate::error::{Error, Result};
use crate::tokenize::Tokenizer;

use super::sig::SimHash64;

/// Default seed used for the inner hash family.
pub const DEFAULT_SEED: u64 = 0x00C0_FFEE_5EED;

/// Per-token weighting strategy.
#[derive(Clone, Debug)]
pub enum Weighting {
    /// Each distinct token contributes weight 1, regardless of frequency.
    Uniform,
    /// Each token contributes weight equal to its term frequency in the
    /// document.
    Tf,
    /// Each token's weight is its TF × IDF, where IDF is read from the
    /// supplied [`IdfTable`]. Tokens absent from the table get IDF = 1
    /// (i.e., reduce to TF).
    IdfWeighted(IdfTable),
}

impl Default for Weighting {
    fn default() -> Self {
        Self::Tf
    }
}

/// Inverse-document-frequency table.
///
/// Opaque to callers; build via [`IdfTable::from_pairs`]. We deliberately
/// do not ship a default corpus — IDF values are corpus-specific and
/// shipping a single default would mislead users into thinking their
/// own corpus's stop-words match Brown / Wikipedia / web-2024.
#[derive(Clone, Debug, Default)]
pub struct IdfTable {
    inner: Arc<BTreeMap<String, f32>>,
}

impl IdfTable {
    /// Build from any iterator of `(token, idf)` pairs.
    ///
    /// Last value wins for duplicate tokens (the iterator is consumed
    /// in order).
    ///
    /// # Arguments
    ///
    /// * `pairs` — iterator yielding `(token, idf)` tuples. Token may
    ///   be any type that converts to `String`.
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::IdfTable;
    ///
    /// let table = IdfTable::from_pairs([
    ///     ("the", 0.1_f32),
    ///     ("dog", 4.0_f32),
    /// ]);
    /// assert_eq!(table.len(), 2);
    /// assert!((table.get("the") - 0.1).abs() < 1e-6);
    /// ```
    pub fn from_pairs<I, S>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (S, f32)>,
        S: Into<String>,
    {
        let mut m = BTreeMap::new();
        for (k, v) in pairs {
            m.insert(k.into(), v);
        }
        Self { inner: Arc::new(m) }
    }

    /// Lookup.
    ///
    /// # Returns
    ///
    /// The IDF for `token`, or `1.0` if `token` is absent. The fallback
    /// value collapses the SimHash weighting to plain TF for unseen
    /// vocabulary, which is the safe default — it never poisons the
    /// accumulator.
    #[inline]
    #[must_use]
    pub fn get(&self, token: &str) -> f32 {
        self.inner.get(token).copied().unwrap_or(1.0)
    }

    /// Number of distinct tokens in the table.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// True if the table is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

/// Builder for [`SimHashFingerprinter`].
#[derive(Clone, Debug)]
pub struct SimHashFingerprinterBuilder {
    seed: u64,
    weighting: Weighting,
    hasher: HashFamily,
}

impl Default for SimHashFingerprinterBuilder {
    fn default() -> Self {
        Self {
            seed: DEFAULT_SEED,
            weighting: Weighting::Tf,
            // 0.2.0: default flipped from MurmurHash3 to Xxh3_64 for
            // throughput. Pass HashFamily::MurmurHash3_x64_128 explicitly
            // to keep datasketch byte parity.
            hasher: HashFamily::Xxh3_64,
        }
    }
}

impl SimHashFingerprinterBuilder {
    /// Override the seed.
    #[must_use]
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Override the weighting strategy.
    #[must_use]
    pub fn weighting(mut self, w: Weighting) -> Self {
        self.weighting = w;
        self
    }

    /// Override the hash family.
    #[must_use]
    pub fn hasher(mut self, hasher: HashFamily) -> Self {
        self.hasher = hasher;
        self
    }

    /// Finish the builder.
    #[must_use]
    pub fn build<T: Tokenizer>(
        self,
        canonicalizer: Canonicalizer,
        tokenizer: T,
    ) -> SimHashFingerprinter<T> {
        SimHashFingerprinter {
            canonicalizer,
            tokenizer,
            seed: self.seed,
            weighting: self.weighting,
            hasher: self.hasher,
        }
    }
}

/// Offline SimHash fingerprinter.
#[derive(Clone, Debug)]
pub struct SimHashFingerprinter<T: Tokenizer> {
    canonicalizer: Canonicalizer,
    tokenizer: T,
    seed: u64,
    weighting: Weighting,
    hasher: HashFamily,
}

impl<T: Tokenizer> SimHashFingerprinter<T> {
    /// Construct with default seed, hasher, and TF weighting.
    ///
    /// # Arguments
    ///
    /// * `canonicalizer` — Unicode preprocessing pipeline.
    /// * `tokenizer` — token producer. For SimHash, [`crate::WordTokenizer`]
    ///   is the typical choice (no shingle adaptor) — Charikar projection
    ///   needs distinct tokens, not k-grams.
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::{Canonicalizer, SimHashFingerprinter, WordTokenizer};
    ///
    /// let fp = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer);
    /// assert!(matches!(fp.weighting(), txtfp::Weighting::Tf));
    /// ```
    pub fn new(canonicalizer: Canonicalizer, tokenizer: T) -> Self {
        Self {
            canonicalizer,
            tokenizer,
            seed: DEFAULT_SEED,
            weighting: Weighting::Tf,
            // 0.2.0: see SimHashFingerprinterBuilder::default note.
            hasher: HashFamily::Xxh3_64,
        }
    }

    /// Override the seed.
    #[must_use]
    pub fn with_seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Override the hash family.
    #[must_use]
    pub fn with_hasher(mut self, hasher: HashFamily) -> Self {
        self.hasher = hasher;
        self
    }

    /// Override the weighting strategy.
    #[must_use]
    pub fn with_weighting(mut self, w: Weighting) -> Self {
        self.weighting = w;
        self
    }

    /// Borrow the canonicalizer.
    pub fn canonicalizer(&self) -> &Canonicalizer {
        &self.canonicalizer
    }

    /// Borrow the tokenizer.
    pub fn tokenizer(&self) -> &T {
        &self.tokenizer
    }

    /// Borrow the weighting.
    pub fn weighting(&self) -> &Weighting {
        &self.weighting
    }

    /// Get the hash family.
    pub fn hasher(&self) -> HashFamily {
        self.hasher
    }

    /// Sketch a canonicalized string into a [`SimHash64`].
    ///
    /// `Tf` is the hot path: each token contributes `±1` per
    /// occurrence, summing to `±tf` per distinct token without ever
    /// materializing the dedup map. `+1+1+1` for a triplicate token is
    /// identical to applying `+tf=3` once after dedup.
    ///
    /// `Uniform` and `IdfWeighted` still need a dedup pass — Uniform
    /// because the weight is `1 per distinct token` (not per
    /// occurrence), and IdfWeighted because `tf × idf` is non-linear
    /// in occurrence count.
    pub(super) fn sketch_canonical(&self, canonical: &str) -> Result<SimHash64> {
        let mut acc: [i64; 64] = [0; 64];
        let mut any = false;

        match &self.weighting {
            Weighting::Tf => {
                // Streaming +1 per occurrence; no map, no key allocs.
                let hasher = self.hasher;
                let seed = self.seed;
                self.tokenizer.for_each_token(canonical, &mut |tok| {
                    any = true;
                    let (lo, _hi) = hash128(hasher, tok.as_bytes(), seed);
                    accumulate_bits(&mut acc, lo, 1);
                });
            }
            Weighting::Uniform | Weighting::IdfWeighted(_) => {
                // Dedupe; then apply per-distinct-token weight. The
                // hashbrown HashMap path is the std-feature fast path
                // (~2× faster than BTreeMap on 1k-token docs).
                #[cfg(feature = "std")]
                let mut counts: std::collections::HashMap<String, u32> =
                    std::collections::HashMap::new();
                #[cfg(not(feature = "std"))]
                let mut counts: alloc::collections::BTreeMap<String, u32> =
                    alloc::collections::BTreeMap::new();

                self.tokenizer.for_each_token(canonical, &mut |tok| {
                    any = true;
                    if let Some(c) = counts.get_mut(tok) {
                        *c += 1;
                    } else {
                        counts.insert(tok.into(), 1);
                    }
                });
                if !any {
                    return Err(Error::InvalidInput("empty document".into()));
                }

                for (tok, tf) in &counts {
                    let weight = match &self.weighting {
                        Weighting::Uniform => 1.0_f64,
                        Weighting::IdfWeighted(table) => (*tf as f64) * table.get(tok) as f64,
                        Weighting::Tf => unreachable!(),
                    };
                    let weight = if weight.is_finite() { weight } else { 1.0 };
                    let w_int = weight.clamp(-1e15, 1e15) as i64;
                    let (lo, _hi) = hash128(self.hasher, tok.as_bytes(), self.seed);
                    accumulate_bits(&mut acc, lo, w_int);
                }
            }
        }

        if !any {
            return Err(Error::InvalidInput("empty document".into()));
        }

        let mut bits: u64 = 0;
        for (b, &slot) in acc.iter().enumerate() {
            if slot > 0 {
                bits |= 1u64 << b;
            }
        }
        Ok(SimHash64(bits))
    }
}

/// Add `±w` to each of the 64 bit-slots of `acc` according to the bits
/// of `lo`: bit `b` set ⇒ `acc[b] += w`, bit `b` clear ⇒ `acc[b] -= w`.
///
/// Saturating arithmetic so adversarial inputs cannot wrap the
/// accumulator; LLVM auto-vectorizes the inner loop on x86_64 with
/// SSE2 because the bit-spread is a sign-bit broadcast.
#[inline]
fn accumulate_bits(acc: &mut [i64; 64], lo: u64, w: i64) {
    for b in 0..64 {
        if (lo >> b) & 1 == 1 {
            acc[b] = acc[b].saturating_add(w);
        } else {
            acc[b] = acc[b].saturating_sub(w);
        }
    }
}

impl<T: Tokenizer> Fingerprinter for SimHashFingerprinter<T> {
    type Output = SimHash64;

    fn fingerprint(&self, input: &str) -> Result<Self::Output> {
        if input.is_empty() {
            return Err(Error::InvalidInput("empty document".into()));
        }
        let canonical = self.canonicalizer.canonicalize(input);
        self.sketch_canonical(&canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::Canonicalizer;
    use crate::classical::simhash::distance::hamming;
    use crate::tokenize::WordTokenizer;

    fn fp() -> SimHashFingerprinter<WordTokenizer> {
        SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer)
    }

    #[test]
    fn empty_input_errors() {
        assert!(matches!(fp().fingerprint(""), Err(Error::InvalidInput(_))));
    }

    #[test]
    fn deterministic() {
        let f = fp();
        let a = f
            .fingerprint("the quick brown fox jumps over the lazy dog")
            .unwrap();
        let b = f
            .fingerprint("the quick brown fox jumps over the lazy dog")
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn similar_docs_have_small_hamming() {
        let f = fp();
        let a = f
            .fingerprint("the quick brown fox jumps over the lazy dog")
            .unwrap();
        let b = f
            .fingerprint("the quick brown fox leaps over the lazy dog")
            .unwrap();
        // Single-token replacement out of 9 → much fewer than 32 bits flipped.
        let h = hamming(a, b);
        assert!(h < 16, "expected hamming < 16, got {h}");
    }

    #[test]
    fn different_docs_have_large_hamming() {
        let f = fp();
        let a = f
            .fingerprint("the quick brown fox jumps over the lazy dog")
            .unwrap();
        let b = f
            .fingerprint("astronomers map cosmic background radiation")
            .unwrap();
        let h = hamming(a, b);
        // Disjoint vocabulary should land near 32 bits flipped (random).
        assert!(h > 16, "expected hamming > 16, got {h}");
    }

    #[test]
    fn uniform_vs_tf_can_differ() {
        let canon = Canonicalizer::default();
        let f1 = SimHashFingerprinter::new(canon.clone(), WordTokenizer)
            .with_weighting(Weighting::Uniform);
        let f2 = SimHashFingerprinter::new(canon, WordTokenizer).with_weighting(Weighting::Tf);
        let a = f1.fingerprint("the the the the cat").unwrap();
        let b = f2.fingerprint("the the the the cat").unwrap();
        // They might happen to agree on individual bits, but TF amplifies
        // 'the' relative to 'cat' so the two strategies are not identical
        // when one term dominates the document.
        // We assert that *some* test input causes a difference; this one
        // is repetitive enough that they should differ.
        assert_ne!(a, b);
    }

    #[test]
    fn idf_table_lookup() {
        let table = IdfTable::from_pairs([("the", 0.1f32), ("cat", 4.0f32)]);
        assert!((table.get("the") - 0.1).abs() < 1e-6);
        assert!((table.get("cat") - 4.0).abs() < 1e-6);
        assert!((table.get("absent") - 1.0).abs() < 1e-6);
        assert_eq!(table.len(), 2);
        assert!(!table.is_empty());
    }

    #[test]
    fn idf_weighting_runs_end_to_end() {
        let table = IdfTable::from_pairs([("the", 0.1f32), ("dog", 4.0f32)]);
        let f = fp().with_weighting(Weighting::IdfWeighted(table));
        let s = f.fingerprint("the dog the dog the dog").unwrap();
        assert_ne!(s, SimHash64::new(0));
    }

    #[test]
    fn schema_round_trip() {
        let f = fp();
        let s = f.fingerprint("hello world").unwrap();
        let bytes = s.as_bytes();
        let s2: SimHash64 = *bytemuck::from_bytes(bytes);
        assert_eq!(s, s2);
    }

    #[test]
    fn xxh3_hasher_works() {
        let f = fp().with_hasher(HashFamily::Xxh3_64);
        let s = f.fingerprint("the quick brown fox jumps").unwrap();
        // Should not be all-zero (sketching ran).
        assert_ne!(s, SimHash64::new(0));
    }

    #[test]
    fn builder_default_matches_constructor() {
        let canon = Canonicalizer::default();
        let a = SimHashFingerprinterBuilder::default().build(canon.clone(), WordTokenizer);
        let b = SimHashFingerprinter::new(canon, WordTokenizer);
        let s_a = a.fingerprint("hello world").unwrap();
        let s_b = b.fingerprint("hello world").unwrap();
        assert_eq!(s_a, s_b);
    }
}
