//! MinHash byte-stable regression tests.
//!
//! Each test computes a MinHash signature over a frozen input file and
//! asserts byte-for-byte equality with the corresponding `.bin` fixture
//! committed under `tests/data/golden/minhash/`.
//!
//! **Failing one of these tests means the algorithm output drifted.**
//! That is a hard breakage — fix the code, do not regenerate the
//! golden, except at a v0.2 release with an explicit byte-layout
//! version bump.

use txtfp::{Canonicalizer, Fingerprinter, MinHashFingerprinter, ShingleTokenizer, WordTokenizer};

fn fp() -> MinHashFingerprinter<ShingleTokenizer<WordTokenizer>, 128> {
    MinHashFingerprinter::<_, 128>::new(
        Canonicalizer::default(),
        ShingleTokenizer {
            k: 5,
            inner: WordTokenizer,
        },
    )
}

fn assert_golden(input: &str, expected: &[u8]) {
    let sig = fp().fingerprint(input).unwrap();
    let actual = bytemuck::bytes_of(&sig);
    assert_eq!(
        actual, expected,
        "MinHash byte layout regression — see tests/data/golden/minhash/"
    );
}

#[test]
fn lorem_ipsum_h128_k5() {
    let input = include_str!("data/corpora/lorem_ipsum.txt");
    let expected = include_bytes!("data/golden/minhash/lorem_ipsum_h128_k5.bin");
    assert_golden(input, expected);
}

#[test]
fn chinese_classical_h128_k5() {
    let input = include_str!("data/corpora/chinese_classical.txt");
    let expected = include_bytes!("data/golden/minhash/chinese_classical_h128_k5.bin");
    assert_golden(input, expected);
}

#[test]
fn mixed_script_h128_k5() {
    let input = include_str!("data/corpora/mixed_script.txt");
    let expected = include_bytes!("data/golden/minhash/mixed_script_h128_k5.bin");
    assert_golden(input, expected);
}

#[test]
fn markdown_post_h128_k5() {
    let input = include_str!("data/corpora/markdown_post.md");
    let expected = include_bytes!("data/golden/minhash/markdown_post_h128_k5.bin");
    assert_golden(input, expected);
}

#[test]
fn emoji_zwj_h128_k5() {
    let input = include_str!("data/corpora/emoji_zwj.txt");
    let expected = include_bytes!("data/golden/minhash/emoji_zwj_h128_k5.bin");
    assert_golden(input, expected);
}

#[test]
fn arabic_rtl_h128_k5() {
    let input = include_str!("data/corpora/arabic_rtl.txt");
    let expected = include_bytes!("data/golden/minhash/arabic_rtl_h128_k5.bin");
    assert_golden(input, expected);
}

#[test]
fn signature_byte_size_is_8_plus_h_times_8() {
    let input = "the quick brown fox jumps over the lazy dog";
    let sig = fp().fingerprint(input).unwrap();
    assert_eq!(bytemuck::bytes_of(&sig).len(), 8 + 128 * 8);
}

#[test]
fn schema_byte_is_one_at_offset_zero() {
    let input = "the quick brown fox jumps over the lazy dog";
    let sig = fp().fingerprint(input).unwrap();
    let bytes = bytemuck::bytes_of(&sig);
    // schema is u16 LE at offset 0..2; v0.1.0 schema is 1.
    assert_eq!(&bytes[..2], &1u16.to_le_bytes());
}

#[test]
fn padding_bytes_are_zero() {
    let input = "the quick brown fox jumps over the lazy dog";
    let sig = fp().fingerprint(input).unwrap();
    let bytes = bytemuck::bytes_of(&sig);
    // _pad: [u8; 6] at offset 2..8.
    assert_eq!(&bytes[2..8], &[0u8; 6]);
}
