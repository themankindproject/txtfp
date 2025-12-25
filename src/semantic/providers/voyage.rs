//! Voyage AI embedding provider.

use core::fmt;
use core::time::Duration;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::semantic::Embedding;
use crate::semantic::provider::EmbeddingProvider;

/// Default Voyage model.
/// Default Voyage model. 512-dim, fastest.
pub const DEFAULT_MODEL: &str = "voyage-3-lite";

fn dimension_for(model: &str) -> usize {
    match model {
        "voyage-3-lite" => 512,
        "voyage-3" => 1024,
        "voyage-large-2" => 1536,
        "voyage-code-2" => 1536,
        _ => 1024,
    }
}

/// Voyage AI embedding provider.
#[derive(Clone)]
pub struct VoyageProvider {
    inner: Arc<Inner>,
}

struct Inner {
    api_key: String,
    base_url: String,
    model: String,
    client: reqwest::blocking::Client,
}

impl VoyageProvider {
    /// Construct from an API key. Uses [`DEFAULT_MODEL`].
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(Inner {
                api_key: api_key.into(),
                base_url: "https://api.voyageai.com/v1".to_string(),
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
    /// `input_type` may be `"document"`, `"query"`, or `None` to omit
    /// the field (Voyage uses `"document"` as the default).
    pub fn embed_batch(
        &self,
        inputs: &[&str],
        input_type: Option<&str>,
    ) -> Result<alloc::vec::Vec<Embedding>> {
        if inputs.is_empty() {
            return Ok(alloc::vec::Vec::new());
        }
        let url = alloc::format!("{}/embeddings", self.inner.base_url);
        let mut body = serde_json::json!({
            "model": self.inner.model,
            "input": inputs,
        });
        if let Some(t) = input_type {
            body["input_type"] = serde_json::Value::from(t);
        }

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
                "Voyage returned status {}",
                resp.status()
            )));
        }
        let json: serde_json::Value = resp.json().map_err(|e| Error::Http(e.to_string()))?;
        let data = json
            .get("data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| Error::Http("missing `data` array".into()))?;

        let mut out = alloc::vec::Vec::with_capacity(data.len());
        for item in data {
            let vec_field = item
                .get("embedding")
                .and_then(|v| v.as_array())
                .ok_or_else(|| Error::Http("missing `embedding`".into()))?;
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

impl fmt::Debug for VoyageProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoyageProvider")
            .field("model", &self.inner.model)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl EmbeddingProvider for VoyageProvider {
    type Input = str;

    fn embed(&self, input: &str) -> Result<Embedding> {
        let mut batch = self.embed_batch(&[input], Some("document"))?;
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
        let p = VoyageProvider::new("voy-secret").unwrap();
        let s = alloc::format!("{p:?}");
        assert!(!s.contains("voy-secret"));
    }

    #[test]
    fn dimension_lookups() {
        assert_eq!(dimension_for("voyage-3-lite"), 512);
        assert_eq!(dimension_for("voyage-large-2"), 1536);
    }
}
