# Changelog

All notable changes to `txtfp` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **`Canonicalizer::canonicalize` is now idempotent on all UTF-8 inputs.**
  Two regressions found by the v0.2.1 cargo-fuzz harness within the
  first minute of running:
  1. Bidi or format codepoints (e.g. U+202A LRE, U+200B ZWSP) sitting
     between combining marks were stripped *after* NFC reordering, so
     the first call saw two short combining sequences and the second
     call (with the format char already gone) saw them merged into one
     and re-sorted by canonical combining class. Fix: pre-filter bidi
     and format codepoints from the char stream *before* feeding NFC,
     so the format char never acts as a sequence boundary.
  2. Simple casefold expansions that produce a combining mark (e.g.
     `İ` U+0130 → `i` + U+0307, ccc=230) followed by another mark with
     smaller ccc left the buffer in non-canonical order. The next
     `canonicalize` call then re-NFC'd it. Fix: re-normalize after
     casefold (the standard UAX #15 NFKC_Casefold construction —
     NFKC → toCasefold → NFKC). The second normalization is a no-op
     on the common case (no expanding folds adjacent to combining
     marks).
- **Output-byte impact.** Both fixes change `canonicalize` output only
  on the specific edge cases above. Goldens for the included fixture
  corpus (Latin / accented Latin / CJK / mixed) are unaffected; MinHash
  and SimHash signatures for inputs that don't combine bidi/format or
  expanding folds *with* combining marks are byte-identical to v0.2.1.

### Internal

- Two new regression tests in `src/canonical/mod.rs` lock in the exact
  fuzzer-found inputs:
  `idempotence_with_format_char_between_combining_marks` and
  `idempotence_with_expanding_casefold_before_combining_mark`.
- Local `cargo fuzz run canonicalize` and `minhash_streaming` each
  for 60 s post-fix: 483 K and 705 K execs respectively, zero crashes.

## [0.2.1] - 2026-05-03

Patch release: one bug fix, one v0.1.0 changelog promise delivered, two
release-quality CI additions. **No breaking changes; signature bytes
unchanged from v0.2.0.**

### Added

- **Cargo-fuzz harness sub-crate** (`fuzz/`). Closes the v0.1.0 changelog
  promise. Two targets to start:
  - `canonicalize` — asserts `Canonicalizer::canonicalize` is idempotent
    and never panics on arbitrary UTF-8.
  - `minhash_streaming` — feeds the streamer arbitrary chunked bytes
    (cuts may fall mid-codepoint) and asserts the streaming output
    matches the offline `fingerprint` output whenever the streamer
    succeeds. Verifies the chunk-boundary UTF-8 carry logic.
  Wired into a non-blocking `fuzz-smoke` CI job (60 s/target on PR).
- **`cargo-semver-checks` CI job**. Catches accidental SemVer breaks
  before tagging.
- **`RELEASING.md`** — frozen 10-step publish procedure (changelog
  → version bump → fmt/clippy/test → semver-checks → publish dry-run
  → commit → tag → push → publish → post-release verify).
- **`tests/tlsh.rs` integration test**. Covers the public TLSH surface
  end-to-end: `TLSH_MIN_INPUT_BYTES` const, identical-input zero-distance,
  similar/unrelated distance ordering, casefold integration, and the
  `sketch_bytes` raw-path divergence from `fingerprint`.

### Fixed

- **`feature = "tlsh"` alone now compiles.** The crate-root `pub mod
  classical` and `pub use classical::{Fingerprinter, StreamingFingerprinter}`
  cfgs previously omitted `tlsh`, so a `--no-default-features --features
  tlsh` build failed with "unresolved module `classical`". Both cfgs now
  include `tlsh`; CI matrix gains a `tlsh-only` and a
  `classical+tlsh+all-non-semantic` row to lock this in.

### Internal

- MinHash SIMD inner-loop investigation: confirmed via release-build
  assembly (`vpcmpltuq` + AVX-512 mask blending on `ymm` registers) that
  LLVM already auto-vectorizes `MinHashFingerprinter::sketch_canonical`
  on stable Rust. No code change — the previously-suggested hand-rolled
  `wide::u64x4` would have duplicated work the compiler already does.

## [0.2.0] - 2026-04-28

Performance-focused breaking release. Default fingerprint bytes change
on both MinHash and SimHash; pin to v0.1.x or pass
`HashFamily::MurmurHash3_x64_128` explicitly if you need parity with
v0.1.x signatures or with Python `datasketch` / `sourmash`.

### Changed (breaking)

