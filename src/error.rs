//! Error type for the `txtfp` crate.
//!
//! Every fallible API in `txtfp` returns [`Result<T>`], a type alias for
//! `core::result::Result<T, Error>`. The single [`Error`] enum is
//! `#[non_exhaustive]` so that adding a new variant in a future version
//! is not a breaking change. Match exhaustively only inside the crate.

use alloc::string::String;

/// All errors surfaced by `txtfp`.
///
/// # Example
///
/// ```
/// use txtfp::Error;
///
/// let err = Error::DimensionMismatch { a: 384, b: 768 };
/// assert!(err.to_string().contains("384"));
/// ```
#[non_exhaustive]
#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// The caller-supplied input violated a precondition.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Two embeddings were compared whose `model_id` fields disagree.
    #[error("model id mismatch: a = {a}, b = {b}")]
    ModelMismatch {
        /// `model_id` of the first operand.
        a: String,
        /// `model_id` of the second operand.
        b: String,
    },

    /// Two embeddings or signatures were compared with mismatched dimensions.
    #[error("dimension mismatch: a = {a}, b = {b}")]
    DimensionMismatch {
        /// Dimension of the first operand.
        a: usize,
        /// Dimension of the second operand.
        b: usize,
    },

    /// A configuration value was rejected (out of range, mutually exclusive, …).
    #[error("config error: {0}")]
    Config(String),

    /// An I/O failure surfaced through `txtfp`.
    #[cfg(feature = "std")]
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// A tokenizer (HF tokenizers) reported a fatal error.
    #[cfg(feature = "semantic")]
    #[error("tokenizer error: {0}")]
    Tokenizer(String),

    /// ONNX Runtime reported a fatal error during model load or inference.
    #[cfg(feature = "semantic")]
    #[error("onnx error: {0}")]
    Onnx(String),

    /// A cloud provider (OpenAI, Voyage, Cohere) reported an HTTP error.
    #[cfg(any(feature = "openai", feature = "voyage", feature = "cohere"))]
    #[error("http error: {0}")]
    Http(String),

    /// A cloud provider returned an empty embedding payload.
    #[cfg(feature = "semantic")]
    #[error("provider returned no embeddings")]
    EmptyEmbedding,

    /// A serialized signature does not match the expected schema version.
    #[error("schema version mismatch: expected {expected}, got {actual}")]
    SchemaMismatch {
        /// Schema version this build of `txtfp` expects.
        expected: u16,
        /// Schema version observed in the input.
        actual: u16,
    },

    /// The caller invoked a feature-gated API that was not enabled at compile time.
    #[error("feature `{0}` not enabled at compile time")]
    FeatureDisabled(&'static str),
}

/// Shorthand for `core::result::Result<T, Error>`.
///
/// # Example
///
/// ```
/// use txtfp::{Error, Result};
///
/// fn ensure_non_empty(input: &str) -> Result<()> {
///     if input.is_empty() {
///         return Err(Error::InvalidInput("empty document".into()));
///     }
///     Ok(())
/// }
/// # ensure_non_empty("hello").unwrap();
/// ```
pub type Result<T, E = Error> = core::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn invalid_input_renders_payload() {
        let s = Error::InvalidInput("empty document".into()).to_string();
        assert!(s.contains("empty document"), "got: {s}");
    }

    #[test]
    fn model_mismatch_renders_both_ids() {
        let s = Error::ModelMismatch {
            a: "bge-small".into(),
            b: "bge-large".into(),
        }
        .to_string();
        assert!(s.contains("bge-small"));
        assert!(s.contains("bge-large"));
    }

    #[test]
    fn dimension_mismatch_renders_both_dims() {
        let s = Error::DimensionMismatch { a: 384, b: 768 }.to_string();
        assert!(s.contains("384"));
        assert!(s.contains("768"));
    }

    #[test]
    fn schema_mismatch_renders_versions() {
        let s = Error::SchemaMismatch {
            expected: 1,
            actual: 7,
        }
        .to_string();
        assert!(s.contains('1'));
        assert!(s.contains('7'));
    }

    #[test]
    fn feature_disabled_renders_name() {
        let s = Error::FeatureDisabled("semantic").to_string();
        assert!(s.contains("semantic"));
    }

    #[test]
    fn error_is_send_sync_static() {
        fn assert_traits<T: Send + Sync + 'static>() {}
        assert_traits::<Error>();
    }

    #[test]
    fn result_alias_works() {
        let f = |x: u32| -> Result<u32> { Ok(x * 2) };
        assert_eq!(f(21).unwrap(), 42);
    }
}
