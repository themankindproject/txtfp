# Changelog

All notable changes to `txtfp` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/themankindproject/txtfp/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/themankindproject/txtfp/releases/tag/v0.1.0
