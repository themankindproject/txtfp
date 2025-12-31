//! Builder for [`LshIndex`].

use crate::error::{Error, Result};

use super::index::LshIndex;

/// Builder controlling the band/row partition of an [`LshIndex`].
///
/// Either set `bands` and `rows` directly (their product must equal the
/// signature width `H`), or call [`LshIndexBuilder::for_threshold`] to
/// pick an optimum partition for a given Jaccard threshold.
#[derive(Copy, Clone, Debug)]
pub struct LshIndexBuilder {
    /// Number of bands.
    pub bands: usize,
    /// Rows per band.
    pub rows: usize,
}

impl LshIndexBuilder {
    /// Construct from explicit `(bands, rows)`.
    ///
    /// # Arguments
    ///
    /// * `bands` — number of hash tables; each table covers one band.
    ///   Higher values give higher recall.
    /// * `rows` — rows per band; each band concatenates `rows` u64 hash
    ///   slots into the band key. Higher values give higher precision.
    ///
    /// `bands * rows` must equal the const generic `H` of the
    /// [`LshIndex`] you intend to build (enforced in [`build`] /
    /// [`try_build`]).
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::LshIndexBuilder;
    /// let b = LshIndexBuilder::new(16, 8);  // 16 × 8 = 128
    /// ```
    ///
    /// [`build`]: Self::build
    /// [`try_build`]: Self::try_build
    #[must_use]
    pub fn new(bands: usize, rows: usize) -> Self {
        Self { bands, rows }
    }

    /// Pick `(bands, rows)` that approximately minimises the sum of
    /// false-positive and false-negative rates around `threshold`,
    /// subject to `bands * rows == h`.
    ///
    /// The search enumerates every factor pair of `h` and picks the one
    /// with the lowest combined error integral over `[0, threshold]`
    /// (false positives) and `[threshold, 1]` (false negatives). The
    /// integration uses 200-point trapezoidal quadrature, plenty for
    /// the smooth `1 - (1 - t^r)^b` collision curve.
    ///
    /// # Arguments
    ///
    /// * `threshold` — Jaccard similarity at which the cutoff knee
    ///   should sit, in the open interval `(0.0, 1.0)`.
    /// * `h` — the [`LshIndex`] signature width (must be ≥ 1).
    ///
    /// # Errors
    ///
    /// Returns [`Error::Config`] when:
    /// - `threshold` is not in `(0.0, 1.0)`,
    /// - `h == 0`, or
    /// - no factor pair of `h` produces a non-degenerate band/row split.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "lsh")]
    /// # {
    /// use txtfp::{LshIndex, LshIndexBuilder};
    ///
    /// let b = LshIndexBuilder::for_threshold(0.7, 128).unwrap();
    /// assert_eq!(b.bands * b.rows, 128);
    /// let _idx: LshIndex<128> = b.build();
    /// # }
    /// ```
    pub fn for_threshold(threshold: f32, h: usize) -> Result<Self> {
        if !(threshold > 0.0 && threshold < 1.0) {
            return Err(Error::Config(alloc::format!(
                "threshold must be in (0.0, 1.0); got {threshold}"
            )));
        }
        if h == 0 {
            return Err(Error::Config("H cannot be zero".into()));
        }

        let mut best = None;
        let mut best_err = f32::INFINITY;
        for b in 1..=h {
            if h % b != 0 {
                continue;
            }
            let r = h / b;
            let err = fp_rate(threshold, b, r) + fn_rate(threshold, b, r);
            if err < best_err {
                best_err = err;
                best = Some((b, r));
            }
        }

        match best {
            Some((bands, rows)) => Ok(Self { bands, rows }),
            None => Err(Error::Config(alloc::format!(
                "no factor pair found for H={h}"
            ))),
        }
    }

    /// Finish the builder, producing an empty [`LshIndex`].
    ///
    /// # Type parameters
    ///
    /// * `H` — signature width, must satisfy `bands * rows == H`.
    ///
    /// # Panics
    ///
    /// Panics if `bands * rows != H` or either is zero. Use
    /// [`try_build`] to get a `Result` instead.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "lsh")]
    /// # {
    /// use txtfp::{LshIndex, LshIndexBuilder};
    /// let idx: LshIndex<128> = LshIndexBuilder::new(16, 8).build();
    /// # }
    /// ```
    ///
    /// [`try_build`]: Self::try_build
    pub fn build<const H: usize>(self) -> LshIndex<H> {
        self.try_build()
            .expect("bands * rows must equal H; use try_build for a Result")
    }

