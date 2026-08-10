//! Adversarial inputs: the canonicalizer must collapse visually-
//! identical strings, and the fingerprinters must not panic on
//! pathological codepoint sequences.

use txtfp::{
    Canonicalizer, CanonicalizerBuilder, Fingerprinter, MinHashFingerprinter, ShingleTokenizer,
    SimHashFingerprinter, WordTokenizer, hamming, jaccard,
};

#[test]
fn zero_width_joiner_injection_is_collapsed() {
    let c = Canonicalizer::default();
    let inner = c.canonicalize("hello\u{200B}world\u{200C}foo\u{200D}bar");
    let plain = c.canonicalize("helloworldfoobar");
    assert_eq!(inner, plain);
}

#[test]
fn rlo_trojan_source_is_stripped() {
    // RLO + a "harmless" word + the actual bytes the attacker wants
    // executed. After canonicalization the visual rendering and the
    // canonical bytes must agree.
    let c = Canonicalizer::default();
    let s = c.canonicalize("admin\u{202E}drow");
    assert!(!s.chars().any(|ch| ch == '\u{202E}'));
}

#[test]
fn variation_selectors_are_dropped() {
    let c = Canonicalizer::default();
    assert_eq!(c.canonicalize("a\u{FE0F}"), "a");
    assert_eq!(c.canonicalize("a\u{FE00}b"), "ab");
    // Tag character.
    assert_eq!(c.canonicalize("\u{E007F}"), "");
}

#[test]
fn bom_is_dropped() {
    let c = Canonicalizer::default();
    assert_eq!(c.canonicalize("\u{FEFF}hello"), "hello");
}

#[test]
fn nfkc_collapses_compat_forms() {
    let c = Canonicalizer::default();
    // Full-width ASCII and circled digits collapse to plain ASCII.
    assert_eq!(c.canonicalize("ＡＢＣ123"), "abc123");
    // ﬁ ligature → fi.
    assert_eq!(c.canonicalize("ﬁle"), "file");
    // Superscript digit.
    assert_eq!(c.canonicalize("e²"), "e2");
}

#[test]
fn nfc_bomb_does_not_oom() {
    // 50 KB of combining marks behind one base char. Naive
    // implementations can blow up, but NFC normalization caps growth.
    let c = Canonicalizer::default();
    let mut s = String::with_capacity(50_000);
    s.push('a');
    for _ in 0..10_000 {
        s.push('\u{0301}');
    }
    let out = c.canonicalize(&s);
    assert!(out.len() <= s.len() * 4);
}

#[test]
fn fingerprinters_dont_panic_on_unicode_noise() {
    let canon = Canonicalizer::default();
    let mh = MinHashFingerprinter::<_, 64>::new(
        canon.clone(),
        ShingleTokenizer {
            k: 3,
            inner: WordTokenizer,
        },
    );
    let sh = SimHashFingerprinter::new(canon, WordTokenizer);

    // Inputs designed to provoke pathological tokenization.
    let pathological = [
        "\u{200B}\u{200C}\u{200D}\u{FEFF}", // zero-widths only — empty after canon
        "\u{202E}\u{202D}\u{202C}",         // bidi controls only
        "\u{0301}\u{0301}\u{0301}",         // combining marks without a base
        "a\u{0301}\u{0301}\u{0301}\u{0301}", // base + 4 combining marks
        "a a a a a a a a a a a a a a a a a a", // many duplicate words
        "🇺🇸🇨🇦🇲🇽🇫🇷🇩🇪🇯🇵",                     // flag emoji sequence
    ];
    for input in &pathological {
        // Some inputs collapse to empty post-canonicalization; we just
        // verify no panic and that errors are well-formed.
        let _ = mh.fingerprint(input);
        let _ = sh.fingerprint(input);
    }
}

#[test]
fn empty_after_canonicalization_errors_cleanly() {
    let canon = Canonicalizer::default();
    let mh = MinHashFingerprinter::<_, 64>::new(
        canon,
        ShingleTokenizer {
            k: 3,
            inner: WordTokenizer,
        },
    );
    // Pure zero-width input becomes empty after canonicalization.
    let r = mh.fingerprint("\u{200B}\u{200B}\u{200B}");
    assert!(r.is_err());
}

#[test]
fn deterministic_under_canonicalizer_choice() {
    // Two canonicalizers with the same config produce identical
    // signatures; this guards against accidental capture of process-
    // global state (RNG, time, etc.).
    let c1 = CanonicalizerBuilder::default().build();
    let c2 = CanonicalizerBuilder::default().build();
    let f1 = MinHashFingerprinter::<_, 64>::new(
        c1,
        ShingleTokenizer {
            k: 3,
            inner: WordTokenizer,
        },
    );
    let f2 = MinHashFingerprinter::<_, 64>::new(
        c2,
        ShingleTokenizer {
            k: 3,
            inner: WordTokenizer,
        },
    );
    let s1 = f1.fingerprint("the quick brown fox jumps").unwrap();
    let s2 = f2.fingerprint("the quick brown fox jumps").unwrap();
    assert_eq!(s1, s2);
    assert!((jaccard(&s1, &s2) - 1.0).abs() < 1e-6);
    let h1 = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer)
        .fingerprint("the quick brown fox jumps")
        .unwrap();
    let h2 = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer)
        .fingerprint("the quick brown fox jumps")
        .unwrap();
    assert_eq!(hamming(h1, h2), 0);
}
