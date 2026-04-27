# txtfp

[![Crates.io](https://img.shields.io/crates/v/txtfp.svg)](https://crates.io/crates/txtfp)
[![Docs.rs](https://docs.rs/txtfp/badge.svg)](https://docs.rs/txtfp)
[![License](https://img.shields.io/crates/l/txtfp)](LICENSE-MIT)
[![Build Status](https://img.shields.io/github/actions/workflow/status/themankindproject/txtfp/ci.yml)](https://github.com/themankindproject/txtfp/actions)
![Rust Version](https://img.shields.io/badge/rust-1.85%2B-blue)

High-performance text fingerprinting SDK for Rust with **classical sketches** (MinHash + LSH, SimHash, TLSH), **Unicode-correct canonicalization**, and **semantic embeddings** (ONNX local + OpenAI / Voyage / Cohere).

## Overview

`txtfp` produces compact, deterministic, byte-stable hashes for text deduplication, near-duplicate detection, and semantic search:

| Method      | Use case                          | Output         | Complexity      |
| ----------- | --------------------------------- | -------------- | --------------- |
| **MinHash** | Set-similarity dedup (Jaccard)    | `[u64; H]`     | O(n) sketch     |
| **LSH**     | Sub-linear near-duplicate lookup  | bucketed index | O(1) avg query  |
| **SimHash** | Bit-LSH near-dup (Hamming)        | `u64`          | O(n) sketch     |
| **TLSH**    | Byte-level locality-sensitive hash | hex string     | O(n) sketch     |
| **Embedding** | Semantic similarity (ANN)        | `Vec<f32>`     | model-dependent |

It is the text counterpart to [`audiofp`](https://crates.io/crates/audiofp) (audio) and `imgfprint` (image), and is consumed by the cross-modal `ucfp` integrator.

Perfect for:

- LLM training-set deduplication
- RAG retrieval ranking
- Content moderation
- Plagiarism detection
- Email / document de-dup at scale

## Features

- **Byte-stable hash layouts** — `MinHashSig<H>` and `SimHash64` are `repr(C)` `bytemuck::Pod`. Schema-versioned, semver-frozen, golden-byte enforced (18 fixtures).
- **Production canonicalization** — NFKC + simple casefold + Bidi/format strip; defends against Trojan Source, ZWJ injection, NFC bombs.
- **`no_std + alloc`-clean default features** — builds for `wasm32-unknown-unknown` out of the box.
- **Streaming + offline fingerprinters** — every classical sketcher has both a `Fingerprinter` (whole-doc) and `StreamingFingerprinter` (chunk-fed) variant.
- **Cloud + local embeddings** — `LocalProvider` (ONNX via `ort` + Hugging Face Hub), `OpenAiProvider`, `VoyageProvider`, `CohereProvider` with retry / `Retry-After` / exponential backoff.
- **Markup helpers** — HTML → text, Markdown → text, PDF → text (with 30 s timeout).
- **Unicode security** — UTS #39 confusable skeleton behind the `security` feature.
- **CJK tokenizer** — `jieba-rs` with `OnceLock`-lazy dictionary for Simplified Chinese.
- **Cross-SDK parity** — `EmbeddingProvider`, `Embedding`, `semantic_similarity`, `FORMAT_VERSION` aligned with `imgfprint` / `audiofp`.

## Installation

```toml
[dependencies]
txtfp = "0.1"
```

### Feature flags

| Feature      | Default | Pulls                                                       |
| ------------ | :-----: | ----------------------------------------------------------- |
| `std`        |   ✅    | libstd. Without it, `no_std + alloc`.                       |
| `minhash`    |   ✅    | MinHash sketcher.                                           |
| `simhash`    |   ✅    | SimHash sketcher.                                           |
| `lsh`        |         | Banded LSH index over MinHash signatures.                   |
| `markup`     |         | `html_to_text`, `markdown_to_text`.                         |
| `pdf`        |         | `pdf_to_text` (with timeout).                               |
| `cjk`        |         | `CjkTokenizer` (jieba).                                     |
| `tlsh`       |         | `TlshFingerprinter`.                                        |
| `security`   |         | UTS #39 confusable skeleton in the canonicalizer.           |
| `serde`      |         | `Serialize` / `Deserialize` on signatures (incl. const-generic MinHash). |
| `parallel`   |         | Rayon-powered batch helpers.                                |
| `semantic`   |         | `LocalProvider` via `ort` + Hugging Face Hub.               |
| `openai`     |         | `OpenAiProvider`.                                           |
| `voyage`     |         | `VoyageProvider`.                                           |
| `cohere`     |         | `CohereProvider`.                                           |

Minimal build (no_std + alloc, MinHash + SimHash only):

```toml
[dependencies]
txtfp = { version = "0.1", default-features = false, features = ["minhash", "simhash"] }
```

With LSH:

```toml
[dependencies]
txtfp = { version = "0.1", features = ["lsh"] }
```

With local ONNX embeddings:

```toml
[dependencies]
txtfp = { version = "0.1", features = ["semantic"] }
```

## Quick Start

```rust
use txtfp::{
    Canonicalizer, Fingerprinter, MinHashFingerprinter,
    ShingleTokenizer, WordTokenizer, jaccard,
};

fn main() -> Result<(), txtfp::Error> {
    let canon = Canonicalizer::default();
    let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
    let fp = MinHashFingerprinter::<_, 128>::new(canon, tok);

    let a = fp.fingerprint("the quick brown fox jumps over the lazy dog at noon today")?;
    let b = fp.fingerprint("the quick brown fox jumps over the lazy dog at dusk today")?;

    let similarity = jaccard(&a, &b);
    println!("Jaccard estimate: {:.2}", similarity);

    if similarity > 0.6 {
        println!("near-duplicate");
    }
    Ok(())
}
```

### LSH for sub-linear near-duplicate lookup

```rust
# #[cfg(feature = "lsh")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::{
    Canonicalizer, Fingerprinter, LshIndex, LshIndexBuilder,
    MinHashFingerprinter, ShingleTokenizer, WordTokenizer,
};

let canon = Canonicalizer::default();
let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
let fp = MinHashFingerprinter::<_, 128>::new(canon, tok);

// Optimize bands/rows for a Jaccard threshold of 0.7.
let mut idx: LshIndex<128> = LshIndexBuilder::for_threshold(0.7, 128)?.build();

idx.insert(0, fp.fingerprint("the quick brown fox jumps over the lazy dog at noon today")?);
idx.insert(1, fp.fingerprint("astronomers detect cosmic background radiation")?);

let probe = fp.fingerprint("the quick brown fox jumps over the lazy dog at dusk today")?;
let neighbours = idx.query_with_threshold(&probe, 0.5);
println!("near-duplicates: {neighbours:?}");
# Ok(()) }
```

## Documentation

For the complete API reference and worked examples, see [USAGE.md](USAGE.md).

## Architecture

### Pipeline

```
input bytes
    │
    ▼
canonicalize  (NFKC + casefold + Bidi/format strip)
    │
    ▼
tokenize      (Word | Grapheme | Shingle | CJK)
    │
    ▼
sketch        (MinHash | SimHash | TLSH | Embedding)
    │
    ▼
compare       (jaccard | hamming | cosine_estimate | semantic_similarity)
```

Every layer is independently swappable: pick a canonicalizer config, plug any `Tokenizer`, choose a `HashFamily`, and the same input always produces the same byte-stable signature.

### Signature byte layouts (frozen for v0.1.x)

```
MinHashSig<H>                       SimHash64
├── schema: u16  (= 1)              └── 8 bytes (u64, little-endian)
├── _pad:   [u8; 6] (zero)
└── hashes: [u64; H], LE

Total size: 8 + 8*H bytes
```

These layouts are enforced by 18 byte-frozen golden-test fixtures (`tests/data/golden/`). Failing a golden test is a hard breakage that requires a major-version bump.

### Algorithms

- **MinHash** uses double-hashing (Indyk–Motwani 1998 + Kirsch–Mitzenmacher 2008): one `murmur3-x64-128` per shingle, then derive `H` slots as `low ^ (i * high)`. Datasketch parity.
- **SimHash** is Charikar 2002: token-weighted bag, 64-lane signed accumulator, sign-extract.
- **LSH** is banded: `bands * rows == H`. `LshIndexBuilder::for_threshold` numerically minimizes false-positive + false-negative integral over `[0, threshold]` and `[threshold, 1]` to pick the partition.
- **TLSH** wraps `tlsh2` 128/1.
- **Local embeddings** load HF Hub ONNX models, tokenize with `tokenizers`, run `ort` 2.0, and pool with `Pooling::{Cls, Mean, MeanNoNorm, Max}`. The pooling default is looked up per-model (BGE → Cls, E5 → Mean, etc.).

## Performance

Single-thread throughput on a 2024-class x86_64 laptop, **fat-LTO release with `RUSTFLAGS="-C target-cpu=native"` and mimalloc** as the benches' global allocator, measured with `cargo bench --features lsh` over the 5 KB `lorem_ipsum` (ASCII) corpus:

| Operation                    | Time        | Throughput            |
| ---------------------------- | ----------- | --------------------- |
| MinHash sketch (h=128)       | ~90 µs/doc  | **~11K docs/sec**     |
| MinHash sketch (h=64)        | ~72 µs/doc  | ~14K docs/sec         |
| SimHash sketch (b=64)        | ~123 µs/doc | ~8K docs/sec          |
| Canonicalize NFKC (ASCII)    | ~410 ns/doc | ~2.4M docs/sec        |
| LSH insert (h=128)           | ~2.0 µs/sig | ~510K signatures/sec  |
| LSH query (10K-doc index)    | ~177 µs     | ~5.6K queries/sec     |
| Hamming compare (`hamming`)  | ~0.5 ns     | ~2B comparisons/sec   |
| Jaccard compare (h=128)      | ~50 ns      | ~20M comparisons/sec  |

Run benchmarks:

```bash
RUSTFLAGS="-C target-cpu=native" cargo bench --features lsh
```

### Optimization knobs

- The canonicalizer takes a single-pass ASCII fast path when the input is pure ASCII and the config is the default.
- `Tokenizer::for_each_token` is a callback-style API that skips per-token `String` allocation; classical sketchers route through it.
- mimalloc gives ~2× on `LSH insert` (alloc-heavy), ~6% on SimHash, marginal elsewhere.
- SIMD-vectorizing the H-loop and the SimHash bit accumulator should push MinHash and SimHash to ≥25K docs/sec. Queued for v0.2 to keep the v0.1.x byte-stable contract clean.

## Stability

- **Hash byte layouts**: frozen for v0.1.x. Golden tests enforce.
- **`EmbeddingProvider`, `Embedding`, `semantic_similarity`**: parity-compatible with `imgfprint` 0.4.x and `audiofp` 0.2.x.
- **`FORMAT_VERSION = 1`**: mirrored across the cross-modal sibling crates so the integrator (`ucfp`) can refuse to open a database whose layout predates the running build.
- **Cross-config comparisons** are gated by `FingerprintMetadata::config_hash`. Two fingerprints with different non-zero `config_hash` values must not be compared.

## Security

- **OOM protection**: streaming sketchers cap buffer at 16 MiB; `pdf_to_text` caps at 50 MiB; `pdf-extract` runs under a 30 s wall-clock timeout.
- **Trojan Source / homoglyph defense**: canonicalizer strips Bidi controls and the Cf category. `security` feature adds the UTS #39 confusable skeleton so Cyrillic 'а' folds to Latin 'a'.
- **NFC bombs bounded**: NFKC growth capped at 18× (Unicode-spec-mandated worst case).
- **API key handling**: cloud providers redact the key in `Debug` impls; never log the bearer header.
- **Deterministic output**: same input always produces the same byte-identical signature; no hidden RNG, no clock dependency.
- **Cryptographic-level attacks on the hash families**: out of scope. MurmurHash3, xxh3, and SimHash are non-cryptographic by design.

## Comparison with alternatives

| Feature                      | txtfp | datasketch (py) | sourmash (py) | rapidfuzz |
| ---------------------------- | :---: | :-------------: | :-----------: | :-------: |
| MinHash                      |  ✓   |       ✓        |      ✓       |    —     |
| Banded LSH                   |  ✓   |       ✓        |      ✓       |    —     |
| SimHash                      |  ✓   |       ✓        |      —       |    —     |
| TLSH                         |  ✓   |       —        |      —       |    —     |
| Streaming sketches           |  ✓   |       ✓        |      ✓       |    —     |
| Unicode canonicalization     |  ✓   |       —        |      —       |   ~      |
| Trojan Source defense        |  ✓   |       —        |      —       |    —     |
| Local ONNX embeddings        |  ✓   |       —        |      —       |    —     |
| Cloud embeddings (OpenAI/…)  |  ✓   |       —        |      —       |    —     |
| Byte-stable hash layouts     |  ✓   |       —        |      —       |    —     |
| `no_std + alloc`             |  ✓   |       —        |      —       |    —     |
| Pure Rust (no Python GIL)    |  ✓   |       —        |      —       |    ✓     |

## Examples

See the `examples/` directory:

- `dedup.rs` — MinHash + LSH end-to-end deduplication
- `near_dup.rs` — SimHash near-duplicate detection
- `semantic.rs` — Local ONNX embedding similarity (requires `semantic`)
- `regen_goldens.rs` — Regenerate the byte-frozen test fixtures (do not run on a v0.1.x release)

```bash
cargo run --example dedup --features lsh --release
cargo run --example near_dup --release
cargo run --example semantic --features semantic --release
```

## Contributing

Contributions welcome. The contract:

1. Fork the repository.
2. Branch (`git checkout -b feature/x`).
3. Run the matrix locally: `cargo test --no-default-features --features "std,minhash,simhash,lsh,markup,security,serde,parallel"`.
4. Run clippy: `cargo clippy --all-targets -- -D warnings`.
5. Run benches if the change touches a hot path: `cargo bench`.
6. **Never regenerate golden fixtures unless you're explicitly bumping a major version.**
7. Open a PR.

### Development

```bash
git clone https://github.com/themankindproject/txtfp
cd txtfp

# Default-feature smoke
cargo test

# Full feature surface
cargo test --features "lsh,markup,security,serde,parallel,tlsh,cjk,pdf"

# Build the docs
cargo doc --no-deps --open
```

## License

Licensed under the [MIT license](LICENSE-MIT).
