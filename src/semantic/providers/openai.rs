//! OpenAI embedding provider.
//!
//! Hits the [Embeddings](https://platform.openai.com/docs/api-reference/embeddings/create)
//! endpoint with a blocking HTTP client; the synchronous [`embed`]
//! method returns one [`Embedding`] per call. Use [`embed_batch`] when
//! you have many inputs — OpenAI accepts up to 2048 inputs per request
//! and per-request overhead dwarfs per-input cost.
//!
//! [`embed`]: crate::EmbeddingProvider::embed
//! [`embed_batch`]: OpenAiProvider::embed_batch

use core::fmt;
use core::time::Duration;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::semantic::Embedding;
use crate::semantic::provider::EmbeddingProvider;

/// Default OpenAI model. 1536-dim, cheapest per-token.
pub const DEFAULT_MODEL: &str = "text-embedding-3-small";

/// Per-model output dimension.
fn dimension_for(model: &str) -> usize {
    match model {
        "text-embedding-3-small" => 1536,
        "text-embedding-3-large" => 3072,
        "text-embedding-ada-002" => 1536,
        _ => 1536, // best guess
    }
}

/// OpenAI embedding provider.
#[derive(Clone)]
pub struct OpenAiProvider {
    inner: Arc<Inner>,
}

struct Inner {
    api_key: String,
    base_url: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl OpenAiProvider {
    /// Construct from an API key. Uses [`DEFAULT_MODEL`] and OpenAI's
    /// production endpoint; override either with the `with_*` methods.
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(Inner {
                api_key: api_key.into(),
                base_url: "https://api.openai.com/v1".to_string(),
                model: DEFAULT_MODEL.to_string(),
                client,
            }),
        })
    }

    /// Override the model name.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        // Arc::make_mut can't help with this since `Inner` owns
        // non-`Clone` fields; just rebuild.
        let prev = Arc::try_unwrap(self.inner).unwrap_or_else(|arc| {
            // Fallback: clone what we can.
            Inner {
                api_key: arc.api_key.clone(),
                base_url: arc.base_url.clone(),
                model: arc.model.clone(),
                client: arc.client.clone(),
            }
        });
        self.inner = Arc::new(Inner {
            model: model.into(),
            ..prev
        });
        self
    }

    /// Override the base URL (for self-hosted or proxy deployments).
    #[must_use]
    pub fn with_base_url(mut self, url: impl Into<String>) -> Self {
        let prev = Arc::try_unwrap(self.inner).unwrap_or_else(|arc| Inner {
            api_key: arc.api_key.clone(),
            base_url: arc.base_url.clone(),
            model: arc.model.clone(),
            client: arc.client.clone(),
        });
        self.inner = Arc::new(Inner {
            base_url: url.into(),
            ..prev
        });
        self
    }

    /// Embed multiple inputs in a single request.
    pub fn embed_batch(&self, inputs: &[&str]) -> Result<alloc::vec::Vec<Embedding>> {
        if inputs.is_empty() {
            return Ok(alloc::vec::Vec::new());
        }
        let url = alloc::format!("{}/embeddings", self.inner.base_url);
        let body = serde_json::json!({
            "model": self.inner.model,
            "input": inputs,
        });
        let resp = self
            .inner
            .client
            .post(url)
            .bearer_auth(&self.inner.api_key)
            .json(&body)
            .send()
            .map_err(|e| Error::Http(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(Error::Http(alloc::format!(
                "OpenAI returned status {}",
                resp.status()
            )));
        }
        let json: serde_json::Value = resp.json().map_err(|e| Error::Http(e.to_string()))?;
        let data = json
            .get("data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Http("missing `data` array in response".into()))?;
        if data.is_empty() {
            return Err(Error::EmptyEmbedding);
        }

        let mut out = alloc::vec::Vec::with_capacity(data.len());
        for item in data {
            let vec_field = item
                .get("embedding")
                .and_then(|v| v.as_array())
                .ok_or_else(|| Error::Http("missing `embedding` field".into()))?;
            let vector: alloc::vec::Vec<f32> = vec_field
                .iter()
                .map(|v| v.as_f64().unwrap_or(0.0) as f32)
                .collect();
            out.push(Embedding::with_model(
                vector,
                Some(self.inner.model.clone()),
            )?);
        }
        Ok(out)
    }
}

impl fmt::Debug for OpenAiProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OpenAiProvider")
            .field("model", &self.inner.model)
            .field("base_url", &self.inner.base_url)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl EmbeddingProvider for OpenAiProvider {
    type Input = str;

    fn embed(&self, input: &str) -> Result<Embedding> {
        let mut batch = self.embed_batch(&[input])?;
        batch.pop().ok_or(Error::EmptyEmbedding)
    }

    fn model_id(&self) -> &str {
        &self.inner.model
    }

    fn dimension(&self) -> usize {
        dimension_for(&self.inner.model)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_api_key() {
        let p = OpenAiProvider::new("sk-secret-do-not-leak").unwrap();
        let s = alloc::format!("{p:?}");
        assert!(!s.contains("sk-secret"));
        assert!(s.contains("<redacted>"));
    }

    #[test]
    fn dimension_table_lookups() {
        assert_eq!(dimension_for("text-embedding-3-small"), 1536);
        assert_eq!(dimension_for("text-embedding-3-large"), 3072);
        // Unknown defaults to 1536.
        assert_eq!(dimension_for("unknown-model"), 1536);
    }

    #[test]
    fn with_model_changes_model_id() {
        let p = OpenAiProvider::new("sk-test")
            .unwrap()
            .with_model("text-embedding-3-large");
        assert_eq!(p.model_id(), "text-embedding-3-large");
        assert_eq!(p.dimension(), 3072);
    }
}
