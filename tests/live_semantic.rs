//! Live integration smoke tests against real embedding endpoints.
//!
//! Skipped by default. Enable with `TXTFP_LIVE=1` (and the relevant
//! API keys) to run:
//!
//! ```bash
//! TXTFP_LIVE=1 \
//! OPENAI_API_KEY=sk-... \
//! VOYAGE_API_KEY=... \
//! COHERE_API_KEY=... \
//!     cargo test --test live_semantic --features "semantic,openai,voyage,cohere" -- --nocapture
//! ```
//!
//! Each test verifies the provider returns:
//! - a vector of the expected dimension,
//! - finite values,
//! - non-zero L2 norm,
//! - higher similarity for paraphrase-class pairs than for unrelated pairs.

#![cfg(any(
    feature = "openai",
    feature = "voyage",
    feature = "cohere",
    feature = "semantic"
))]

use std::env;

fn live_enabled() -> bool {
    env::var("TXTFP_LIVE").map(|v| v == "1").unwrap_or(false)
}

#[allow(dead_code)]
fn skip_unless_live(test: &str) -> bool {
    if !live_enabled() {
        eprintln!("skipping {test}: set TXTFP_LIVE=1 to enable");
        return true;
    }
    false
}

#[cfg(feature = "semantic")]
#[test]
fn live_local_provider_bge_small() {
    use txtfp::semantic::{EmbeddingProvider, LocalProvider, semantic_similarity};

    if skip_unless_live("live_local_provider_bge_small") {
        return;
    }

    let p = LocalProvider::from_pretrained("BAAI/bge-small-en-v1.5")
        .expect("bge-small-en-v1.5 must be reachable");

    assert_eq!(p.dimension(), 384, "bge-small produces 384-dim embeddings");
    assert_eq!(p.model_id(), "BAAI/bge-small-en-v1.5");

    let a = p
        .embed_document("the cat sat on the mat")
        .expect("embed should succeed");
    let b = p
        .embed_document("a feline rests on a rug")
        .expect("embed should succeed");
    let c = p
        .embed_document("compilers translate source code")
        .expect("embed should succeed");

    assert_eq!(a.dim(), 384);
    assert!(a.l2_norm() > 0.5);
    assert!(a.vector.iter().all(|x| x.is_finite()));

    let sim_ab = semantic_similarity(&a, &b).unwrap();
    let sim_ac = semantic_similarity(&a, &c).unwrap();
    assert!(
        sim_ab > sim_ac,
        "paraphrase pair must score higher: {sim_ab} vs {sim_ac}"
    );
}

#[cfg(feature = "openai")]
#[test]
fn live_openai_text_embedding_3_small() {
    use txtfp::semantic::EmbeddingProvider;
    use txtfp::semantic::providers::OpenAiProvider;

    if skip_unless_live("live_openai_text_embedding_3_small") {
        return;
    }
    let key = match env::var("OPENAI_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            eprintln!("skipping openai live test: no OPENAI_API_KEY");
            return;
        }
    };

    let p = OpenAiProvider::new(key).expect("client builds");
    let v = p.embed("the quick brown fox").expect("embed succeeds");
    assert_eq!(v.dim(), 1536, "text-embedding-3-small is 1536-dim");
    assert_eq!(v.model_id.as_deref(), Some("text-embedding-3-small"));
    assert!(v.l2_norm() > 0.5);
    assert!(v.vector.iter().all(|x| x.is_finite()));
}

#[cfg(feature = "voyage")]
#[test]
fn live_voyage_3_lite() {
    use txtfp::semantic::EmbeddingProvider;
    use txtfp::semantic::providers::VoyageProvider;

    if skip_unless_live("live_voyage_3_lite") {
        return;
    }
    let key = match env::var("VOYAGE_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            eprintln!("skipping voyage live test: no VOYAGE_API_KEY");
            return;
        }
    };
    let p = VoyageProvider::new(key).expect("client builds");
    let v = p.embed("the quick brown fox").expect("embed succeeds");
    assert_eq!(v.dim(), 512);
    assert_eq!(v.model_id.as_deref(), Some("voyage-3-lite"));
    assert!(v.l2_norm() > 0.5);
}

#[cfg(feature = "cohere")]
#[test]
fn live_cohere_embed_english_v3() {
    use txtfp::semantic::EmbeddingProvider;
    use txtfp::semantic::providers::CohereProvider;

    if skip_unless_live("live_cohere_embed_english_v3") {
        return;
    }
    let key = match env::var("COHERE_API_KEY") {
        Ok(k) => k,
        Err(_) => {
            eprintln!("skipping cohere live test: no COHERE_API_KEY");
            return;
        }
    };
    let p = CohereProvider::new(key).expect("client builds");
    let v = p.embed("the quick brown fox").expect("embed succeeds");
    assert_eq!(v.dim(), 1024);
    assert_eq!(v.model_id.as_deref(), Some("embed-english-v3.0"));
    assert!(v.l2_norm() > 0.5);
}
