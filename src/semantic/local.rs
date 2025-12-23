//! Local ONNX-based [`LocalProvider`].
//!
//! Loads a transformer encoder from disk (or the Hugging Face Hub) via
//! the `ort` runtime, tokenizes input with the upstream `tokenizers`
//! crate, runs inference, and pools to a fixed-size embedding.
//!
//! # Inference contract
//!
//! - The ONNX model must accept `input_ids` and `attention_mask` as
//!   `i64` tensors of shape `[1, seq_len]`. Optional `token_type_ids`
//!   are zeroed if the graph requests them.
//! - The output must include either `last_hidden_state` or
//!   `sentence_embedding`. We probe the first matching name; for
//!   `last_hidden_state` we apply [`Pooling`] to reduce
//!   `[1, seq_len, hidden]` to `[hidden]`.
//!
//! # Model id → pooling default
//!
//! When constructing via [`LocalProvider::from_pretrained`], we look up
//! the pooling default for popular embedders so callers don't have to
//! remember whether `bge-*` uses CLS pooling or `e5-*` uses mean. The
//! table is intentionally small; pass an explicit pooling via
//! [`LocalProvider::from_onnx`] or the builder for unlisted models.

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::convert::TryFrom;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ndarray::Array2;
use ort::session::{Session, SessionInputValue};
use ort::session::builder::GraphOptimizationLevel;
use ort::value::TensorRef;
use tokenizers::Tokenizer;

use crate::error::{Error, Result};

use super::embedding::Embedding;
use super::pooling::Pooling;
use super::provider::EmbeddingProvider;

/// Pooling defaults for popular embedders (model id → pooling).
///
/// Picked from each model's official inference snippet. Models not
/// listed default to [`Pooling::Mean`] when the constructor cannot
/// infer a better choice.
const POOLING_TABLE: &[(&str, Pooling)] = &[
    ("BAAI/bge-small-en-v1.5", Pooling::Cls),
    ("BAAI/bge-base-en-v1.5", Pooling::Cls),
    ("BAAI/bge-large-en-v1.5", Pooling::Cls),
    ("BAAI/bge-m3", Pooling::Cls),
    ("intfloat/multilingual-e5-small", Pooling::Mean),
    ("intfloat/multilingual-e5-base", Pooling::Mean),
    ("intfloat/multilingual-e5-large", Pooling::Mean),
    ("intfloat/e5-small-v2", Pooling::Mean),
    ("intfloat/e5-base-v2", Pooling::Mean),
    ("intfloat/e5-large-v2", Pooling::Mean),
    ("sentence-transformers/all-MiniLM-L6-v2", Pooling::Mean),
    ("sentence-transformers/all-MiniLM-L12-v2", Pooling::Mean),
    ("sentence-transformers/all-mpnet-base-v2", Pooling::Mean),
    ("nomic-ai/nomic-embed-text-v1.5", Pooling::Mean),
    ("thenlper/gte-small", Pooling::Mean),
    ("thenlper/gte-base", Pooling::Mean),
    ("thenlper/gte-large", Pooling::Mean),
    ("Snowflake/snowflake-arctic-embed-m", Pooling::Cls),
    ("mixedbread-ai/mxbai-embed-large-v1", Pooling::Cls),
];

/// Query-side prefix for models that distinguish queries from documents.
const QUERY_PREFIXES: &[(&str, &str)] = &[
    ("BAAI/bge-small-en-v1.5", "Represent this sentence for searching relevant passages: "),
    ("BAAI/bge-base-en-v1.5", "Represent this sentence for searching relevant passages: "),
    ("BAAI/bge-large-en-v1.5", "Represent this sentence for searching relevant passages: "),
    ("intfloat/multilingual-e5-small", "query: "),
    ("intfloat/multilingual-e5-base", "query: "),
    ("intfloat/multilingual-e5-large", "query: "),
    ("intfloat/e5-small-v2", "query: "),
    ("intfloat/e5-base-v2", "query: "),
    ("intfloat/e5-large-v2", "query: "),
];

/// Document-side prefix (needed for `e5-*` which uses `passage: `).
const DOC_PREFIXES: &[(&str, &str)] = &[
    ("intfloat/multilingual-e5-small", "passage: "),
    ("intfloat/multilingual-e5-base", "passage: "),
    ("intfloat/multilingual-e5-large", "passage: "),
    ("intfloat/e5-small-v2", "passage: "),
    ("intfloat/e5-base-v2", "passage: "),
    ("intfloat/e5-large-v2", "passage: "),
];

