//! Semantic embedding support.
//!
//! Behind the `semantic` feature, `txtfp` exposes an
//! [`EmbeddingProvider`] trait and a [`LocalProvider`] implementation
//! that loads ONNX models from the Hugging Face Hub, plus a cosine
//! [`semantic_similarity`] helper that refuses to compare embeddings
//! from different models or dimensions.
//!
//! The trait shape, the [`Embedding`] struct layout, and the
//! `model_id`-guarding helper are intentionally parity-compatible with
//! the corresponding types in `imgfprint`. Cloud-hosted providers are
//! out of scope for this crate; users wanting OpenAI / Voyage / Cohere
//! /etc. should implement [`EmbeddingProvider`] against their own HTTP
//! client of choice — the trait is small enough that vendor wrappers
//! do not pull their weight here.
//!
//! # Example
//!
//! ```no_run
//! use txtfp::semantic::{EmbeddingProvider, LocalProvider, semantic_similarity};
//!
//! let provider = LocalProvider::from_pretrained("BAAI/bge-small-en-v1.5")?;
//! let a = provider.embed("the cat sat on the mat")?;
//! let b = provider.embed("a feline rests on a rug")?;
//! let s = semantic_similarity(&a, &b)?;
//! assert!(s > 0.5);
//! # Ok::<(), txtfp::Error>(())
//! ```

mod chunk;
mod embedding;
mod local;
mod pooling;
mod provider;

pub use chunk::{ChunkMode, ChunkingStrategy, chunk_for_model};
pub use embedding::Embedding;
pub use local::{LocalProvider, LocalProviderBuilder};
pub use pooling::Pooling;
pub use provider::{EmbeddingProvider, semantic_similarity};
