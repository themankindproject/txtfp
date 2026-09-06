# Changelog

All notable changes to `txtfp` are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **`Utf8StreamBuffer` (MinHash/SimHash streaming): a rejected chunk
  destroyed the in-progress UTF-8 carry.** On a hard-invalid chunk the
  carry was `take`n into a working buffer and dropped with it, so a
  caller that ignored the error and kept streaming silently lost the
  partial codepoint. The carry is now restored before the error
  returns. Cap enforcement also ignored carry bytes, letting
  `buffer + carry` drift up to 3 bytes past the documented
  `max_bytes`; the budget now counts both.
- **`EmbeddingProvider` docs referenced the removed `Error::Http`
  variant** (dropped with the cloud providers in v0.3.0) — a latent
  broken intra-doc link in the `semantic` feature.

### Performance

- **`Canonicalizer` ASCII fast paths now bulk-copy and lowercase
  in place** (`push_str` + `make_ascii_lowercase`) instead of pushing
  per-`char`: one memcpy plus one vectorized byte pass where the
  per-char loop re-encoded every codepoint. Byte-identical output;
  goldens unchanged.
- **`html_to_text` no longer allocates a stripped copy when the input
  contains no `<script>`/`<style>` region** — the common case for
  script-free pages is now a borrowed pass-through after a linear
  scan; owned output is only built when a region is actually dropped.
- **`MinHash`/`SimHash` streaming steady state drops one memcpy per
  chunk**: chunks arriving with an empty carry and fully-valid UTF-8
  commit directly to the buffer instead of transiting a combine
  buffer first.
- **`LshIndex::insert` on replace does one reverse-map probe instead
  of two** (`remove`-then-scrub shared helper replaces
  `contains_key` + `remove`).
- **`LocalProvider::run` no longer clones ONNX input names or builds
  heap `String` keys per inference** — session input names are read by
  borrow and the input list uses `&'static str` keys.

### Changed

- **`jaccard()` replaces the hand-rolled `wide::i64x4` kernel with an
  auto-vectorizable zip-count.** Verified in emitted asm: the idiomatic
  loop vectorizes to the same 4×u64 lane width the explicit version
  used (`pcmpeqd`+`psubq` on SSE2, `vpcmpeqq`+`vpsubq` on AVX2+) with
  fewer instructions per iteration, and `wide` was the crate's only
  use of that dependency — it is removed from `Cargo.toml`.

### Removed (dependencies)

- **`wide`** — sole consumer (`jaccard`) now auto-vectorizes without
  it; one fewer dependency in every build configuration.

### Internal

- `MinHashStreaming` and `SimHashStreaming` share one
  `BufferedStream` core (`src/classical/streaming.rs`); the public
  types, methods, and trait impls are unchanged.
- `LshIndex::extend_par` / `try_extend_par` share one insertion core;
  only their validation contracts differ.
- The three per-module L2-normalize helpers are one
  `l2_normalize_in_place` in `semantic::embedding`.
- New regression tests: UTF-8 carry survival across a rejected chunk,
  cap accounting with carry in flight.

## [0.3.1] - 2026-08-11

### Fixed

- **`html_to_text` script/style stripping was quadratic and matched
  non-tags.** `strip_script_and_style` re-lowercased the entire remaining
  input on every iteration (O(k·n) for k tags) and treated bare
  prefixes like `<scripture>` / `<styling>` as open tags, dropping the
  document tail. Rewritten as a single-pass, allocation-free scan that
  requires a real tag-name boundary; `</script >`-style closers are now
  honoured too.
- **`LshIndex::with_bands_rows` validated `bands * rows` with an
  unchecked multiply.** In release builds an overflowing product could
  wrap to exactly `H` and bypass validation before the infeasible
  `Vec::with_capacity` allocation; now uses `checked_mul` and returns
  `Error::Config` on overflow.
- **`ChunkingStrategy.overlap`** was documented as "must be < max_tokens"
  but never enforced, and `ChunkMode::Recursive` ignored overlap
  entirely. Overlap is now clamped internally so a pathological value
  cannot push chunks past the cap, and Recursive mode (plus the
  over-cap-sentence fallback in SentenceBounded) seeds the same overlap
  as the other modes.

