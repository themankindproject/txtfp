//! Cohere embedding provider.

use core::fmt;
use core::time::Duration;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::semantic::Embedding;
use crate::semantic::provider::EmbeddingProvider;

/// Default Cohere model. 1024-dim, English.
pub const DEFAULT_MODEL: &str = "embed-english-v3.0";

fn dimension_for(model: &str) -> usize {
    match model {
        "embed-english-v3.0" | "embed-multilingual-v3.0" => 1024,
        "embed-english-light-v3.0" | "embed-multilingual-light-v3.0" => 384,
        _ => 1024,
    }
}

/// Cohere embedding provider.
#[derive(Clone)]
pub struct CohereProvider {
    inner: Arc<Inner>,
}

struct Inner {
    api_key: String,
    base_url: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl CohereProvider {
    /// Construct from an API key. Uses [`DEFAULT_MODEL`].
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(Inner {
                api_key: api_key.into(),
                base_url: "https://api.cohere.com/v1".to_string(),
                model: DEFAULT_MODEL.to_string(),
                client,
            }),
        })
    }

    /// Override the model name.
    #[must_use]
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        let prev = Arc::try_unwrap(self.inner).unwrap_or_else(|arc| Inner {
            api_key: arc.api_key.clone(),
            base_url: arc.base_url.clone(),
            model: arc.model.clone(),
            client: arc.client.clone(),
        });
        self.inner = Arc::new(Inner {
            model: model.into(),
            ..prev
        });
        self
    }

    /// Embed multiple inputs in a single request.
    ///
    /// `input_type` must be one of Cohere's accepted values:
    /// `"search_document"`, `"search_query"`, `"classification"`, or
    /// `"clustering"`.
    pub fn embed_batch(
        &self,
        inputs: &[&str],
        input_type: &str,
    ) -> Result<alloc::vec::Vec<Embedding>> {
        if inputs.is_empty() {
            return Ok(alloc::vec::Vec::new());
        }
        let url = alloc::format!("{}/embed", self.inner.base_url);
        let body = serde_json::json!({
            "model": self.inner.model,
            "texts": inputs,
            "input_type": input_type,
        });
        let inner = self.inner.clone();
        let url_owned = url;
        let body_owned = body;
        let resp = super::retry::send_with_retry(
            &inner.client,
            || {
                inner
                    .client
                    .post(&url_owned)
                    .bearer_auth(&inner.api_key)
                    .json(&body_owned)
            },
            "Cohere",
        )?;
        let json: serde_json::Value = resp.json().map_err(|e| Error::Http(e.to_string()))?;
        let embeddings = json
            .get("embeddings")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Http("missing `embeddings`".into()))?;
        let mut out = alloc::vec::Vec::with_capacity(embeddings.len());
        for item in embeddings {
            let vec_field = item
                .as_array()
                .ok_or_else(|| Error::Http("embedding not an array".into()))?;
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

impl fmt::Debug for CohereProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CohereProvider")
            .field("model", &self.inner.model)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl EmbeddingProvider for CohereProvider {
    type Input = str;

    fn embed(&self, input: &str) -> Result<Embedding> {
        let mut batch = self.embed_batch(&[input], "search_document")?;
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
    fn debug_redacts() {
        let p = CohereProvider::new("co-secret").unwrap();
        let s = alloc::format!("{p:?}");
        assert!(!s.contains("co-secret"));
    }

    #[test]
    fn dimension_lookups() {
        assert_eq!(dimension_for("embed-english-v3.0"), 1024);
        assert_eq!(dimension_for("embed-english-light-v3.0"), 384);
    }
}
