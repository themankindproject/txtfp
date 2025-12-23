//! Cloud-hosted embedding providers.
//!
//! Each provider lives behind its own feature flag so the dependencies
//! it pulls in (`reqwest`, `tokio`, `serde_json`) only enter the build
//! when actually needed.

#[cfg(feature = "openai")]
#[cfg_attr(docsrs, doc(cfg(feature = "openai")))]
pub mod openai;

#[cfg(feature = "voyage")]
#[cfg_attr(docsrs, doc(cfg(feature = "voyage")))]
pub mod voyage;

#[cfg(feature = "cohere")]
#[cfg_attr(docsrs, doc(cfg(feature = "cohere")))]
pub mod cohere;

#[cfg(feature = "openai")]
pub use openai::OpenAiProvider;

#[cfg(feature = "voyage")]
pub use voyage::VoyageProvider;

#[cfg(feature = "cohere")]
pub use cohere::CohereProvider;
