//! `criterion` LSH benchmarks. Requires `--features lsh`.

#[cfg(feature = "lsh")]
mod inner {
    use criterion::{Criterion, criterion_group, criterion_main};
    use txtfp::{
        Canonicalizer, Fingerprinter, LshIndex, LshIndexBuilder, MinHashFingerprinter,
        ShingleTokenizer, WordTokenizer,
    };

    fn synth_corpus(n: usize) -> alloc::vec::Vec<alloc::string::String> {
        // Slight variations of one base sentence.
        (0..n)
            .map(|i| alloc::format!("the quick brown fox jumps over the lazy dog {i}"))
            .collect()
    }

    extern crate alloc;

    fn insert_10k(c: &mut Criterion) {
        let canon = Canonicalizer::default();
        let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
        let f = MinHashFingerprinter::<_, 128>::new(canon, tok);
        let docs = synth_corpus(10_000);
        let sigs: alloc::vec::Vec<_> = docs.iter().map(|d| f.fingerprint(d).unwrap()).collect();

        c.bench_function("lsh::insert_10k", |b| {
            b.iter(|| {
                let mut idx: LshIndex<128> =
                    LshIndexBuilder::for_threshold(0.7, 128).unwrap().build();
                for (i, sig) in sigs.iter().enumerate() {
                    idx.insert(i as u64, *sig);
                }
            })
        });
    }

    fn query_10k(c: &mut Criterion) {
        let canon = Canonicalizer::default();
        let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
        let f = MinHashFingerprinter::<_, 128>::new(canon, tok);
        let docs = synth_corpus(10_000);

        let mut idx: LshIndex<128> =
            LshIndexBuilder::for_threshold(0.7, 128).unwrap().build();
        for (i, d) in docs.iter().enumerate() {
            idx.insert(i as u64, f.fingerprint(d).unwrap());
        }

        let probe = f.fingerprint(&docs[0]).unwrap();
        c.bench_function("lsh::query_10k", |b| b.iter(|| idx.query(&probe)));
    }

    criterion_group!(benches, insert_10k, query_10k);
    criterion_main!(benches);
}

#[cfg(not(feature = "lsh"))]
fn main() {}
