//! `criterion` canonicalizer benchmarks.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use txtfp::Canonicalizer;

fn lorem() -> &'static str {
    include_str!("../tests/data/corpora/lorem_ipsum.txt")
}

fn nfkc_5kb(c: &mut Criterion) {
    let canon = Canonicalizer::default();
    let input = lorem();
    let mut g = c.benchmark_group("canonical");
    g.throughput(Throughput::Bytes(input.len() as u64));
    g.bench_function("nfkc_5kb", |b| b.iter(|| canon.canonicalize(input)));
}

criterion_group!(benches, nfkc_5kb);
criterion_main!(benches);
