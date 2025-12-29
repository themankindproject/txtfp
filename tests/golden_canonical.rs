//! Canonicalization byte-stable regression tests.

use txtfp::Canonicalizer;

fn assert_canonical(input: &str, expected: &str) {
    let actual = Canonicalizer::default().canonicalize(input);
    assert_eq!(
        actual, expected,
        "Canonicalizer output regression — see tests/data/golden/canonical/"
    );
}

#[test]
fn lorem_ipsum_canonical() {
    let input = include_str!("data/corpora/lorem_ipsum.txt");
    let expected = include_str!("data/golden/canonical/lorem_ipsum.txt");
    assert_canonical(input, expected);
}

#[test]
fn chinese_classical_canonical() {
    let input = include_str!("data/corpora/chinese_classical.txt");
    let expected = include_str!("data/golden/canonical/chinese_classical.txt");
    assert_canonical(input, expected);
}

#[test]
fn mixed_script_canonical() {
    let input = include_str!("data/corpora/mixed_script.txt");
    let expected = include_str!("data/golden/canonical/mixed_script.txt");
    assert_canonical(input, expected);
}

#[test]
fn markdown_post_canonical() {
    let input = include_str!("data/corpora/markdown_post.md");
    let expected = include_str!("data/golden/canonical/markdown_post.txt");
    assert_canonical(input, expected);
}

#[test]
fn emoji_zwj_canonical() {
    let input = include_str!("data/corpora/emoji_zwj.txt");
    let expected = include_str!("data/golden/canonical/emoji_zwj.txt");
    assert_canonical(input, expected);
}

#[test]
fn arabic_rtl_canonical() {
    let input = include_str!("data/corpora/arabic_rtl.txt");
    let expected = include_str!("data/golden/canonical/arabic_rtl.txt");
    assert_canonical(input, expected);
}
