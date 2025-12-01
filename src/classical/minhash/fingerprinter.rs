//! Offline MinHash fingerprinter.

use alloc::string::String;

use crate::canonical::Canonicalizer;
use crate::classical::Fingerprinter;
use crate::classical::hash::{HashFamily, hash128};
use crate::error::{Error, Result};
use crate::tokenize::Tokenizer;

use super::sig::MinHashSig;

/// Default seed for [`MinHashFingerprinter`]. Hex spelling: `0xC0FFEE_5EED`.
///
/// Frozen for v0.1.x: changing the default seed would change every
/// downstream signature.
pub const DEFAULT_SEED: u64 = 0x00C0_FFEE_5EED;

/// Builder for [`MinHashFingerprinter`].
///
/// Defaults: `seed = DEFAULT_SEED`, `hasher = MurmurHash3_x64_128`.
#[derive(Clone, Debug)]
pub struct MinHashFingerprinterBuilder {
    seed: u64,
    hasher: HashFamily,
}

impl Default for MinHashFingerprinterBuilder {
    fn default() -> Self {
        Self {
            seed: DEFAULT_SEED,
            hasher: HashFamily::MurmurHash3_x64_128,
        }
    }
}

impl MinHashFingerprinterBuilder {
    /// Override the base seed. Each of the `H` hash slots derives its
    /// effective seed by adding its slot index, but only when re-seeding
    /// — the default double-hashing path uses a single base seed.
    #[must_use]
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Override the hash family. Default is
    /// [`HashFamily::MurmurHash3_x64_128`] for datasketch parity.
    #[must_use]
    pub fn hasher(mut self, hasher: HashFamily) -> Self {
        self.hasher = hasher;
        self
    }

    /// Finish the builder.
    #[must_use]
    pub fn build<T: Tokenizer, const H: usize>(
        self,
        canonicalizer: Canonicalizer,
        tokenizer: T,
    ) -> MinHashFingerprinter<T, H> {
        MinHashFingerprinter {
            canonicalizer,
            tokenizer,
            seed: self.seed,
            hasher: self.hasher,
        }
    }
}

/// Offline MinHash fingerprinter parameterized by tokenizer and slot count.
///
/// `H = 128` is the recommended default for general-purpose corpus
/// deduplication; smaller `H` (32, 64) trades estimator variance for
/// memory and compute. See Broder 1997 for the variance bound:
/// `Var ≈ p(1-p)/H`.
#[derive(Clone, Debug)]
pub struct MinHashFingerprinter<T: Tokenizer, const H: usize> {
    canonicalizer: Canonicalizer,
    tokenizer: T,
    seed: u64,
    hasher: HashFamily,
}