### Performance

- **MinHash slot loop**: the per-slot `i * hi` multiply is replaced by
  the equivalent wrapping-add chain (`lo + hi + …`), one compare + add
  per slot. Byte-identical output.
- **`markdown_to_text_with`** trims in place (`drain` + `truncate`)
  instead of allocating a second `String`.
- **`LocalProvider` inference** builds `input_ids` / `attention_mask` /
  `token_type_ids` as `ndarray` views over the token vectors instead of
  allocating three `Array2`s (and a `mask` clone) per call.

### Added

- **Schema-checked deserialization.** `MinHashSig::from_bytes` /
  `MinHashSig: TryFrom<&[u8]>` validate the exact wire length *and* the
  embedded schema version, so a column written by a future `txtfp`
  fails with `Error::SchemaMismatch` instead of silently deserializing
  into a signature that would compare wrong. `SimHash64::from_bytes` /
  `TryFrom<&[u8]>` validate the 8-byte length.
- **`TlshFingerprint: FromStr + Display`** — validated hex now parses
  and renders through the standard traits.
- **Self-describing config hashes.** `MinHashFingerprinter::config_hash`,
  `SimHashFingerprinter::config_hash`, and `TlshFingerprinter::config_hash`
  bake in the canonicalizer config, tokenizer name, hash family, seed,
  and (for SimHash) weighting discriminant — cross-config comparisons
  can no longer be refused with a hand-typed string that drifts.
- **`Canonicalizer::canonicalize_into(&mut String)`** — reuses the
  caller's buffer (cleared, allocation retained) so corpus loops save
  one allocation per document; the offline fingerprinters now route
  through it.
- **`LshIndex::ids()` / `LshIndex::iter()`** — enumerate stored
  documents without reconstructing the index.
- **`LshIndex::try_extend_par`** (`parallel` feature) — the checked
  variant of `extend_par`: validates the whole batch up front and
  returns `Error::InvalidInput` on a duplicate id instead of silently
  corrupting the index in release builds.
- **Property test:** `tests/property_lsh.rs` verifies that
  `query_with_threshold` equals a brute-force Jaccard scan and that
  `query()` is a strictly-sorted, deduplicated superset over random
  signatures.
- **Fuzz target:** `fuzz/fuzz_targets/markup.rs` (HTML + Markdown →
  text, panic-freedom) added to the fuzz crate with the `markup`
  feature enabled.
- **CI:** matrix gains a zero-feature (`--no-default-features`) build so
  the canonicalize + tokenize-only surface is exercised.

## [0.3.0] - 2026-05-26

Scope-tightening major release. Drops three feature areas that diluted
the crate's stated focus on byte-stable text fingerprinting:
cloud-provider embedding wrappers (OpenAI, Voyage, Cohere), Lindera-based
Japanese / Korean tokenization, and PDF text extraction. The
`Tokenizer`, `Fingerprinter`, `EmbeddingProvider`, and signature byte
layouts are unchanged — every existing fingerprint produced by v0.2.x
continues to round-trip byte-identical under v0.3.0.

### Removed (breaking)

- **`openai`, `voyage`, `cohere` features and their providers.** The
  `EmbeddingProvider` trait is the contract; per-vendor wrappers added
  ongoing maintenance burden against three independent API surfaces
  while pulling `reqwest + tokio + serde_json` into the optional dep
  tree, and live tests for them were `#[ignore]`'d. Implement
  `EmbeddingProvider` against your own HTTP client of choice.
  Migration: ~30 lines per provider, no trait changes.
- **`pdf` feature and `txtfp::pdf` module.** Text extraction is
  upstream of fingerprinting and `pdf-extract` brings a heavy dep
  tree (`lopdf`, font parsing, deflate). Extract text yourself and
  feed it to `Canonicalizer::canonicalize`. Migration: drop the
  feature flag and call `pdf_extract::extract_text` directly, or any
  other PDF-to-text pipeline.
