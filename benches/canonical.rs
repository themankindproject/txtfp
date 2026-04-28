//! `criterion` canonicalizer benchmarks.

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use txtfp::Canonicalizer;

fn lorem() -> &'static str {
    include_str!("../tests/data/corpora/lorem_ipsum.txt")
}

fn bom_zwsp_lorem() -> &'static str {
    // Prepend a BOM, sprinkle a ZWSP every ~80 bytes — exercises the
    // "ASCII + droppable format/bidi" fast path. Real-world shape: a
    // CSV / web page that opens with U+FEFF and contains a few hidden
    // injection codepoints in otherwise-ASCII content.
    let base = lorem();
    let mut s = String::with_capacity(base.len() + 128);
    s.push('\u{FEFF}');
    let mut last = 0;
    let bytes = base.as_bytes();
    for i in (80..bytes.len()).step_by(80) {
        // Land on a valid UTF-8 boundary (the corpus is pure ASCII so any byte is fine).
        s.push_str(core::str::from_utf8(&bytes[last..i]).unwrap());
        s.push('\u{200B}');
        last = i;
    }
    s.push_str(core::str::from_utf8(&bytes[last..]).unwrap());
    Box::leak(s.into_boxed_str())
}

fn nfkc_5kb(c: &mut Criterion) {
    let canon = Canonicalizer::default();
    let input = lorem();
    let mut g = c.benchmark_group("canonical");
    g.throughput(Throughput::Bytes(input.len() as u64));
    g.bench_function("nfkc_5kb", |b| b.iter(|| canon.canonicalize(input)));
}

fn nfkc_5kb_bom_zwsp(c: &mut Criterion) {
    let canon = Canonicalizer::default();
    let input = bom_zwsp_lorem();
    let mut g = c.benchmark_group("canonical");
    g.throughput(Throughput::Bytes(input.len() as u64));
    g.bench_function("nfkc_5kb_bom_zwsp", |b| {
        b.iter(|| canon.canonicalize(input))
    });
}

criterion_group!(benches, nfkc_5kb, nfkc_5kb_bom_zwsp);
criterion_main!(benches);
