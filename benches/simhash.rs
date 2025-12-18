//! `criterion` SimHash benchmarks.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use txtfp::{Canonicalizer, Fingerprinter, SimHashFingerprinter, WordTokenizer};

fn lorem() -> &'static str {
    include_str!("../tests/data/corpora/lorem_ipsum.txt")
}

fn b64_5kb(c: &mut Criterion) {
    let f = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer);
    let input = lorem();
    let mut g = c.benchmark_group("simhash");
    g.throughput(Throughput::Bytes(input.len() as u64));
    g.bench_function("b64_5kb", |b| b.iter(|| f.fingerprint(input).unwrap()));
}

criterion_group!(benches, b64_5kb);
criterion_main!(benches);
