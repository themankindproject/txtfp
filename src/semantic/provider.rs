//! [`EmbeddingProvider`] trait + cosine similarity helper.

use crate::error::{Error, Result};

use super::embedding::{Embedding, check_compatible};

/// Trait implemented by any source of [`Embedding`] vectors.
///
/// # Associated `Input`
///
/// Most text providers use `type Input = str`. Providers that batch
/// across multiple inputs may use `type Input = [&str]` or
/// `type Input = ()` and expose batching as inherent methods on the
/// concrete type — implementations are free to expose richer ergonomics
/// outside the trait.
///
/// The trait itself uses `?Sized` so `&str` and `&[T]` both fit.
///
/// # Thread safety
///
/// Implementations should be `Send + Sync` so a single provider can be
/// shared across worker threads. Cloud-provider implementations
/// typically wrap an HTTP client which is itself `Send + Sync`; the
/// local ONNX provider serializes inference behind a mutex.
pub trait EmbeddingProvider: Send + Sync {
    /// The kind of input this provider consumes.
    type Input: ?Sized;

    /// Compute an embedding for `input`.
    ///
    /// # Errors
    ///
    /// Implementations return:
    /// - [`crate::Error::InvalidInput`] for malformed input,
    /// - [`crate::Error::Tokenizer`] / [`crate::Error::Onnx`] for local
    ///   provider failures,
    /// - [`crate::Error::Http`] for cloud provider transport failures
    ///   (after exhausting retries),
    /// - [`crate::Error::EmptyEmbedding`] for providers that returned
    ///   no data.
    fn embed(&self, input: &Self::Input) -> Result<Embedding>;

    /// The model identifier this provider produces.
    ///
    /// Used as the `model_id` field on the returned [`Embedding`] so
    /// downstream comparisons via [`semantic_similarity`] can detect
    /// model drift.
    fn model_id(&self) -> &str;

    /// The output dimensionality.
    ///
    /// Must match [`Embedding::dim`] on every successful return. Used
    /// by integrators to size database columns and refuse incompatible
    /// joins.
    fn dimension(&self) -> usize;
}

/// Cosine similarity between two embeddings.
///
/// Computes `dot(a, b) / (‖a‖ · ‖b‖)`. The function does **not**
/// require pre-normalized inputs — it computes the norms inline.
///
/// # Errors
///
/// Returns:
/// - [`Error::ModelMismatch`] when both embeddings carry `model_id`s
///   that differ.
/// - [`Error::DimensionMismatch`] when `a.dim() != b.dim()`.
/// - [`Error::InvalidInput`] when either side has zero L2 norm.
///
/// # Returns
///
/// `f32` in `[-1.0, 1.0]`:
/// - `1.0` — identical direction
/// - `0.0` — orthogonal
/// - `-1.0` — opposite direction
///
/// # Example
///
/// ```
/// # #[cfg(feature = "semantic")]
/// # fn demo() -> Result<(), txtfp::Error> {
/// use txtfp::{Embedding, semantic_similarity};
///
/// let a = Embedding::new(vec![1.0, 0.0, 0.0])?;
/// let b = Embedding::new(vec![1.0, 0.0, 0.0])?;
/// assert!((semantic_similarity(&a, &b)? - 1.0).abs() < 1e-6);
/// # Ok(()) }
/// ```
pub fn semantic_similarity(a: &Embedding, b: &Embedding) -> Result<f32> {
    check_compatible(a, b)?;

    let mut dot = 0.0_f32;
    let mut norm_a_sq = 0.0_f32;
    let mut norm_b_sq = 0.0_f32;
    for i in 0..a.vector.len() {
        let av = a.vector[i];
        let bv = b.vector[i];
        dot += av * bv;
        norm_a_sq += av * av;
        norm_b_sq += bv * bv;
    }

    let na = norm_a_sq.sqrt();
    let nb = norm_b_sq.sqrt();
    if na == 0.0 || nb == 0.0 {
        return Err(Error::InvalidInput(
            "cannot compute cosine for a zero-norm embedding".into(),
        ));
    }
    Ok(dot / (na * nb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    fn emb(v: alloc::vec::Vec<f32>) -> Embedding {
        Embedding::new(v).unwrap()
    }

    #[test]
    fn identical_vectors_score_one() {
        let a = emb(alloc::vec![1.0, 0.0, 0.0]);
        let b = emb(alloc::vec![1.0, 0.0, 0.0]);
        assert!((semantic_similarity(&a, &b).unwrap() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn orthogonal_vectors_score_zero() {
        let a = emb(alloc::vec![1.0, 0.0]);
        let b = emb(alloc::vec![0.0, 1.0]);
        assert!(semantic_similarity(&a, &b).unwrap().abs() < 1e-6);
    }

    #[test]
    fn opposite_vectors_score_minus_one() {
        let a = emb(alloc::vec![1.0, 0.0]);
        let b = emb(alloc::vec![-1.0, 0.0]);
        assert!((semantic_similarity(&a, &b).unwrap() + 1.0).abs() < 1e-6);
    }

    #[test]
    fn rejects_dim_mismatch() {
        let a = emb(alloc::vec![1.0, 0.0, 0.0]);
        let b = emb(alloc::vec![1.0, 0.0]);
        assert!(matches!(
            semantic_similarity(&a, &b),
            Err(Error::DimensionMismatch { .. })
        ));
    }

    #[test]
    fn rejects_model_mismatch() {
        let a = Embedding::with_model(alloc::vec![1.0; 4], Some("ma".into())).unwrap();
        let b = Embedding::with_model(alloc::vec![1.0; 4], Some("mb".into())).unwrap();
        assert!(matches!(
            semantic_similarity(&a, &b),
            Err(Error::ModelMismatch { .. })
        ));
    }

    #[test]
    fn allows_one_sided_model_id() {
        let a = Embedding::new(alloc::vec![1.0; 4]).unwrap();
        let b = Embedding::with_model(alloc::vec![1.0; 4], Some("mb".to_string())).unwrap();
        let s = semantic_similarity(&a, &b).unwrap();
        assert!((s - 1.0).abs() < 1e-6);
    }

    #[test]
    fn large_dim_round_trip() {
        let dim = 768;
        let v: alloc::vec::Vec<f32> = (0..dim).map(|i| (i as f32 + 1.0) / dim as f32).collect();
        let e = Embedding::new(v).unwrap();
        let s = semantic_similarity(&e, &e).unwrap();
        assert!((s - 1.0).abs() < 1e-5);
    }
}
