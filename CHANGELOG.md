# Changelog

All notable changes to `txtfp` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2025-12-31

### Changed

- `jieba-rs` upgraded 0.7 → 0.9 — drops the unmaintained `fxhash`
  transitive (clears RUSTSEC-2025-0057 from the published dep tree).
  v0.1.1 still pulled `fxhash` via jieba 0.7; v0.1.2 is the first
  release with a fully-maintained dependency closure (the only
  remaining advisory ignore is `paste` via `ort-sys`, RUSTSEC-2024-0436).

## [0.1.1] - 2025-12-31

### Added

- **`cjk-japanese` feature** — real Japanese morphological tokenization
  via `lindera` 3.x with embedded IPADIC. Lazy-loaded once per process
  via `OnceLock`.
- **`cjk-korean` feature** — real Korean morphological tokenization via
  `lindera` 3.x with embedded ko-dic.
- **`CjkSegmenter::LinderaKoDic` variant** — lindera + ko-dic for
  Korean. The pre-existing `CjkSegmenter::Lindera` variant now performs
  real lindera + IPADIC tokenization when the `cjk-japanese` feature is
  enabled (vs the v0.1.0 UAX-29 stub).

### Changed

- **MSRV bumped from 1.85 → 1.88.** Unblocks newer `jieba-rs`,
  `lindera` 3.x, `time` 0.3.46+, and the fix for RUSTSEC-2026-0009.
- **`lsh` is now in the default feature set.** Most callers using
  MinHash at scale want LSH; opt out via `default-features = false`.
- `jieba-rs` upgraded from 0.6 → 0.7 (drops the unmaintained `fxhash`
  transitive dep — clears RUSTSEC-2025-0057).
- `time` 0.3.45 → 0.3.46+ via the MSRV bump (clears RUSTSEC-2026-0009).

### Removed

- `RUSTSEC-2025-0057` and `RUSTSEC-2026-0009` advisory ignores
  (no longer applicable after the MSRV / dep upgrades).

### Notes

- Hash byte layouts remain frozen — every golden-byte test continues
  to pass.
- `cjk-japanese` and `cjk-korean` add significant compressed-binary
  size (~50 MiB and ~150 MiB respectively) because `lindera` embeds
  the dictionary. Both build steps download the dictionary from
  `Lindera.dev` at compile time; offline builds need
  `LINDERA_CACHE` or `LINDERA_DICTIONARIES_PATH` set.

## [0.1.0] - 2025-11-04

Initial release.

### Added

- Canonicalization pipeline (`Canonicalizer`, `CanonicalizerBuilder`):
  NFC / NFKC normalization, simple Unicode casefold, Bidi / format
  character stripping, optional UTS #39 confusable skeleton behind the
  `security` feature.
- Tokenizers: `WordTokenizer`, `GraphemeTokenizer`, `ShingleTokenizer`,
  and `CjkTokenizer` (`cjk` feature).
- Classical fingerprinters with both offline (`Fingerprinter`) and
  streaming (`StreamingFingerprinter`) variants:
  - **MinHash** — `MinHashSig<H>` (`bytemuck::Pod`), `MinHashFingerprinter`,
    `jaccard`. Default `H = 128`, MurmurHash3-x64-128 hash family for
    `datasketch` parity.
  - **SimHash** — `SimHash64`, `SimHashFingerprinter`, `hamming`,
    `cosine_estimate` (Charikar 2002).
- Banded **LSH** index over MinHash signatures (`lsh` feature).
- Semantic layer (`semantic` feature):
  - `EmbeddingProvider` trait, `Embedding` struct, `semantic_similarity`
    helper. Trait shape is parity-compatible with `imgfprint`.
  - `LocalProvider` over ONNX models hosted on the Hugging Face Hub,
    with a built-in pooling table for popular embedders (BGE, E5, MiniLM,
    Nomic, GTE, Snowflake Arctic, mxbai).
  - `OpenAiProvider`, `VoyageProvider`, `CohereProvider` behind their
    individual feature flags.
  - `ChunkingStrategy` and `chunk_for_model` for long-input handling.
- Markup helpers: `html_to_text`, `markdown_to_text` (`markup` feature).
- PDF helper: `pdf_to_text` (`pdf` feature).
- TLSH re-export behind the `tlsh` feature.
- Unified `Fingerprint` enum with `FingerprintMetadata`. Stable
  `Fingerprint::name()` format frozen at this release.

### Notes

- Hash byte layouts (`MinHashSig<H>`, `SimHash64`) are **semver-frozen** as
  of v0.1.0. They will not change across v0.1.x patch releases.
- Default features (`std`, `minhash`, `simhash`) build cleanly on
  `wasm32-unknown-unknown`.
- Fuzz harnesses (cargo-fuzz, separate sub-crate) are deferred to v0.2;
  the crate ships as a single publishable Cargo package, mirroring
  `audiofp`'s layout.

[Unreleased]: https://github.com/themankindproject/txtfp/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/themankindproject/txtfp/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/themankindproject/txtfp/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/themankindproject/txtfp/releases/tag/v0.1.0
