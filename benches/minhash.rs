//! `criterion` MinHash benchmarks.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use txtfp::{
    Canonicalizer, Fingerprinter, MinHashFingerprinter, ShingleTokenizer, WordTokenizer,
};

fn lorem() -> &'static str {
    include_str!("../tests/data/corpora/lorem_ipsum.txt")
}

fn h128_5kb(c: &mut Criterion) {
    let canon = Canonicalizer::default();
    let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
    let f = MinHashFingerprinter::<_, 128>::new(canon, tok);
    let input = lorem();
    let mut g = c.benchmark_group("minhash");
    g.throughput(Throughput::Bytes(input.len() as u64));
    g.bench_function("h128_5kb", |b| b.iter(|| f.fingerprint(input).unwrap()));
}

fn h64_5kb(c: &mut Criterion) {
    let canon = Canonicalizer::default();
    let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
    let f = MinHashFingerprinter::<_, 64>::new(canon, tok);
    let input = lorem();
    let mut g = c.benchmark_group("minhash");
    g.throughput(Throughput::Bytes(input.len() as u64));
    g.bench_function("h64_5kb", |b| b.iter(|| f.fingerprint(input).unwrap()));
}

criterion_group!(benches, h128_5kb, h64_5kb);
criterion_main!(benches);
