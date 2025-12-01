//! Jaccard-similarity estimator over MinHash signatures.

use super::sig::MinHashSig;

/// Estimate the Jaccard similarity of the two sets that produced these
/// signatures.
///
/// Returns `(matches as f32) / (H as f32)` — the fraction of slots that
/// agree. Bounded to `[0.0, 1.0]`. Mathematically symmetric and
/// reflexive: `jaccard(s, s) == 1.0`.
///
/// The estimator's standard deviation is `sqrt(p(1-p)/H)` where `p` is
/// the true Jaccard similarity; for `H = 128` and `p = 0.5`, that's
/// about `±0.044`.
#[must_use]
pub fn jaccard<const H: usize>(a: &MinHashSig<H>, b: &MinHashSig<H>) -> f32 {
    if H == 0 {
        return 0.0;
    }
    let mut matches: usize = 0;
    for i in 0..H {
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