- **`cjk-japanese` and `cjk-korean` features and the
  `CjkSegmenter::Lindera` / `CjkSegmenter::LinderaKoDic` enum
  variants.** Embedded IPADIC and ko-dic dictionaries added 50 MiB
  and 150 MiB to the binary respectively, plus a build-time network
  dependency on Lindera.dev. Implement the `Tokenizer` trait against
  `lindera`, `vibrato`, or any other dedicated tokenizer for Japanese
  / Korean and feed it into any `Fingerprinter` directly.
- **`Error::Http` variant.** Was only constructed from the cloud
  providers; no kept feature path produces it.

### Changed (breaking)

- **`CjkSegmenter` is now `#[non_exhaustive]`** with `Jieba` as the
  only variant. The annotation lets future minor releases add
  language-specific segmenters without breaking downstream `match`
  arms.

### Removed (dependencies)

- `reqwest`, `serde_json` (optional), `tokio` — only used by the
  removed cloud providers.
- `lindera` — only used by the removed `cjk-japanese` /
  `cjk-korean` features.
- `pdf-extract` — only used by the removed `pdf` feature.

### Migration guide

| If you were using…                            | Replace with                                                              |
| --------------------------------------------- | ------------------------------------------------------------------------- |
| `txtfp::semantic::providers::OpenAiProvider`  | Implement `EmbeddingProvider` against `reqwest`/`ureq` directly.          |
| `txtfp::semantic::providers::VoyageProvider`  | Same as above.                                                            |
| `txtfp::semantic::providers::CohereProvider`  | Same as above.                                                            |
| `txtfp::pdf_to_text(bytes)`                   | `pdf_extract::extract_text_from_mem(bytes)` (add `pdf-extract` directly). |
| `CjkSegmenter::Lindera` (`cjk-japanese`)      | Implement `Tokenizer` over `lindera::tokenizer::Tokenizer` with IPADIC.   |
| `CjkSegmenter::LinderaKoDic` (`cjk-korean`)   | Same, with `ko-dic`.                                                      |
| `txtfp::Error::Http(_)`                       | Map upstream HTTP errors to your own error type before crossing API.     |

### Notes

- All v0.1.0+ golden byte fixtures still pass.
- `cargo-semver-checks` reports this release as a major bump (variant
  removals, feature removals).
- Default features (`std`, `minhash`, `simhash`, `lsh`) build cleanly
  on `wasm32-unknown-unknown`. The pruned dep tree shrinks the wasm
  build closure modestly.

## [0.2.3] - 2026-05-26

Performance and ergonomics patch release. **No breaking changes;
signature bytes unchanged from v0.2.2.** All v0.1.0+ golden fixtures
still pass.

### Fixed

- **`TlshFingerprint::new` length validator was wrong.** The validator
  required `hex.len() == 70`, but `tlsh2::Tlsh128_1::hash()` returns
  72 ASCII bytes (`"T1"` prefix + 70 hex digits). The validator never
  matched real TLSH output — `TlshFingerprinter::sketch_bytes`
  constructed `TlshFingerprint { hex }` directly and silently bypassed
  validation. Any caller that obtained a hex string from a sibling
  source (network, sidecar database) and ran it through `new()` would
  always have been rejected. Now correct: validates exactly 72 chars,
  `"T1"` prefix, hex digits in the body. `sketch_bytes` is routed
  through `new()` so any future `tlsh2` format drift surfaces at
  build time rather than at distance-comparison time.
- **`IdfTable` lookup was `O(log n)` instead of `O(1)`.** The internal
  map was `BTreeMap<String, f32>` despite `hashbrown` already being a
  dependency for the `simhash` feature. Swapped to
  `hashbrown::HashMap<String, f32>` with capacity pre-sized from
  `Iterator::size_hint`. `IdfTable: Clone + Debug + Default` still
  hold; the only observable change is `Debug`'s output (not
  semver-stable). No public API change.

### Performance

- **`LshIndex::query` no longer allocates a `HashSet` per call.**
  Replaced the `HashSet`-based dedup with `Vec::extend_from_slice` +
  `sort_unstable` + `dedup`. The contiguous Vec walk is cache-friendly
  and beats per-id hash-table inserts at the candidate counts LSH
  actually produces (≤ a few hundred for typical loads). Output is
  now documented as ascending order; previously documented as
  arbitrary, so this is a tightening, not a break.
