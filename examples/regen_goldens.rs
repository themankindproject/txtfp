//! Regenerate the byte-stable golden fixtures for the test suite.
//!
//! Usage: `cargo run --example regen_goldens --features lsh --release`.
//!
//! Reads each text in `tests/data/corpora/`, runs canonicalize +
//! MinHash + SimHash with the default configurations, and writes the
//! signature bytes to `tests/data/golden/{algo}/{name}.bin`.
//!
//! **Goldens are part of the v0.1.0 stable contract** — never
//! regenerate them once a v0.1.x release is tagged. If a code change
//! moves a golden, fix the code, not the golden.

use std::fs;
use std::path::PathBuf;

use txtfp::{
    Canonicalizer, Fingerprinter, MinHashFingerprinter, ShingleTokenizer, SimHashFingerprinter,
    WordTokenizer,
};

fn main() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let corpora = manifest.join("tests/data/corpora");
    let golden = manifest.join("tests/data/golden");

    fs::create_dir_all(golden.join("minhash")).unwrap();
    fs::create_dir_all(golden.join("simhash")).unwrap();
    fs::create_dir_all(golden.join("canonical")).unwrap();

    let canon = Canonicalizer::default();
    let tok = ShingleTokenizer {
        k: 5,
        inner: WordTokenizer,
    };
    let mh = MinHashFingerprinter::<_, 128>::new(canon.clone(), tok);
    let sh = SimHashFingerprinter::new(canon.clone(), WordTokenizer);

    for entry in fs::read_dir(&corpora).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let txt = fs::read_to_string(&path).unwrap();

        // Canonical text.
        let c = canon.canonicalize(&txt);
        fs::write(golden.join("canonical").join(format!("{stem}.txt")), &c).unwrap();

        // MinHash.
        if let Ok(sig) = mh.fingerprint(&txt) {
            let bytes = bytemuck::bytes_of(&sig);
            fs::write(
                golden.join("minhash").join(format!("{stem}_h128_k5.bin")),
                bytes,
            )
            .unwrap();
        }

        // SimHash.
        if let Ok(sig) = sh.fingerprint(&txt) {
            let bytes = bytemuck::bytes_of(&sig);
            fs::write(
                golden.join("simhash").join(format!("{stem}_b64.bin")),
                bytes,
            )
            .unwrap();
        }

        println!("regenerated: {stem}");
    }
}
