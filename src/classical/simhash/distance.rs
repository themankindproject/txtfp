//! Hamming distance and cosine estimate for [`SimHash64`].

use super::sig::SimHash64;

/// Hamming distance between two SimHashes.
///
/// Returns the number of bits that differ. Range `0..=64`.
/// Constant-time via [`u64::count_ones`].
#[inline]
#[must_use]
pub fn hamming(a: SimHash64, b: SimHash64) -> u32 {
    (a.0 ^ b.0).count_ones()
}

/// Charikar 2002 cosine estimate from Hamming distance.
///
/// Returns `cos((distance / 64) * π)`. Output range `[-1.0, 1.0]`:
/// distance 0 → 1.0, distance 32 → 0.0, distance 64 → -1.0.
///
/// Note this is the *random projection* cosine estimator, not a
/// model-aware cosine. It's accurate when the underlying SimHash was
/// computed with at least a few hundred features (typical for real
/// documents).
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