/// Hugging-Face filenames that may hold the ONNX graph, in priority order.
const ONNX_CANDIDATES: &[&str] = &[
    "onnx/model.onnx",
    "onnx/model_quantized.onnx",
    "model.onnx",
];

/// Default sequence-length cap used when the tokenizer config does not
/// pin one. Covers BGE/E5/MiniLM/etc.
const DEFAULT_MAX_SEQ_LEN: usize = 512;

#[inline]
fn pooling_for(model_id: &str) -> Pooling {
    POOLING_TABLE
        .iter()
        .find(|(k, _)| *k == model_id)
        .map(|(_, p)| *p)
        .unwrap_or(Pooling::Mean)
}

#[inline]
fn query_prefix_for(model_id: &str) -> Option<&'static str> {
    QUERY_PREFIXES
        .iter()
        .find(|(k, _)| *k == model_id)
        .map(|(_, p)| *p)
}

#[inline]
fn doc_prefix_for(model_id: &str) -> Option<&'static str> {
    DOC_PREFIXES
        .iter()
        .find(|(k, _)| *k == model_id)
        .map(|(_, p)| *p)
}

/// Builder for [`LocalProvider`].
pub struct LocalProviderBuilder {
    model_id: Option<String>,
    onnx_path: Option<PathBuf>,
    tokenizer_path: Option<PathBuf>,
    pooling: Option<Pooling>,
    query_prefix: Option<Option<String>>,
    doc_prefix: Option<Option<String>>,
    max_seq_len: usize,
    intra_threads: Option<usize>,
}

impl LocalProviderBuilder {
    fn new() -> Self {
        Self {
            model_id: None,
            onnx_path: None,
            tokenizer_path: None,
            pooling: None,
            query_prefix: None,
            doc_prefix: None,
            max_seq_len: DEFAULT_MAX_SEQ_LEN,
            intra_threads: None,
        }
    }

    /// Override the model identifier baked into produced [`Embedding`]s.
    #[must_use]
    pub fn model_id(mut self, id: impl Into<String>) -> Self {
        self.model_id = Some(id.into());
        self
    }

    /// Set the path to the ONNX graph file.
    #[must_use]
    pub fn onnx_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.onnx_path = Some(path.into());
        self
    }

    /// Set the path to a `tokenizer.json` (HF tokenizer format).
    #[must_use]
    pub fn tokenizer_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.tokenizer_path = Some(path.into());
        self
    }

    /// Override the pooling strategy.
    #[must_use]
    pub fn pooling(mut self, p: Pooling) -> Self {
        self.pooling = Some(p);
        self
    }

    /// Set a custom query-side prefix (used by [`LocalProvider::embed_query`]).
    #[must_use]
    pub fn query_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.query_prefix = Some(Some(prefix.into()));
        self
    }

    /// Set a custom document-side prefix (used by [`LocalProvider::embed_document`]).
    #[must_use]
    pub fn doc_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.doc_prefix = Some(Some(prefix.into()));
        self
    }

    /// Override the maximum sequence length. Inputs longer than this
    /// are truncated to this many tokens.
    #[must_use]
    pub fn max_seq_len(mut self, n: usize) -> Self {
        self.max_seq_len = n.max(1);
        self
    }

    /// Set the number of intra-op threads ort will use. Defaults to ort's heuristic.
    #[must_use]
    pub fn intra_threads(mut self, n: usize) -> Self {
        self.intra_threads = Some(n.max(1));
        self
    }

    /// Finalize the builder.
    pub fn build(self) -> Result<LocalProvider> {
        let model_id = self
            .model_id
            .clone()
            .unwrap_or_else(|| "local-onnx".to_string());
        let onnx_path = self.onnx_path.ok_or_else(|| {
            Error::Config("LocalProviderBuilder: onnx_path is required".into())
        })?;
        let tokenizer_path = self.tokenizer_path.ok_or_else(|| {
            Error::Config("LocalProviderBuilder: tokenizer_path is required".into())
        })?;
        let pooling = self
            .pooling
            .unwrap_or_else(|| pooling_for(&model_id));
        let query_prefix = self
            .query_prefix
            .unwrap_or_else(|| query_prefix_for(&model_id).map(str::to_string));
        let doc_prefix = self
            .doc_prefix
            .unwrap_or_else(|| doc_prefix_for(&model_id).map(str::to_string));

        let mut session_builder = Session::builder()
            .map_err(|e| Error::Onnx(e.to_string()))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(|e| Error::Onnx(e.to_string()))?;
        if let Some(n) = self.intra_threads {
            session_builder = session_builder
                .with_intra_threads(n)
                .map_err(|e| Error::Onnx(e.to_string()))?;
        }
        let session = session_builder
            .commit_from_file(&onnx_path)
            .map_err(|e| Error::Onnx(e.to_string()))?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            .map_err(|e| Error::Tokenizer(e.to_string()))?;

        let dim = infer_hidden_dim(&session)?;

        Ok(LocalProvider(Arc::new(LocalInner {
            session: Mutex::new(session),
            tokenizer,
            model_id,
            dim,
            max_seq_len: self.max_seq_len,
            pooling,
            query_prefix,
            doc_prefix,
        })))
    }
}

