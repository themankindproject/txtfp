//! Pooling strategies for converting transformer hidden states into a
//! single embedding vector.

use alloc::vec::Vec;

/// Pooling strategy used by [`super::LocalProvider`] when reducing
/// `[seq_len, hidden]` outputs to a single `[hidden]` vector.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Pooling {
    /// Take the `[CLS]` token (index 0). Correct for BGE, Snowflake
    /// Arctic, mxbai. L2-normalize the result.
    Cls,
    /// Mean over the attention-mask-weighted hidden states.
    /// L2-normalize the result. Correct for E5, MiniLM, GTE, Nomic.
    Mean,
    /// Mean pooling but **without** L2 normalization. Used for some
    /// downstream tasks (clustering, KMeans) where a non-unit norm
    /// preserves magnitude information.
    MeanNoNorm,
    /// Element-wise maximum over the attention-masked hidden states.
    /// Less common; offered for completeness.
    Max,
}

impl Pooling {
    /// True if this strategy L2-normalizes the output.
    #[inline]
    #[must_use]
    pub fn normalizes(self) -> bool {
        matches!(self, Pooling::Cls | Pooling::Mean | Pooling::Max)
    }

    /// Apply the pooling strategy.
    ///
    /// `hidden` is laid out as `[seq_len, hidden_dim]` row-major: the
    /// hidden vector for token `i` lives at
    /// `hidden[i * hidden_dim..(i + 1) * hidden_dim]`.
    ///
    /// `attention_mask`, if provided, must have length `seq_len`. A
    /// value of `1` includes the token in `Mean` / `Max`; `0` excludes
    /// it. `Cls` ignores the mask entirely.
    ///
    /// Returns a `Vec<f32>` of length `hidden_dim`.
    pub fn apply(self, hidden: &[f32], hidden_dim: usize, attention_mask: Option<&[i64]>) -> Vec<f32> {
        if hidden_dim == 0 || hidden.is_empty() {
            return Vec::new();
        }
        debug_assert_eq!(
            hidden.len() % hidden_dim,
            0,
            "hidden length must be a multiple of hidden_dim"
        );
        let seq_len = hidden.len() / hidden_dim;

        let pooled: Vec<f32> = match self {
            Pooling::Cls => hidden[..hidden_dim].to_vec(),
            Pooling::Mean | Pooling::MeanNoNorm => {
                let mut sum = alloc::vec![0.0_f32; hidden_dim];
                let mut counted: f32 = 0.0;
                for tok in 0..seq_len {
                    if !mask_says_keep(attention_mask, tok) {
                        continue;
                    }
                    let off = tok * hidden_dim;
                    for d in 0..hidden_dim {
                        sum[d] += hidden[off + d];
                    }
                    counted += 1.0;
                }
                if counted == 0.0 {
                    sum
                } else {
                    let inv = 1.0 / counted;
                    for v in &mut sum {
                        *v *= inv;
                    }
                    sum
                }
            }
            Pooling::Max => {
                let mut best = alloc::vec![f32::MIN; hidden_dim];
                let mut any = false;
                for tok in 0..seq_len {
                    if !mask_says_keep(attention_mask, tok) {
                        continue;
                    }
                    any = true;
                    let off = tok * hidden_dim;
                    for d in 0..hidden_dim {
                        let v = hidden[off + d];
                        if v > best[d] {
                            best[d] = v;
                        }
                    }
                }
                if any { best } else { alloc::vec![0.0; hidden_dim] }
            }
        };

        if self.normalizes() {
            l2_normalize(pooled)
        } else {
            pooled
        }
    }
}

#[inline]
fn mask_says_keep(mask: Option<&[i64]>, idx: usize) -> bool {
    match mask {
        None => true,
        Some(m) => m.get(idx).copied().unwrap_or(0) != 0,
    }
}

fn l2_normalize(mut v: Vec<f32>) -> Vec<f32> {
    let n_sq: f32 = v.iter().map(|x| x * x).sum();
    let n = n_sq.sqrt();
    if n > 0.0 && n.is_finite() {
        for x in &mut v {
            *x /= n;
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cls_takes_first_token_and_normalizes() {
        // 2 tokens, dim 3.
        let hidden = alloc::vec![3.0, 0.0, 4.0, 99.0, 99.0, 99.0];
        let out = Pooling::Cls.apply(&hidden, 3, None);
        // Original magnitude 5 → normalize to (0.6, 0.0, 0.8).
        assert!((out[0] - 0.6).abs() < 1e-6);
        assert!(out[1].abs() < 1e-6);
        assert!((out[2] - 0.8).abs() < 1e-6);
    }

    #[test]
    fn mean_averages_then_normalizes() {
        // 2 tokens, dim 2.
        let hidden = alloc::vec![1.0, 0.0, 0.0, 1.0];
        let out = Pooling::Mean.apply(&hidden, 2, None);
        // Mean = (0.5, 0.5); normalized = (0.7071, 0.7071).
        assert!((out[0] - 0.70710677).abs() < 1e-5);
        assert!((out[1] - 0.70710677).abs() < 1e-5);
    }

    #[test]
    fn mean_no_norm_keeps_magnitude() {
        let hidden = alloc::vec![2.0, 0.0, 0.0, 2.0];
        let out = Pooling::MeanNoNorm.apply(&hidden, 2, None);
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!((out[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn mean_respects_attention_mask() {
        let hidden = alloc::vec![10.0, 0.0, 0.0, 10.0];
        let mask = alloc::vec![1_i64, 0_i64];
        // Only first token included; mean = (10, 0); normalized = (1, 0).
        let out = Pooling::Mean.apply(&hidden, 2, Some(&mask));
        assert!((out[0] - 1.0).abs() < 1e-6);
        assert!(out[1].abs() < 1e-6);
    }

    #[test]
    fn max_picks_elementwise() {
        let hidden = alloc::vec![1.0, 5.0, 4.0, 2.0];
        let out = Pooling::Max.apply(&hidden, 2, None);
        // Max per dim = (4, 5); normalized.
        let raw = alloc::vec![4.0_f32, 5.0_f32];
        let n = (16.0_f32 + 25.0).sqrt();
        assert!((out[0] - raw[0] / n).abs() < 1e-5);
        assert!((out[1] - raw[1] / n).abs() < 1e-5);
    }

    #[test]
    fn empty_hidden_yields_empty() {
        let out = Pooling::Mean.apply(&[], 8, None);
        assert!(out.is_empty());
    }

    #[test]
    fn normalizes_predicate_matches_apply() {
        for p in [Pooling::Cls, Pooling::Mean, Pooling::Max] {
            assert!(p.normalizes());
        }
        assert!(!Pooling::MeanNoNorm.normalizes());
    }
}