    /// Finish the builder, returning [`Error::Config`] if `bands * rows`
    /// does not equal the const generic `H` or either is zero.
    ///
    /// Prefer this over [`build`] in any path that takes user-supplied
    /// dimensions.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Config`] when:
    /// - `bands * rows != H`, or
    /// - `bands == 0` or `rows == 0`.
    ///
    /// [`build`]: Self::build
    pub fn try_build<const H: usize>(self) -> Result<LshIndex<H>> {
        LshIndex::with_bands_rows(self.bands, self.rows)
    }
}

/// Probability that two signatures with Jaccard `t` collide in **at
/// least one** band, for a `(b, r)`-banded LSH.
#[inline]
fn prob_match(t: f32, b: usize, r: usize) -> f32 {
    let inner = 1.0 - t.powi(r as i32);
    let inner = inner.clamp(0.0, 1.0);
    1.0 - inner.powi(b as i32)
}

/// False-positive rate: integral over `[0, threshold]` of the match
/// probability (i.e., the area under the band-collision curve in the
/// region the user wanted *excluded*).
#[inline]
fn fp_rate(threshold: f32, b: usize, r: usize) -> f32 {
    integrate(0.0, threshold, |t| prob_match(t, b, r))
}

/// False-negative rate: integral over `[threshold, 1]` of `1 - match`
/// (i.e., the area in the region the user wanted *included* but where
/// the LSH missed).
#[inline]
fn fn_rate(threshold: f32, b: usize, r: usize) -> f32 {
    integrate(threshold, 1.0, |t| 1.0 - prob_match(t, b, r))
}

/// Trapezoidal numerical integration of `f` over `[lo, hi]`.
fn integrate<F>(lo: f32, hi: f32, mut f: F) -> f32
where
    F: FnMut(f32) -> f32,
{
    if hi <= lo {
        return 0.0;
    }
    const N: usize = 200;
    let dx = (hi - lo) / N as f32;
    let mut sum = 0.5 * (f(lo) + f(hi));
    for i in 1..N {
        let x = lo + i as f32 * dx;
        sum += f(x);
    }
    sum * dx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_new_round_trip() {
        let b = LshIndexBuilder::new(16, 8);
        assert_eq!(b.bands, 16);
        assert_eq!(b.rows, 8);
    }

    #[test]
    fn for_threshold_finds_factor_pair() {
        let b = LshIndexBuilder::for_threshold(0.7, 128).unwrap();
        assert_eq!(b.bands * b.rows, 128);
    }

    #[test]
    fn for_threshold_rejects_extremes() {
        assert!(LshIndexBuilder::for_threshold(0.0, 128).is_err());
        assert!(LshIndexBuilder::for_threshold(1.0, 128).is_err());
        assert!(LshIndexBuilder::for_threshold(-0.1, 128).is_err());
        assert!(LshIndexBuilder::for_threshold(1.1, 128).is_err());
    }

    #[test]
    fn for_threshold_rejects_zero_h() {
        assert!(LshIndexBuilder::for_threshold(0.5, 0).is_err());
    }

    #[test]
    fn higher_threshold_picks_more_rows() {
        let b_low = LshIndexBuilder::for_threshold(0.3, 128).unwrap();
        let b_high = LshIndexBuilder::for_threshold(0.9, 128).unwrap();
        // A higher threshold means we want a sharper cutoff; this
        // tilts the optimum toward fewer bands and more rows per band.
        assert!(b_high.rows >= b_low.rows, "{b_high:?} vs {b_low:?}");
    }

    #[test]
    fn try_build_rejects_mismatched_h() {
        let b = LshIndexBuilder::new(16, 8);
        let r: Result<LshIndex<64>> = b.try_build();
        assert!(matches!(r, Err(Error::Config(_))));
    }

    #[test]
    fn try_build_succeeds_when_h_matches() {
        let b = LshIndexBuilder::new(16, 8);
        let r: Result<LshIndex<128>> = b.try_build();
        assert!(r.is_ok());
    }

    #[test]
    fn prob_match_is_zero_at_zero_jaccard() {
        let p = prob_match(0.0, 16, 8);
        assert!(p.abs() < 1e-6, "got {p}");
    }

    #[test]
    fn prob_match_is_one_at_one_jaccard() {
        let p = prob_match(1.0, 16, 8);
        assert!((p - 1.0).abs() < 1e-6, "got {p}");
    }

    #[test]
    fn prob_match_is_monotone() {
        let a = prob_match(0.3, 16, 8);
        let b = prob_match(0.5, 16, 8);
        let c = prob_match(0.8, 16, 8);
        assert!(a < b && b < c);
    }
}
