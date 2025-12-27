//! Internationalization tests: every classical fingerprinter must
//! handle a representative document in each major script without
//! panicking, and produce stable output.

use txtfp::{
    Canonicalizer, Fingerprinter, MinHashFingerprinter, ShingleTokenizer, SimHashFingerprinter,
    WordTokenizer, hamming, jaccard,
};

const CORPUS: &[(&str, &str)] = &[
    (
        "english",
        "the quick brown fox jumps over the lazy dog at noon",
    ),
    (
        "simplified-chinese",
        "我爱你 中文测试 世界 你好 早上好 谢谢你",
    ),
    (
        "japanese",
        "私は日本語を勉強しています 漢字とひらがなとカタカナ",
    ),
    ("korean", "안녕하세요 한국어로 만나서 반갑습니다 서울"),
    ("arabic", "مرحبا بالعالم اختبار باللغة العربية الكتابة"),
    ("hebrew", "שלום עולם בדיקה בעברית כתיבה מימין לשמאל"),
    ("devanagari-hindi", "नमस्ते दुनिया हिंदी में परीक्षण लेखन"),
    ("russian", "привет мир тестирование на русском языке слова"),
    ("thai", "สวัสดีชาวโลก การทดสอบภาษาไทยไม่มีช่องว่าง"),
    ("greek", "γεια σου κόσμε δοκιμή στα ελληνικά γράμματα"),
    ("emoji-zwj", "👨‍👩‍👧‍👦 family emoji 🇺🇸 flag 🏳️‍🌈 rainbow"),
    ("vietnamese", "tiếng Việt có dấu phụ thử nghiệm chữ"),
];

fn minhash() -> MinHashFingerprinter<ShingleTokenizer<WordTokenizer>, 128> {
    MinHashFingerprinter::<_, 128>::new(
        Canonicalizer::default(),
        ShingleTokenizer {
            k: 3,
            inner: WordTokenizer,
        },
    )
}

fn simhash() -> SimHashFingerprinter<WordTokenizer> {
    SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer)
}

#[test]
fn canonicalization_does_not_panic_on_any_corpus() {
    let c = Canonicalizer::default();
    for (label, text) in CORPUS {
        let _ = c.canonicalize(text);
        // Idempotent.
        let once = c.canonicalize(text);
        let twice = c.canonicalize(&once);
        assert_eq!(once, twice, "non-idempotent on {label}");
    }
}

#[test]
fn minhash_fingerprints_every_script_deterministically() {
    let f = minhash();
    for (label, text) in CORPUS {
        let a = f
            .fingerprint(text)
            .unwrap_or_else(|e| panic!("minhash failed on {label}: {e}"));
        let b = f
            .fingerprint(text)
            .unwrap_or_else(|e| panic!("minhash failed on {label}: {e}"));
        assert_eq!(a, b, "non-deterministic on {label}");
        assert!((jaccard(&a, &b) - 1.0).abs() < 1e-6);
    }
}

#[test]
fn simhash_fingerprints_every_script_deterministically() {
    let f = simhash();
    for (label, text) in CORPUS {
        let a = f
            .fingerprint(text)
            .unwrap_or_else(|e| panic!("simhash failed on {label}: {e}"));
        let b = f
            .fingerprint(text)
            .unwrap_or_else(|e| panic!("simhash failed on {label}: {e}"));
        assert_eq!(a, b, "non-deterministic on {label}");
        assert_eq!(hamming(a, b), 0);
    }
}

#[test]
fn cross_script_minhash_signatures_are_distinct() {
    let f = minhash();
    let sigs: Vec<_> = CORPUS
        .iter()
        .map(|(label, t)| (label, f.fingerprint(t).unwrap()))
        .collect();
    for i in 0..sigs.len() {
        for j in (i + 1)..sigs.len() {
            let (la, sa) = sigs[i];
            let (lb, sb) = sigs[j];
            assert_ne!(sa, sb, "{la} collided with {lb}");
        }
    }
}