- **`LshIndex::query_with_threshold` no longer allocates a second
  `Vec`.** Switched from `into_iter().filter().collect()` to in-place
  `Vec::retain`. Saves one allocation per query.
- **Streaming sketchers consolidated.** Extracted a shared
  `pub(crate) Utf8StreamBuffer` helper used by both `MinHashStreaming`
  and `SimHashStreaming`. Dedupes ~110 lines of UTF-8 carry/commit
  logic. The helper also `reserve`s capacity in `update` before
  `extend_from_slice`, eliminating a double-realloc on large chunks
  in steady-state streaming.
- **`ShingleTokenizer::for_each_token` no longer reallocates mid-shingle.**
  Buffer cap is now `(flat.len() + k).max(64)` — a safe upper bound
  for any single shingle, with an SSO-friendly floor for tiny inputs.
  Previous fixed `64`-byte cap forced a re-allocation when the running
  shingle held long technical words or k > ~5.
- **`WordTokenizer::tokens` and `for_each_token`** no longer carry a
  dead `filter(|s| !s.is_empty())`: `unicode-segmentation`'s
  `unicode_words()` never yields empty slices. Tiny inner-loop saving
  on the SimHash hot path; clearer code on the MinHash hot path.

### Added

- **`MinHashFingerprinter::into_streaming()`** and
  **`SimHashFingerprinter::into_streaming()`**. Idiomatic conversion
  from offline to streaming sketcher without requiring the caller to
  name `MinHashStreaming` / `SimHashStreaming` directly. The streamer
  inherits canonicalizer + tokenizer + seed + hash family + (for
  SimHash) weighting, with the default 16 MiB buffer cap. Override
  via `with_max_bytes`. Pure additive — no other API moves.

### Internal

- **`LshIndexBuilder::build` panic message** now reports the actual
  `bands * rows` value and points users at `try_build()` or
  `for_threshold()`. The doc comment additionally notes that builders
  produced by `for_threshold()` are guaranteed to satisfy
  `bands * rows == H` and so cannot trip the panic for that reason.
- **`Canonicalizer::strip_format` doc** now explicitly states that
  setting `strip_format = true` always strips Bidi controls, because
  the Cf category is a superset of Bidi controls. The behaviour was
  already this way (intentional, with a comment in `bidi::is_format`);
  the doc had not advertised the relationship.
- **`MinHashFingerprinter::with_hasher` doc** corrected: now says the
  default is `Xxh3_64` (since v0.2.0). The doc had stale text claiming
  MurmurHash3 was the default. Code was already correct.
- **`DEFAULT_SEED` hex spelling** unified to `0x00C0_FFEE_5EED` across
  `classical/hash.rs` and `classical/minhash/fingerprinter.rs`. The
  numeric value never changed.
- **`alloc::format` import in `src/fingerprint.rs`** is now gated on
  the same `cfg(any(feature = ...))` set as the only consumer
  (`Fingerprint::name()`), eliminating a dead-import warning under
  `--no-default-features`.
- **`TlshFingerprint.hex` field doc** updated to say "exactly 72 ASCII
  characters: the `"T1"` prefix followed by 70 hex digits".

## [0.2.2] - 2026-05-03

Hot-fix patch release. The cargo-fuzz harness shipped with v0.2.1
caught two `Canonicalizer::canonicalize` idempotence violations on
its very first CI run; this release fixes both. **No API changes.**

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

[Unreleased]: https://github.com/themankindproject/txtfp/compare/v0.3.0...HEAD
[Unreleased]: https://github.com/themankindproject/txtfp/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/themankindproject/txtfp/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/themankindproject/txtfp/compare/v0.2.3...v0.3.0
[0.2.3]: https://github.com/themankindproject/txtfp/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/themankindproject/txtfp/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/themankindproject/txtfp/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/themankindproject/txtfp/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/themankindproject/txtfp/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/themankindproject/txtfp/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/themankindproject/txtfp/releases/tag/v0.1.0
