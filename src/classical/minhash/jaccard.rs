//! Jaccard-similarity estimator over MinHash signatures.

use super::sig::MinHashSig;

/// Estimate the Jaccard similarity of the two sets that produced these
/// signatures.
///
/// # Arguments
///
/// * `a`, `b` — signatures over the same `H` and produced with the
///   same canonicalizer + tokenizer + hash family + seed. Comparing
///   signatures from different configurations is **not** rejected at
///   the type level (the byte layouts match) but the result is
///   meaningless; gate with [`crate::FingerprintMetadata::config_hash`].
///
/// # Returns
///
/// `(matches / H) as f32` — the fraction of slots that agree. Bounded
/// to `[0.0, 1.0]`. Mathematically symmetric (`jaccard(a, b) == jaccard(b, a)`)
/// and reflexive (`jaccard(s, s) == 1.0`).
///
/// # Performance
///
/// `O(H)` per call; trivially auto-vectorizable. For `H = 128` this is
/// ~50 ns per comparison on a 2024-class CPU — fast enough that LSH
/// query post-verification can call it for every candidate without
/// breaking sub-millisecond latency.
///
/// # Statistical accuracy
///
/// The estimator's standard deviation is `sqrt(p(1-p)/H)` where `p` is
/// the true Jaccard similarity. Concrete bounds:
///
/// | `H`  | 1σ at p=0.5 | 1σ at p=0.9 |
/// | ---- | ----------- | ----------- |
/// | 64   | ±0.063      | ±0.038      |
/// | 128  | ±0.044      | ±0.027      |
/// | 256  | ±0.031      | ±0.019      |
///
/// # Example
///
/// ```
/// # #[cfg(feature = "minhash")]
/// # fn demo() -> Result<(), txtfp::Error> {
/// use txtfp::{
///     Canonicalizer, Fingerprinter, MinHashFingerprinter,
///     ShingleTokenizer, WordTokenizer, jaccard,
/// };
///
/// let fp = MinHashFingerprinter::<_, 128>::new(
///     Canonicalizer::default(),
///     ShingleTokenizer { k: 3, inner: WordTokenizer },
/// );
///
/// let a = fp.fingerprint("the quick brown fox jumps")?;
/// let b = fp.fingerprint("the quick brown fox leaps")?;
///
/// let j = jaccard(&a, &b);
/// assert!((0.0..=1.0).contains(&j));
/// # Ok(()) }
/// ```
#[must_use]
pub fn jaccard<const H: usize>(a: &MinHashSig<H>, b: &MinHashSig<H>) -> f32 {
    if H == 0 {
        return 0.0;
    }

    // SIMD path: process 4 lanes at a time. `cmp_eq` returns each lane
    // as `u64::MAX` (true) or `0` (false). Reinterpreted as i64 those
    // are `-1` and `0`, so subtracting from a running accumulator yields
    // a per-lane match count we sum at the end.
    let chunks = H / 4;
    let tail_start = chunks * 4;

    let mut acc = wide::i64x4::ZERO;
    for i in 0..chunks {
        let off = i * 4;
        let av = wide::u64x4::new([
            a.hashes[off],
            a.hashes[off + 1],
            a.hashes[off + 2],
            a.hashes[off + 3],
        ]);
        let bv = wide::u64x4::new([
            b.hashes[off],
            b.hashes[off + 1],
            b.hashes[off + 2],
            b.hashes[off + 3],
        ]);
        let mask: wide::u64x4 = av.cmp_eq(bv);
        let mask_i: wide::i64x4 = bytemuck::cast(mask);
        acc = acc - mask_i;
    }
    let mut matches: u64 = acc.as_array_ref().iter().sum::<i64>() as u64;

    // Scalar tail for `H % 4 != 0`.
    for i in tail_start..H {
        if a.hashes[i] == b.hashes[i] {
            matches += 1;
        }
    }

    (matches as f32) / (H as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classical::minhash::sig::SCHEMA_VERSION;

    fn sig_with<const H: usize>(hashes: [u64; H]) -> MinHashSig<H> {
        MinHashSig {
            schema: SCHEMA_VERSION,
            _pad: [0; 6],
            hashes,
        }
    }

    #[test]
    fn identity_is_one() {
        let s: MinHashSig<8> = sig_with([1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(jaccard(&s, &s), 1.0);
    }

    #[test]
    fn fully_disjoint_is_zero() {
        let a: MinHashSig<8> = sig_with([1, 2, 3, 4, 5, 6, 7, 8]);
        let b: MinHashSig<8> = sig_with([9, 10, 11, 12, 13, 14, 15, 16]);
        assert_eq!(jaccard(&a, &b), 0.0);
    }

    #[test]
    fn half_overlap_is_half() {
        let a: MinHashSig<8> = sig_with([1, 2, 3, 4, 5, 6, 7, 8]);
        let b: MinHashSig<8> = sig_with([1, 2, 3, 4, 99, 99, 99, 99]);
        assert!((jaccard(&a, &b) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn bounds_are_zero_to_one() {
        let a: MinHashSig<4> = sig_with([1, 2, 3, 4]);
        let b: MinHashSig<4> = sig_with([1, 2, 5, 6]);
        let j = jaccard(&a, &b);
        assert!((0.0..=1.0).contains(&j));
    }

    #[test]
    fn symmetric() {
        let a: MinHashSig<4> = sig_with([1, 2, 3, 4]);
        let b: MinHashSig<4> = sig_with([1, 2, 99, 99]);
        assert_eq!(jaccard(&a, &b), jaccard(&b, &a));
    }
}