/// Local ONNX embedding provider.
///
/// Cheap to clone (`Arc` under the hood). All inference is serialized
/// behind an internal mutex so the same provider can be shared across
/// threads — for high-throughput scenarios, use one `LocalProvider`
/// per worker.
#[derive(Clone)]
pub struct LocalProvider(Arc<LocalInner>);

struct LocalInner {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
    model_id: String,
    dim: usize,
    max_seq_len: usize,
    pooling: Pooling,
    query_prefix: Option<String>,
    doc_prefix: Option<String>,
}

impl core::fmt::Debug for LocalProvider {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalProvider")
            .field("model_id", &self.0.model_id)
            .field("dim", &self.0.dim)
            .field("pooling", &self.0.pooling)
            .field("max_seq_len", &self.0.max_seq_len)
            .finish()
    }
}

impl LocalProvider {
    /// Start a builder.
    pub fn builder() -> LocalProviderBuilder {
        LocalProviderBuilder::new()
    }

    /// Construct from a Hugging Face Hub model id, downloading on demand
    /// via `hf-hub`.
    pub fn from_pretrained(model_id: &str) -> Result<Self> {
        let api = hf_hub::api::sync::Api::new()
            .map_err(|e| Error::Config(alloc::format!("hf_hub init: {e}")))?;
        let repo = api.model(model_id.to_string());
        let onnx_path = ONNX_CANDIDATES
            .iter()
            .find_map(|name| repo.get(name).ok())
            .ok_or_else(|| {
                Error::Config(alloc::format!(
                    "no ONNX file found in repo `{model_id}`"
                ))
            })?;
        let tokenizer_path = repo
            .get("tokenizer.json")
            .map_err(|e| Error::Config(alloc::format!("tokenizer.json: {e}")))?;

        Self::builder()
            .model_id(model_id)
            .onnx_path(onnx_path)
            .tokenizer_path(tokenizer_path)
            .build()
    }

    /// Construct from explicit ONNX + tokenizer paths.
    pub fn from_onnx(
        onnx_path: &Path,
        tokenizer_path: &Path,
        pooling: Pooling,
    ) -> Result<Self> {
        Self::builder()
            .onnx_path(onnx_path.to_path_buf())
            .tokenizer_path(tokenizer_path.to_path_buf())
            .pooling(pooling)
            .build()
    }

    /// Embed `input` as a document (uses the document prefix, if any).
    pub fn embed_document(&self, input: &str) -> Result<Embedding> {
        let prefixed = match &self.0.doc_prefix {
            Some(p) => alloc::format!("{p}{input}"),
            None => input.to_string(),
        };
        self.run(&prefixed)
    }

    /// Embed `input` as a query (uses the query prefix, if any).
    pub fn embed_query(&self, input: &str) -> Result<Embedding> {
        let prefixed = match &self.0.query_prefix {
            Some(p) => alloc::format!("{p}{input}"),
            None => input.to_string(),
        };
        self.run(&prefixed)
    }

