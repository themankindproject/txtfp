# txtfp

[![Crates.io](https://img.shields.io/crates/v/txtfp.svg)](https://crates.io/crates/txtfp)
[![Docs.rs](https://docs.rs/txtfp/badge.svg)](https://docs.rs/txtfp)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](#license)

`txtfp` is a Rust text-fingerprinting SDK: deterministic, byte-stable hashes
for text deduplication, near-duplicate detection, and semantic search.

It is the text counterpart to [`audiofp`](https://crates.io/crates/audiofp)
(audio fingerprinting) and `imgfprint` (image fingerprinting), and is
consumed by the cross-modal `ucfp` integrator crate.

## Highlights

- **MinHash + LSH** — Jaccard-similarity sketches for set-style document
  deduplication (default features).
- **SimHash** — locality-sensitive 64-bit fingerprints for near-duplicate
  detection (default features).
- **ONNX semantic embeddings** — `LocalProvider` (BGE, E5, MiniLM, Nomic, …)
  via `ort`, plus pluggable cloud providers (OpenAI, Voyage, Cohere)
  behind feature flags.
- **Production-grade canonicalization** — NFKC + simple casefold + Bidi /
  format-character stripping, with optional UTS #39 confusable skeleton
  for security-sensitive matching.
- **`no_std + alloc`-clean default features.** Targets
  `wasm32-unknown-unknown` with the default features (`std`, `minhash`,
  `simhash`).
- **Byte-stable, semver-frozen hash layouts** — every signature is prefixed
  with a `u16` schema version. v0.1.x patch releases never change golden
  bytes.

## Quick start

```rust,no_run
use txtfp::{
    canonical::Canonicalizer,
    classical::minhash::{MinHashFingerprinter, jaccard},
    tokenize::{ShingleTokenizer, WordTokenizer},
    Fingerprinter,
};

let canonicalizer = Canonicalizer::default();
let tokenizer     = ShingleTokenizer { k: 5, inner: WordTokenizer };
let fp            = MinHashFingerprinter::<_, 128>::new(canonicalizer, tokenizer);

let a = fp.fingerprint("the quick brown fox jumps over the lazy dog").unwrap();
let b = fp.fingerprint("the quick brown fox leaps over the lazy dog").unwrap();

let similarity = jaccard(&a, &b);
assert!(similarity > 0.5);
```

See [`examples/`](examples/) for end-to-end deduplication, near-duplicate,
and semantic-search workflows.

## Cargo features

| Feature      | Default | Pulls                                                   |
| ------------ | :-----: | ------------------------------------------------------- |
| `std`        |   ✅    | libstd. Without it, `no_std + alloc`.                    |
| `minhash`    |   ✅    | MinHash sketcher.                                        |
| `simhash`    |   ✅    | SimHash sketcher.                                        |
| `lsh`        |         | Banded LSH index over MinHash signatures.                |
| `markup`     |         | `html_to_text`, `markdown_to_text`.                      |
| `pdf`        |         | `pdf_to_text` (via `pdf-extract`).                       |
| `cjk`        |         | `CjkTokenizer` (jieba, lindera).                         |
| `security`   |         | UTS #39 confusable skeleton in the canonicalizer.        |
| `serde`      |         | `Serialize`/`Deserialize` on signatures.                 |
| `parallel`   |         | Rayon-powered batch helpers.                             |
| `tlsh`       |         | Re-export `tlsh2` types behind a `Fingerprint` variant.  |
| `semantic`   |         | `LocalProvider` via `ort` + Hugging Face Hub.            |
| `openai`     |         | `OpenAiProvider` (HTTP, async).                          |
| `voyage`     |         | `VoyageProvider` (HTTP, async).                          |
| `cohere`     |         | `CohereProvider` (HTTP, async).                          |

## Performance

Single-thread throughput on a 2024-class x86_64 laptop, **fat-LTO release
build with `RUSTFLAGS="-C target-cpu=native"` and mimalloc** as the
benches' global allocator, measured with `cargo bench --features lsh`
over the 5 KB `lorem_ipsum` (ASCII) corpus:

| Operation                | Time        | Throughput          |
| ------------------------ | ----------- | ------------------- |
| MinHash sketch (h=128)   | ~90 µs/doc  | **~11K docs/sec**   |
| MinHash sketch (h=64)    | ~72 µs/doc  | ~14K docs/sec       |
| SimHash sketch (b=64)    | ~123 µs/doc | ~8K docs/sec        |
| Canonicalize NFKC (ASCII)| ~410 ns/doc | ~2.4M docs/sec      |
| LSH insert (h=128)       | ~2.0 µs/sig | ~510K signatures/s  |
| LSH query (10K-doc index)| ~177 µs     | ~5.6K queries/sec   |

The canonicalizer takes a single-pass ASCII fast path for ASCII input
(no NFKC trip, no Bidi/format scan). The classical sketchers route
through a callback-style `Tokenizer::for_each_token` that skips the
per-token `String` allocation; for `Word + Shingle` (the dominant
configuration) this collapses ~2N allocations into a single re-used
buffer.

SIMD-vectorizing the H-loop and the SimHash bit accumulator should
push MinHash and SimHash to ≥25K docs/sec; that work is queued for
v0.2 to keep the v0.1.x byte-stable contract clean.

## Status

v0.1.0 — initial release. The classical algorithms (MinHash, SimHash, LSH,
TLSH) and canonicalization pipeline are stable. Hash byte layouts are
semver-frozen as of this release.

## License

Licensed under the [MIT license](LICENSE-MIT).

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you shall be licensed as above,
without any additional terms or conditions.
