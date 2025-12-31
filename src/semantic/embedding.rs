//! [`Embedding`] — a vector of `f32` with optional model identifier.

use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{Error, Result};

/// A semantic embedding: an `n`-dimensional `f32` vector tagged with
/// the producing model's identifier.
///
/// # Layout parity
///
/// This struct mirrors `imgfprint::Embedding` field-for-field so the
/// integrator crate `ucfp` can route `Embedding` values transparently
/// across modalities.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Embedding {
    /// The embedding values.
    pub vector: Vec<f32>,
    /// The producer model's identifier (e.g.
    /// `"BAAI/bge-small-en-v1.5"`). `None` when the embedding's source
    /// is unknown — comparing such an embedding against a
    /// model-tagged one is permitted only because we cannot prove a
    /// mismatch.
    pub model_id: Option<String>,
}

impl Embedding {
    /// Construct an embedding from a vector with no model id.
    ///
    /// # Arguments
    ///
    /// * `vector` — the embedding values.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidInput`] when:
    /// - `vector` is empty, or
    /// - any element is non-finite (NaN or ±Inf).
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "semantic")]
    /// # {
    /// use txtfp::Embedding;
    /// let e = Embedding::new(vec![0.1, 0.2, 0.3, 0.4]).unwrap();
    /// assert_eq!(e.dim(), 4);
    /// assert!(e.model_id.is_none());
    /// # }
    /// ```
    pub fn new(vector: Vec<f32>) -> Result<Self> {
        Self::with_model(vector, None)
    }

    /// Construct an embedding with an optional model id.
    ///
    /// Tagging the model id lets [`super::semantic_similarity`] refuse
    /// to compare embeddings produced by different models — a common
    /// silent-corruption source in retrieval pipelines.
    ///
    /// # Arguments
    ///
    /// * `vector` — the embedding values.
    /// * `model_id` — producer model identifier (e.g.
    ///   `"BAAI/bge-small-en-v1.5"`), or `None` if unknown.
    ///
    /// # Errors
    ///
    /// Same as [`Embedding::new`].
    pub fn with_model(vector: Vec<f32>, model_id: Option<String>) -> Result<Self> {
        if vector.is_empty() {
            return Err(Error::InvalidInput("embedding vector is empty".into()));
        }
        if vector.iter().any(|x| !x.is_finite()) {
            return Err(Error::InvalidInput(
                "embedding contains non-finite values".into(),
            ));
        }
        Ok(Self { vector, model_id })
    }

    /// Dimensionality (length of the underlying vector).
    #[inline]
    #[must_use]
    pub fn dim(&self) -> usize {
        self.vector.len()
    }

    /// L2 norm.
    ///
    /// # Returns
    ///
    /// `sqrt(Σ vᵢ²)`. Always non-negative and finite for valid
    /// `Embedding`s (the constructor rejects non-finite inputs).
    #[must_use]
    pub fn l2_norm(&self) -> f32 {
        let sum_sq: f32 = self.vector.iter().map(|x| x * x).sum();
        sum_sq.sqrt()
    }

    /// In-place L2-normalize.
    ///
    /// After this call, `self.l2_norm() ≈ 1.0` for non-zero vectors. A
    /// zero-norm vector is left at zero (not promoted to NaN). Idempotent.
    pub fn normalize(&mut self) {
        let n = self.l2_norm();
        if n > 0.0 && n.is_finite() {
            for v in &mut self.vector {
                *v /= n;
            }
        }
    }

    /// Dot product against another embedding.
    ///
    /// # Errors
    ///
    /// Returns:
    /// - [`Error::ModelMismatch`] when both operands carry `model_id`s
    ///   that differ.
    /// - [`Error::DimensionMismatch`] when `self.dim() != other.dim()`.
    ///
    /// # Example
    ///
    /// ```
    /// # #[cfg(feature = "semantic")]
    /// # fn demo() -> Result<(), txtfp::Error> {
    /// use txtfp::Embedding;
    /// let a = Embedding::new(vec![1.0, 0.0, 0.0])?;
    /// let b = Embedding::new(vec![1.0, 0.0, 0.0])?;
    /// assert!((a.dot(&b)? - 1.0).abs() < 1e-6);
    /// # Ok(()) }
    /// ```
    pub fn dot(&self, other: &Embedding) -> Result<f32> {
        check_compatible(self, other)?;
        let mut acc = 0.0_f32;
        for i in 0..self.vector.len() {
            acc += self.vector[i] * other.vector[i];
        }
        Ok(acc)
    }
}

/// Check that two embeddings can be compared. Used by
/// [`Embedding::dot`] and [`super::semantic_similarity`].
pub(super) fn check_compatible(a: &Embedding, b: &Embedding) -> Result<()> {
    if let (Some(am), Some(bm)) = (&a.model_id, &b.model_id) {
        if am != bm {
            return Err(Error::ModelMismatch {
                a: am.clone(),
                b: bm.clone(),
            });
        }
    }
    if a.vector.len() != b.vector.len() {
        return Err(Error::DimensionMismatch {
            a: a.vector.len(),
            b: b.vector.len(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_vector_rejected() {
        let r = Embedding::new(Vec::new());
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn nan_rejected() {
        let r = Embedding::new(vec![1.0, f32::NAN, 0.0]);
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn infinity_rejected() {
        let r = Embedding::new(vec![1.0, f32::INFINITY, 0.0]);
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn dim_matches_length() {
        let e = Embedding::new(vec![0.1, 0.2, 0.3, 0.4]).unwrap();
        assert_eq!(e.dim(), 4);
    }

    #[test]
    fn l2_norm_matches_pythag() {
        let e = Embedding::new(vec![3.0, 4.0]).unwrap();
        assert!((e.l2_norm() - 5.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_sets_unit_length() {
        let mut e = Embedding::new(vec![3.0, 4.0]).unwrap();
        e.normalize();
        assert!((e.l2_norm() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn normalize_zero_vector_is_safe() {
        let mut e = Embedding::with_model(vec![0.0, 0.0, 0.0], Some("zero".into())).unwrap();
        e.normalize();
        // Stays at zero (not NaN).
        assert!(e.vector.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn dot_with_matching_models() {
        let a = Embedding::with_model(vec![1.0, 0.0], Some("m1".into())).unwrap();
        let b = Embedding::with_model(vec![1.0, 0.0], Some("m1".into())).unwrap();
        assert!((a.dot(&b).unwrap() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn dot_rejects_model_mismatch() {
        let a = Embedding::with_model(vec![1.0; 4], Some("a".into())).unwrap();
        let b = Embedding::with_model(vec![1.0; 4], Some("b".into())).unwrap();
        assert!(matches!(a.dot(&b), Err(Error::ModelMismatch { .. })));
    }

    #[test]
    fn dot_rejects_dim_mismatch() {
        let a = Embedding::new(vec![1.0; 3]).unwrap();
        let b = Embedding::new(vec![1.0; 4]).unwrap();
        assert!(matches!(a.dot(&b), Err(Error::DimensionMismatch { .. })));
    }
}
