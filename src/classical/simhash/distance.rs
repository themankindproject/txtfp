//! Hamming distance and cosine estimate for [`SimHash64`].

use super::sig::SimHash64;

/// Hamming distance between two SimHashes.
///
/// Lowers to hardware POPCNT on x86_64 and `cnt` on AArch64 — effectively
/// free.
///
/// # Returns
///
/// Number of bits that differ. Range `0..=64`. Symmetric:
/// `hamming(a, b) == hamming(b, a)`.
///
/// # Example
///
/// ```
/// use txtfp::{SimHash64, hamming};
///
/// assert_eq!(hamming(SimHash64::new(0), SimHash64::new(0)), 0);
/// assert_eq!(hamming(SimHash64::new(0), SimHash64::new(u64::MAX)), 64);
/// assert_eq!(hamming(SimHash64::new(0), SimHash64::new(1)), 1);
/// ```
#[inline]
#[must_use]
pub fn hamming(a: SimHash64, b: SimHash64) -> u32 {
    (a.0 ^ b.0).count_ones()
}

/// Charikar 2002 cosine estimate from Hamming distance.
///
/// Maps a SimHash Hamming distance to its random-projection cosine
/// equivalent: `cos((distance / 64) * π)`. The estimator assumes the
/// underlying SimHash was computed with enough features (a few hundred
/// or more, typical for real documents) for the random projection
/// argument to apply.
///
/// # Returns
///
/// A value in `[-1.0, 1.0]`:
///
/// | Hamming | Cosine |
/// | ------- | ------ |
/// | 0       | 1.0    |
/// | 16      | ≈ 0.71 |
/// | 32      | 0.0    |
/// | 48      | ≈ −0.71|
/// | 64      | −1.0   |
///
/// # Example
///
/// ```
/// use txtfp::{SimHash64, cosine_estimate};
///
/// let identical = cosine_estimate(SimHash64::new(0), SimHash64::new(0));
/// assert!((identical - 1.0).abs() < 1e-6);
/// ```
#[inline]
#[must_use]
pub fn cosine_estimate(a: SimHash64, b: SimHash64) -> f32 {
    let d = hamming(a, b) as f32;
    let frac = d / 64.0;
    (frac * core::f32::consts::PI).cos()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_distance_is_zero() {
        let s = SimHash64::new(0xAA55_AA55_AA55_AA55);
        assert_eq!(hamming(s, s), 0);
    }

    #[test]
    fn full_inversion_is_64() {
        let a = SimHash64::new(0x0000_0000_0000_0000);
        let b = SimHash64::new(0xFFFF_FFFF_FFFF_FFFF);
        assert_eq!(hamming(a, b), 64);
    }

    #[test]
    fn one_bit_flip_is_one() {
        let a = SimHash64::new(0);
        let b = SimHash64::new(1);
        assert_eq!(hamming(a, b), 1);
    }

    #[test]
    fn cosine_at_distance_zero_is_one() {
        let s = SimHash64::new(123);
        assert!((cosine_estimate(s, s) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_at_distance_64_is_minus_one() {
        let a = SimHash64::new(0);
        let b = SimHash64::new(u64::MAX);
        assert!((cosine_estimate(a, b) + 1.0).abs() < 1e-6);
    }

    #[test]
    fn cosine_at_distance_32_is_zero() {
        let a = SimHash64::new(0);
        let b = SimHash64::new(0xFFFF_FFFF);
        // 32 bits set → distance 32.
        assert_eq!(hamming(a, b), 32);
        assert!(cosine_estimate(a, b).abs() < 1e-6);
    }

    #[test]
    fn symmetric() {
        let a = SimHash64::new(0xAA);
        let b = SimHash64::new(0x55);
        assert_eq!(hamming(a, b), hamming(b, a));
        assert!((cosine_estimate(a, b) - cosine_estimate(b, a)).abs() < 1e-6);
    }
}