    /// Run the model on `text` already containing any required prefix.
    fn run(&self, text: &str) -> Result<Embedding> {
        // Tokenize.
        let encoding = self
            .0
            .tokenizer
            .encode(text, true)
            .map_err(|e| Error::Tokenizer(e.to_string()))?;
        let ids = encoding.get_ids();
        let mask = encoding.get_attention_mask();

        // Truncate to max_seq_len.
        let take = ids.len().min(self.0.max_seq_len);
        let ids: Vec<i64> = ids.iter().take(take).map(|v| *v as i64).collect();
        let mask_i64: Vec<i64> = mask.iter().take(take).map(|v| *v as i64).collect();
        let token_type_ids = alloc::vec![0_i64; take];

        let seq_len = ids.len();
        if seq_len == 0 {
            return Err(Error::InvalidInput("tokenizer produced empty sequence".into()));
        }

        // Build [1, seq_len] tensors.
        let ids_arr = Array2::from_shape_vec((1, seq_len), ids)
            .map_err(|e| Error::Onnx(alloc::format!("ids shape: {e}")))?;
        let mask_arr = Array2::from_shape_vec((1, seq_len), mask_i64.clone())
            .map_err(|e| Error::Onnx(alloc::format!("mask shape: {e}")))?;
        let tt_arr = Array2::from_shape_vec((1, seq_len), token_type_ids)
            .map_err(|e| Error::Onnx(alloc::format!("token_type shape: {e}")))?;

        // Run.
        let mut session = self
            .0
            .session
            .lock()
            .map_err(|_| Error::Onnx("session mutex poisoned".into()))?;

        // Some graphs request token_type_ids; others don't. Inspect input names.
        let input_names: Vec<String> = session
            .inputs
            .iter()
            .map(|i| i.name.clone())
            .collect();

        // Build the input list, skipping `token_type_ids` when the
        // graph does not request it.
        let needs_token_type = input_names.iter().any(|n| n == "token_type_ids");
        let unexpected = input_names.iter().find(|n| {
            !matches!(n.as_str(), "input_ids" | "attention_mask" | "token_type_ids")
        });
        if let Some(name) = unexpected {
            return Err(Error::Onnx(alloc::format!(
                "unexpected ONNX input `{name}` (expected input_ids/attention_mask/token_type_ids)"
            )));
        }

        let ids_view = TensorRef::from_array_view(&ids_arr)
            .map_err(|e| Error::Onnx(alloc::format!("ids view: {e}")))?;
        let mask_view = TensorRef::from_array_view(&mask_arr)
            .map_err(|e| Error::Onnx(alloc::format!("mask view: {e}")))?;

        let mut inputs: Vec<(String, SessionInputValue<'_>)> = Vec::with_capacity(3);
        inputs.push(("input_ids".to_string(), ids_view.into()));
        inputs.push(("attention_mask".to_string(), mask_view.into()));
        if needs_token_type {
            let tt_view = TensorRef::from_array_view(&tt_arr)
                .map_err(|e| Error::Onnx(alloc::format!("token_type view: {e}")))?;
            inputs.push(("token_type_ids".to_string(), tt_view.into()));
        }

        let outputs = session
            .run(inputs)
            .map_err(|e| Error::Onnx(alloc::format!("inference: {e}")))?;

        // Pick the first output tensor whose name matches one of the
        // patterns we know how to handle. This is the typical
        // sentence-transformers / HF-Optimum convention.
        let preferred = ["sentence_embedding", "last_hidden_state", "pooler_output"];
        let mut chosen: Option<&str> = None;
        for name in &preferred {
            if outputs.contains_key(*name) {
                chosen = Some(name);
                break;
            }
        }
        // Fallback: first output.
        let chosen_name: String = match chosen {
            Some(n) => n.to_string(),
            None => outputs
                .keys()
                .next()
                .map(|k| k.to_string())
                .ok_or_else(|| Error::Onnx("model produced no outputs".into()))?,
        };

        let value = outputs
            .get(chosen_name.as_str())
            .ok_or_else(|| Error::Onnx("output disappeared".into()))?;
        let (shape, data) = value
            .try_extract_tensor::<f32>()
            .map_err(|e| Error::Onnx(alloc::format!("extract f32 tensor: {e}")))?;

        // Two cases:
        //  1) [1, hidden] — already pooled (sentence_embedding,
        //     pooler_output). L2-normalize per the configured strategy.
        //  2) [1, seq_len, hidden] — apply the configured pooling.
        let dims_vec: Vec<usize> = shape.iter().map(|d| usize::try_from(*d).unwrap_or(0)).collect();
        let pooled: Vec<f32> = match dims_vec.len() {
            2 => {
                let hidden = dims_vec[1];
                if hidden == 0 {
                    return Err(Error::Onnx("zero-hidden output".into()));
                }
                if self.0.pooling.normalizes() {
                    l2_normalize_owned(data.to_vec())
                } else {
                    data.to_vec()
                }
            }
            3 => {
                let hidden = dims_vec[2];
                self.0.pooling.apply(data, hidden, Some(&mask_i64))
            }
            other => {
                return Err(Error::Onnx(alloc::format!(
                    "unsupported output rank: {other}"
                )));
            }
        };

        // `outputs` holds a borrow on `session`; drop it explicitly so
        // the lock guard is released before we allocate the `Embedding`.
        drop(outputs);
        drop(session);
        // Keep the input arrays alive until here so the borrowed
        // `TensorRef`s remain valid through `run`.
        drop(ids_arr);
        drop(mask_arr);
        drop(tt_arr);

        Embedding::with_model(pooled, Some(self.0.model_id.clone()))
    }
}

fn l2_normalize_owned(mut v: Vec<f32>) -> Vec<f32> {
    let n_sq: f32 = v.iter().map(|x| x * x).sum();
    let n = n_sq.sqrt();
    if n > 0.0 && n.is_finite() {
        for x in &mut v {
            *x /= n;
        }
    }
    v
}

/// Inspect the loaded session to infer the embedding dimension. We
/// pick the last dimension of the first output tensor whose shape has
/// at least one fixed dim. If everything is symbolic, we fall back to
/// the empirical hidden size used by `bge-small` (384).
fn infer_hidden_dim(session: &Session) -> Result<usize> {
    for out in &session.outputs {
        if let ort::value::ValueType::Tensor { shape, .. } = &out.output_type {
            // `shape` indexes by axis; iterate to find the last
            // non-symbolic dimension.
            let dims: Vec<i64> = shape.iter().copied().collect();
            if let Some(last) = dims.last() {
                if *last > 0 {
                    let v = usize::try_from(*last)
                        .map_err(|_| Error::Onnx("hidden dim out of range".into()))?;
                    if v > 0 {
                        return Ok(v);
                    }
                }
            }
        }
    }
    Ok(384)
}

impl EmbeddingProvider for LocalProvider {
    type Input = str;

