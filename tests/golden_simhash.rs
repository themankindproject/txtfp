//! SimHash byte-stable regression tests.

use txtfp::{Canonicalizer, Fingerprinter, SimHashFingerprinter, WordTokenizer};

fn fp() -> SimHashFingerprinter<WordTokenizer> {
    SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer)
}

fn assert_golden(input: &str, expected: &[u8]) {
    let sig = fp().fingerprint(input).unwrap();
    let actual = bytemuck::bytes_of(&sig);
    assert_eq!(
        actual, expected,
        "SimHash byte layout regression — see tests/data/golden/simhash/"
    );
}

#[test]
fn lorem_ipsum_b64() {
    let input = include_str!("data/corpora/lorem_ipsum.txt");
    let expected = include_bytes!("data/golden/simhash/lorem_ipsum_b64.bin");
    assert_golden(input, expected);
}

#[test]
fn chinese_classical_b64() {
    let input = include_str!("data/corpora/chinese_classical.txt");
    let expected = include_bytes!("data/golden/simhash/chinese_classical_b64.bin");
    assert_golden(input, expected);
}

#[test]
fn mixed_script_b64() {
    let input = include_str!("data/corpora/mixed_script.txt");
    let expected = include_bytes!("data/golden/simhash/mixed_script_b64.bin");
    assert_golden(input, expected);
}

#[test]
fn markdown_post_b64() {
    let input = include_str!("data/corpora/markdown_post.md");
    let expected = include_bytes!("data/golden/simhash/markdown_post_b64.bin");
    assert_golden(input, expected);
}

#[test]
fn emoji_zwj_b64() {
    let input = include_str!("data/corpora/emoji_zwj.txt");
    let expected = include_bytes!("data/golden/simhash/emoji_zwj_b64.bin");
    assert_golden(input, expected);
}

#[test]
fn arabic_rtl_b64() {
    let input = include_str!("data/corpora/arabic_rtl.txt");
    let expected = include_bytes!("data/golden/simhash/arabic_rtl_b64.bin");
    assert_golden(input, expected);
}

#[test]
fn signature_is_8_bytes() {
    let input = "the quick brown fox jumps over the lazy dog";
    let sig = fp().fingerprint(input).unwrap();
    assert_eq!(bytemuck::bytes_of(&sig).len(), 8);
}
