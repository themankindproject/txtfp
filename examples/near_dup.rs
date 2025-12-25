//! Near-duplicate detection via SimHash.
//!
//! Run with: `cargo run --example near_dup --release`.

use txtfp::{Canonicalizer, Fingerprinter, SimHashFingerprinter, WordTokenizer, hamming};

fn main() {
    let canon = Canonicalizer::default();
    let fp = SimHashFingerprinter::new(canon, WordTokenizer);

    let a = fp
        .fingerprint("the quick brown fox jumps over the lazy dog")
        .unwrap();
    let b = fp
        .fingerprint("the quick brown fox leaps over the lazy dog")
        .unwrap();
    let c = fp
        .fingerprint("astronomers detect cosmic background radiation")
        .unwrap();

    println!("hamming(a,b) = {}", hamming(a, b));
    println!("hamming(a,c) = {}", hamming(a, c));
}
