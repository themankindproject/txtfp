//! Live integration smoke test against the local ONNX provider.
//!
//! Skipped by default. Enable with `TXTFP_LIVE=1` to run:
//!
//! ```bash
//! TXTFP_LIVE=1 cargo test --test live_semantic --features semantic -- --nocapture
//! ```
//!
//! The test downloads `BAAI/bge-small-en-v1.5` from the Hugging Face
//! Hub on first run and verifies the provider returns:
//! - a vector of the expected dimension,
//! - finite values,
//! - non-zero L2 norm,
//! - higher similarity for paraphrase pairs than for unrelated pairs.

#![cfg(feature = "semantic")]

use std::env;

fn live_enabled() -> bool {
    env::var("TXTFP_LIVE").map(|v| v == "1").unwrap_or(false)
}

fn skip_unless_live(test: &str) -> bool {
    if !live_enabled() {
        eprintln!("skipping {test}: set TXTFP_LIVE=1 to enable");
        return true;
    }
    false
}

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
