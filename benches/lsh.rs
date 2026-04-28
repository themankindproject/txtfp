//! `criterion` LSH benchmarks. Requires `--features lsh`.

#[cfg(feature = "lsh")]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[cfg(feature = "lsh")]
use criterion::{Criterion, criterion_group, criterion_main};
#[cfg(feature = "lsh")]
use txtfp::{
    Canonicalizer, Fingerprinter, LshIndex, LshIndexBuilder, MinHashFingerprinter,
    ShingleTokenizer, WordTokenizer,
};

#[cfg(feature = "lsh")]
fn synth_corpus(n: usize) -> Vec<String> {
    (0..n)
        .map(|i| format!("the quick brown fox jumps over the lazy dog {i}"))
        .collect()
}

#[cfg(feature = "lsh")]
fn insert_10k(c: &mut Criterion) {
    let canon = Canonicalizer::default();
    let tok = ShingleTokenizer {
        k: 5,
        inner: WordTokenizer,
    };
    let f = MinHashFingerprinter::<_, 128>::new(canon, tok);
    let docs = synth_corpus(10_000);
    let sigs: Vec<_> = docs.iter().map(|d| f.fingerprint(d).unwrap()).collect();

    c.bench_function("lsh::insert_10k", |b| {
        b.iter(|| {
            let mut idx: LshIndex<128> = LshIndexBuilder::for_threshold(0.7, 128).unwrap().build();
            for (i, sig) in sigs.iter().enumerate() {
                idx.insert(i as u64, *sig);
            }
        })
    });
}

#[cfg(all(feature = "lsh", feature = "parallel"))]
fn insert_par_10k(c: &mut Criterion) {
    let canon = Canonicalizer::default();
    let tok = ShingleTokenizer {
        k: 5,
        inner: WordTokenizer,
    };
    let f = MinHashFingerprinter::<_, 128>::new(canon, tok);
    let docs = synth_corpus(10_000);
    let sigs: Vec<_> = docs.iter().map(|d| f.fingerprint(d).unwrap()).collect();
    let pairs: Vec<(u64, _)> = sigs
        .iter()
        .enumerate()
        .map(|(i, s)| (i as u64, *s))
        .collect();

    c.bench_function("lsh::insert_par_10k", |b| {
        b.iter(|| {
            let mut idx: LshIndex<128> = LshIndexBuilder::for_threshold(0.7, 128).unwrap().build();
            idx.extend_par(pairs.iter().copied());
        })
    });
}

#[cfg(feature = "lsh")]
fn query_10k(c: &mut Criterion) {
    let canon = Canonicalizer::default();
    let tok = ShingleTokenizer {
        k: 5,
        inner: WordTokenizer,
    };
    let f = MinHashFingerprinter::<_, 128>::new(canon, tok);
    let docs = synth_corpus(10_000);

    let mut idx: LshIndex<128> = LshIndexBuilder::for_threshold(0.7, 128).unwrap().build();
    for (i, d) in docs.iter().enumerate() {
        idx.insert(i as u64, f.fingerprint(d).unwrap());
    }

    let probe = f.fingerprint(&docs[0]).unwrap();
    c.bench_function("lsh::query_10k", |b| b.iter(|| idx.query(&probe)));
}

#[cfg(all(feature = "lsh", feature = "parallel"))]
criterion_group!(benches, insert_10k, insert_par_10k, query_10k);
#[cfg(all(feature = "lsh", not(feature = "parallel")))]
criterion_group!(benches, insert_10k, query_10k);
#[cfg(feature = "lsh")]
criterion_main!(benches);

#[cfg(not(feature = "lsh"))]
fn main() {}