impl<T: Tokenizer, const H: usize> MinHashFingerprinter<T, H> {
    /// Construct a fingerprinter with the default seed and hasher.
    pub fn new(canonicalizer: Canonicalizer, tokenizer: T) -> Self {
        Self {
            canonicalizer,
            tokenizer,
            seed: DEFAULT_SEED,
            hasher: HashFamily::MurmurHash3_x64_128,
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

    /// Borrow the canonicalizer.
    pub fn canonicalizer(&self) -> &Canonicalizer {
        &self.canonicalizer
    }

    /// Borrow the tokenizer.
    pub fn tokenizer(&self) -> &T {
        &self.tokenizer
    }

    /// Get the seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// Get the hash family.
    pub fn hasher(&self) -> HashFamily {
        self.hasher
    }

    /// Sketch a canonicalized string into a [`MinHashSig<H>`].
    ///
    /// Used internally by [`Fingerprinter::fingerprint`] *and* by
    /// [`super::streaming::MinHashStreaming::finalize`].
    pub(super) fn sketch_canonical(&self, canonical: &str) -> Result<MinHashSig<H>> {
        let mut sig = MinHashSig::<H>::empty();
        let mut any = false;
        let token_iter = self.tokenizer.tokens(canonical).into_string_iter();
        for tok in token_iter {
            any = true;
            // Double-hashing: one hash per shingle, derive H slots cheaply.
            let (lo, hi) = hash128(self.hasher, tok.as_bytes(), self.seed);
            for (i, slot) in sig.hashes.iter_mut().enumerate() {
                let h = lo.wrapping_add((i as u64).wrapping_mul(hi));
                if h < *slot {
                    *slot = h;
                }
            }
        }
        if !any {
            return Err(Error::InvalidInput("empty document".into()));
        }
        Ok(sig)
    }
}

impl<T: Tokenizer, const H: usize> Fingerprinter for MinHashFingerprinter<T, H> {
    type Output = MinHashSig<H>;

    fn fingerprint(&self, input: &str) -> Result<Self::Output> {
        if input.is_empty() {
            return Err(Error::InvalidInput("empty document".into()));
        }
        let canonical: String = self.canonicalizer.canonicalize(input);
        self.sketch_canonical(&canonical)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canonical::Canonicalizer;
    use crate::classical::minhash::jaccard::jaccard;
    use crate::tokenize::{ShingleTokenizer, WordTokenizer};

    fn fp() -> MinHashFingerprinter<ShingleTokenizer<WordTokenizer>, 128> {
        MinHashFingerprinter::new(
            Canonicalizer::default(),
            ShingleTokenizer { k: 3, inner: WordTokenizer },
        )
    }

    #[test]
    fn empty_input_errors() {
        let f = fp();
        let r = f.fingerprint("");
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn whitespace_only_errors() {
        let f = fp();
        let r = f.fingerprint("    \n\n\n");
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn deterministic() {
        let f = fp();
        let a = f.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
        let b = f.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn similar_docs_have_high_jaccard() {
        let f = fp();
        let a = f.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
        let b = f.fingerprint("the quick brown fox leaps over the lazy dog").unwrap();
        let j = jaccard(&a, &b);
        // True Jaccard ≈ 0.4 (4 shared / 10 union shingles for k=3); the
        // H=128 estimator's 1σ is ±0.044, so allow a lower bound at 2σ.
        assert!(j > 0.30, "expected j > 0.30, got {j}");
    }

    #[test]
    fn different_docs_have_low_jaccard() {
        let f = fp();
        let a = f.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
        let b = f.fingerprint("a completely unrelated sentence about astronomy").unwrap();
        let j = jaccard(&a, &b);
        assert!(j < 0.3, "expected j < 0.3, got {j}");
    }

    #[test]
    fn permutation_invariance() {
        // Set theory: token order should not affect the signature, *if*
        // the tokenizer doesn't bake in order. Word-shingles do bake in
        // order (k-grams). So we test on word-only (k=1).
        let f = MinHashFingerprinter::<_, 64>::new(Canonicalizer::default(), WordTokenizer);
        let a = f.fingerprint("alpha beta gamma delta").unwrap();
        let b = f.fingerprint("delta gamma beta alpha").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn duplicate_insensitivity() {
        let f = MinHashFingerprinter::<_, 64>::new(Canonicalizer::default(), WordTokenizer);
        let a = f.fingerprint("alpha beta gamma delta").unwrap();
        let b = f.fingerprint("alpha beta gamma delta alpha beta gamma delta").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn schema_field_set() {
        let f = fp();
        let s = f.fingerprint("hello world hello world").unwrap();
        assert_eq!(s.schema, super::super::sig::SCHEMA_VERSION);
    }

    #[test]
    fn seed_change_changes_signature() {
        let f1 = fp();
        let f2 = fp().with_seed(42);
        let a = f1.fingerprint("the quick brown fox jumps").unwrap();
        let b = f2.fingerprint("the quick brown fox jumps").unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn xxh3_hasher_is_selectable() {
        let f = fp().with_hasher(HashFamily::Xxh3_64);
        let a = f.fingerprint("the quick brown fox jumps").unwrap();
        // Should not collapse to all-MAX (sketching ran).
        assert!(a.hashes.iter().any(|h| *h != u64::MAX));
    }

    #[test]
    fn builder_default_matches_constructor() {
        let canon = Canonicalizer::default();
        let tok = ShingleTokenizer { k: 3, inner: WordTokenizer };
        let a = MinHashFingerprinterBuilder::default().build::<_, 128>(canon.clone(), tok.clone());
        let b: MinHashFingerprinter<_, 128> = MinHashFingerprinter::new(canon, tok);
        let s_a = a.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
        let s_b = b.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
        assert_eq!(s_a, s_b);
    }
}