    fn embed(&self, input: &str) -> Result<Embedding> {
        // Default to "document" semantics; callers wanting query-side
        // prefixes should use [`LocalProvider::embed_query`].
        self.embed_document(input)
    }

    fn model_id(&self) -> &str {
        &self.0.model_id
    }

    fn dimension(&self) -> usize {
        self.0.dim
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pooling_table_lookups() {
        assert_eq!(pooling_for("BAAI/bge-small-en-v1.5"), Pooling::Cls);
        assert_eq!(pooling_for("intfloat/e5-small-v2"), Pooling::Mean);
        // Unknown → Mean default.
        assert_eq!(pooling_for("unknown/model-xyz"), Pooling::Mean);
    }

    #[test]
    fn query_prefix_lookups() {
        assert!(query_prefix_for("BAAI/bge-small-en-v1.5")
            .unwrap()
            .contains("Represent"));
        assert_eq!(query_prefix_for("intfloat/e5-base-v2"), Some("query: "));
        assert!(query_prefix_for("unknown/x").is_none());
    }

    #[test]
    fn doc_prefix_for_e5() {
        assert_eq!(doc_prefix_for("intfloat/e5-base-v2"), Some("passage: "));
        assert!(doc_prefix_for("BAAI/bge-small-en-v1.5").is_none());
    }

    #[test]
    fn builder_requires_paths() {
        let r = LocalProvider::builder().build();
        assert!(matches!(r, Err(Error::Config(_))));
        let r = LocalProvider::builder()
            .onnx_path("/nonexistent.onnx")
            .build();
        assert!(matches!(r, Err(Error::Config(_))));
    }

    #[test]
    fn from_onnx_with_missing_paths_errors() {
        let r = LocalProvider::from_onnx(
            Path::new("/definitely-not-a-real-path/model.onnx"),
            Path::new("/definitely-not-a-real-path/tokenizer.json"),
            Pooling::Cls,
        );
        assert!(r.is_err());
    }

    /// Live integration test — requires network access to download a
    /// small model from the Hugging Face Hub. Skipped in default CI
    /// runs; opt in with `cargo test --features semantic -- --ignored`.
    #[test]
    #[ignore = "requires HF Hub network access"]
    fn from_pretrained_bge_small() -> Result<()> {
        let p = LocalProvider::from_pretrained("BAAI/bge-small-en-v1.5")?;
        let a = p.embed_document("hello world")?;
        assert!(a.dim() > 0);
        let b = p.embed_query("hello world")?;
        assert!(b.dim() > 0);
        Ok(())
    }
}
