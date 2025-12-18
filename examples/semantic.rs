//! Semantic similarity via the local ONNX `LocalProvider`.
//!
//! Run with: `cargo run --example semantic --features semantic --release`.
//!
//! Downloads `BAAI/bge-small-en-v1.5` from the Hugging Face Hub on first
//! invocation; subsequent runs use the local cache.

#[cfg(feature = "semantic")]
fn main() -> Result<(), Box<dyn core::error::Error>> {
    use txtfp::semantic::{EmbeddingProvider, LocalProvider, semantic_similarity};

    let provider = LocalProvider::from_pretrained("BAAI/bge-small-en-v1.5")?;
    let a = provider.embed("the cat sat on the mat")?;
    let b = provider.embed("a feline rests on a rug")?;
    let c = provider.embed("compilers translate source code")?;

    println!("similar:     {}", semantic_similarity(&a, &b)?);
    println!("dissimilar:  {}", semantic_similarity(&a, &c)?);
    Ok(())
}

#[cfg(not(feature = "semantic"))]
fn main() {
    eprintln!("Re-run with `--features semantic` to see the demo.");
}