- **Default hash family flipped from `MurmurHash3_x64_128` to `Xxh3_64`**
  for both `MinHashFingerprinter` and `SimHashFingerprinter`. The
  `xxh3_128` single-pass variant is used internally so `(lo, hi)` for
  MinHash double-hashing now comes from one call instead of two.
  Restoring v0.1.x bytes:
  ```rust
  fp.with_hasher(HashFamily::MurmurHash3_x64_128)
  ```
- **MinHash and SimHash signature bytes change** as a result of the
  default-hasher flip. The on-disk struct layout (schema u16, padding,
  H × u64) is unchanged — only the slot values differ.
- Golden fixtures under `tests/data/golden/{minhash,simhash}/`
  regenerated. `examples/regen_goldens.rs` produces the new bytes.

### Performance

Measured on `cargo bench --quick` (Linux, x86_64) vs v0.1.2 baseline:

| bench               | v0.1.2     | v0.2.0     | Δ        |
| ------------------- | ---------- | ---------- | -------- |
| `simhash::b64_5kb`  | 345.09 µs  | 204.81 µs  | **−40.7%** |
| `canonical::nfkc_5kb` | 808.83 ns  | 540.06 ns  | **−33.2%** |
| `minhash::h64_5kb`  | 93.04 µs   | 76.07 µs   | **−18.2%** |
| `lsh::insert_10k`   | 22.10 ms   | 18.76 ms   | **−15.1%** |
| `minhash::h128_5kb` | 118.68 µs  | 109.88 µs  | **−7.4%**  |
| `lsh::query_10k`    | 177.63 µs  | 393.49 µs  | **+121%** ⚠ |

The `lsh::query_10k` regression is mostly the query *returning more
candidates*, not slower per-candidate work: under the new `Xxh3_64`
default the bench corpus produces **7262 candidates per query** vs
**4470 under MurmurHash3** (1.62× more). The 10K-doc bench corpus is
a worst case for collision-heavy inputs (9/10 words shared across
all docs); the per-candidate cost is approximately constant. Pin to
`HashFamily::MurmurHash3_x64_128` if your workload looks like the
bench and you need v0.1.x query latency.

### Internal

- `Canonicalizer::canonicalize` fuses normalization + bidi/format
  strip into a single allocation (was three sequential allocations
  per non-ASCII call). Casefold remains a separate whole-string call
  to preserve multi-char folds (German `ß` → `ss`, Greek
  final-sigma).
- New ASCII fast path: inputs that are ASCII *plus* droppable
  bidi/format codepoints (BOM-prefixed CSV, ZWSP-injected text, RLO
  trojan source, variation selectors on ASCII bases) skip the full
  Unicode pipeline and run a single-pass `to_ascii_lowercase` over
  the kept bytes. Measured **17× speedup** on a 5 KB lorem corpus
  with one leading BOM and a ZWSP every 80 bytes (170 µs → 9.8 µs).
  Byte-stable with the slow path.
- `SimHashFingerprinter` for `Weighting::Tf` no longer materializes a
  per-token counts map: the streaming `±1`-per-occurrence accumulator
  is mathematically identical to deduping then weighting by tf.
  `Weighting::Uniform` and `Weighting::IdfWeighted` retain a dedup
  pass (now via `std::collections::HashMap`, was `BTreeMap`).
- `jaccard()` is SIMD-vectorized via the `wide` crate (`u64x4`) — 4×
  fewer comparisons for `H = 128` signatures.
- `LshIndex` band tables now use an identity hasher (their keys are
  `xxh3_64` digests, already well distributed); the per-id reverse
  map keeps the default ahash hasher because application ids may be
  sequential.
- Removed dead `bidi::strip` and `normalize::{nfc,nfkc}` wrapper
  modules (replaced by the fused canonicalize pipeline).

### Added

- New `wide` dependency (stable, no_std-compatible) for SIMD primitives.
- `LshIndex::extend_par` (gated on the `parallel` feature) — bulk
  insert sharded by band across the rayon thread pool. Each worker
  owns one band's hash table, so the inserts are contention-free.
  Measured **1.74×** speedup on 8 cores for the `insert_par_10k`
  bench (20.2 ms → 11.6 ms). Restricted to fresh ids (no
  replacement); `debug_assert!`s on duplicates.

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

[Unreleased]: https://github.com/themankindproject/txtfp/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/themankindproject/txtfp/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/themankindproject/txtfp/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/themankindproject/txtfp/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/themankindproject/txtfp/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/themankindproject/txtfp/releases/tag/v0.1.0
