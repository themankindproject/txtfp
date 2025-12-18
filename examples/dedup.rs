//! End-to-end deduplication example: MinHash + LSH.
//!
//! Run with: `cargo run --example dedup --features lsh --release`.

#[cfg(feature = "lsh")]
fn main() {
    use txtfp::{
        Canonicalizer, Fingerprinter, LshIndex, LshIndexBuilder, MinHashFingerprinter,
        ShingleTokenizer, WordTokenizer,
    };

    let docs = [
        "the quick brown fox jumps over the lazy dog",
        "the quick brown fox leaps over the lazy dog",
        "a completely unrelated sentence about astronomy",
        "the quick brown fox jumps over a lazy cat",
    ];

    let canon = Canonicalizer::default();
    let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
    let fp = MinHashFingerprinter::<_, 128>::new(canon, tok);

    let mut index: LshIndex<128> =
        LshIndexBuilder::for_threshold(0.7, 128).expect("valid threshold").build();

    for (id, text) in docs.iter().enumerate() {
        let sig = fp.fingerprint(text).expect("non-empty");
        index.insert(id as u64, sig);
    }

    let probe = fp.fingerprint(docs[0]).unwrap();
    let neighbours = index.query(&probe);
    println!("neighbours of doc 0: {neighbours:?}");
}

#[cfg(not(feature = "lsh"))]
fn main() {
    eprintln!("Re-run with `--features lsh` to see the demo.");
}
