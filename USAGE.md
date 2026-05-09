# txtfp Usage Guide

> Complete API reference and examples for high-performance text fingerprinting.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Pipeline Overview](#pipeline-overview)
- [Core API](#core-api)
  - [Canonicalization](#canonicalization)
    - [`Canonicalizer`](#canonicalizer)
    - [`CanonicalizerBuilder`](#canonicalizerbuilder)
    - [Confusable skeleton (security feature)](#confusable-skeleton-security-feature)
  - [Tokenizers](#tokenizers)
    - [`Tokenizer` trait](#tokenizer-trait)
    - [`WordTokenizer`](#wordtokenizer)
    - [`GraphemeTokenizer`](#graphemetokenizer)
    - [`ShingleTokenizer`](#shingletokenizer)
    - [`CjkTokenizer`](#cjktokenizer-cjk-feature)
  - [Classical fingerprinters](#classical-fingerprinters)
    - [`Fingerprinter` and `StreamingFingerprinter` traits](#fingerprinter-and-streamingfingerprinter-traits)
    - [MinHash](#minhash-minhash-feature)
    - [SimHash](#simhash-simhash-feature)
    - [LSH](#lsh-lsh-feature)
    - [TLSH](#tlsh-tlsh-feature)
  - [Unified `Fingerprint` enum](#unified-fingerprint-enum)
  - [Semantic embeddings](#semantic-embeddings-semantic-feature)
    - [`Embedding`](#embedding)
    - [`EmbeddingProvider` trait](#embeddingprovider-trait)
    - [`semantic_similarity`](#semantic_similarity)
    - [`LocalProvider` (ONNX)](#localprovider-onnx)
    - [Cloud providers](#cloud-providers)
    - [`Pooling`](#pooling)
    - [`ChunkingStrategy`](#chunkingstrategy)
- [Markup and PDF helpers](#markup-and-pdf-helpers)
- [Streaming](#streaming)
- [Serde](#serde)
- [Error handling](#error-handling)
- [Performance tips](#performance-tips)
- [Feature flags](#feature-flags)
- [Cross-SDK parity](#cross-sdk-parity)

---

## Quick Start

A copy-pasteable starting point: install the crate and run a near-duplicate detection over MinHash signatures. The example chains the four stages of the standard pipeline using sensible defaults. Produces two `MinHashSig<128>` signatures and prints their estimated Jaccard similarity.

Add to `Cargo.toml`:

```toml
[dependencies]
txtfp = "0.2"
```

> **Upgrading from 0.1.x?** v0.2.0 flips the default hash family from
> `MurmurHash3_x64_128` to `Xxh3_64` for both MinHash and SimHash —
> signature bytes change. Pin to `0.1` or pass
> `HashFamily::MurmurHash3_x64_128` explicitly for v0.1.x / Python
> `datasketch` byte parity. See [`HashFamily`](#tweaking-the-hash-family).
>
> **v0.2.1** is a patch release: bytes and API are identical to v0.2.0.
> Adds the cargo-fuzz harness sub-crate (delivers the v0.1.0 changelog
> promise), `cargo-semver-checks` CI, [`RELEASING.md`](RELEASING.md), a
> dedicated TLSH integration test, and fixes `--features tlsh` building
> alone (the cfg gate previously required `minhash`/`simhash`/`lsh` for
> the parent `classical` module to be declared).

The 30-second example — Jaccard near-duplicate detection over MinHash:

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

    println!("Jaccard: {:.2}", jaccard(&a, &b));
    Ok(())
}
```

---

## Pipeline Overview

A four-stage pipeline that turns a `&str` into a fixed-size fingerprint and a similarity score against another fingerprint. Each stage solves a different problem and has independent failure modes. Each stage is a trait (or set of traits) with multiple implementations. Guarantees byte-identical signatures for the same input under the same configuration.

Every fingerprint flows through four stages, each independently swappable:

```
input
  │
  ├── Canonicalizer       (Unicode normalization, casefold, Bidi/format strip)
  │
  ├── Tokenizer           (Word, Grapheme, Shingle, CJK)
  │
  ├── Fingerprinter       (MinHash, SimHash, TLSH, Embedding)
  │
  └── compare             (jaccard, hamming, cosine_estimate, semantic_similarity)
```

The same input always produces the same byte-identical signature when the same configuration is used.

---

## Core API

### Canonicalization

A configurable string-to-string transform that maps "visually or semantically equivalent" inputs to the same bytes. Stages: NFKC normalization → drop bidi/format codepoints → simple casefold → optional UTS #39 confusable skeleton. Configuration is captured in `config_string()` so two consumers can verify they're comparing apples to apples.

#### `Canonicalizer`

A stateless, thread-safe handle on a configured canonicalization pipeline. `Send + Sync`. Calling `Canonicalizer::default()` gives you NFKC + simple casefold + bidi-strip + format-strip.

```rust
pub struct Canonicalizer { /* opaque */ }
impl Default for Canonicalizer { /* … */ }
```

Stateless, `Send + Sync`. Constructed via `Canonicalizer::default()` or `CanonicalizerBuilder::default().build()`.

##### `canonicalize()`

The main entry point: takes `&str`, returns `String`. Runs the configured pipeline: Unicode normalization (NFKC by default), Bidi-control + Cf-category strip, simple Unicode casefold, optional confusable skeleton. Fast paths: pure ASCII uses `to_ascii_lowercase()`; ASCII + droppable format/bidi uses single-pass filter-and-lowercase.

```rust
pub fn canonicalize(&self, input: &str) -> String
```

Runs the configured pipeline:

1. Unicode normalization (NFKC by default).
2. Bidi-control + Cf-category strip (drops ZWJ, ZWSP, BOM, RLO, variation selectors, …).
3. Simple Unicode casefold.
4. Optional UTS #39 confusable skeleton (`security` feature).

**Fast paths** (default config only; both byte-stable with the slow path):

1. **Pure ASCII** — `to_ascii_lowercase()` in one pass. ~540 ns per 5 KB
   on 2024 hardware. NFC/NFKC, simple casefold, and the strip phases
   are all no-ops on ASCII, so the output is identical to the full pipeline.
2. **ASCII + droppable format/bidi codepoints** (v0.2.0+) — covers
   BOM-prefixed text (CSV with U+FEFF), ZWSP injection, RLO Trojan
   Source attacks, variation selectors on ASCII bases. Single-pass
   filter-and-lowercase. Measured 17× faster than the slow path on a
   5 KB lorem corpus with one BOM and a ZWSP every 80 bytes.

Anything that needs real Unicode work (non-ASCII letters, compat-form
decomposition, multi-char folds like `ß` → `ss`) falls through to the
full pipeline.

**Example:**

```rust
use txtfp::Canonicalizer;

let c = Canonicalizer::default();
assert_eq!(c.canonicalize("Hello\u{200B}World"), "helloworld");           // ZWSP stripped
assert_eq!(c.canonicalize("ＡＢＣ"), "abc");                               // NFKC + casefold
assert_eq!(c.canonicalize("admin\u{202E}drow"), "admindrow");              // Trojan Source
assert_eq!(c.canonicalize("ﬁle"), "file");                                // ligature fold
```

##### `config_string()`

A short, stable, human-readable identifier for the configuration (e.g. `"nfkc-cf-simple-bidi-fmt"`). Feed into `txtfp::config_hash` to get a 64-bit identifier suitable for indexing alongside signatures.

```rust
pub fn config_string(&self) -> String
```

Returns a stable identifier such as `"nfkc-cf-simple-bidi-fmt"`. Feed into `txtfp::config_hash` to disambiguate stored fingerprints.

#### `CanonicalizerBuilder`

A plain `pub`-fields struct for constructing a `Canonicalizer` with non-default stages. Fill the fields you care about, leave the rest at defaults via `..Default::default()`, and call `.build()`. Returns a `Canonicalizer` with the chosen stages wired up.

```rust
pub struct CanonicalizerBuilder {
    pub normalization:    Normalization,    // Nfc | Nfkc | None
    pub case_fold:        CaseFold,         // None | Simple
    pub strip_bidi:       bool,
    pub strip_format:     bool,
    pub apply_confusable: bool,             // requires `security` feature
}
```

**Example: a security-tightened canonicalizer:**

```rust
# #[cfg(feature = "security")]
# {
use txtfp::{CanonicalizerBuilder, CaseFold, Normalization};

let c = CanonicalizerBuilder {
    normalization:    Normalization::Nfkc,
    case_fold:        CaseFold::Simple,
    strip_bidi:       true,
    strip_format:     true,
    apply_confusable: true,                  // collapse Cyrillic/Latin homoglyphs
}.build();

assert_eq!(c.canonicalize("раураl"), c.canonicalize("paypal"));
# }
```

#### Confusable skeleton (`security` feature)

Maps visually similar codepoints to a common skeleton per UTS #39. Use it when usernames, domains, or filenames must be compared as humans see them — Cyrillic 'а' and Latin 'a' fold to the same character. Don't turn it on for full-text dedup — the skeleton is lossy.

```rust
# #[cfg(feature = "security")]
# {
use txtfp::{CanonicalizerBuilder};

let c = CanonicalizerBuilder { apply_confusable: true, ..Default::default() }.build();
assert_eq!(c.canonicalize("раураl"), c.canonicalize("paypal"));
# }
```

---

### Tokenizers

Turn a canonicalized `&str` into a stream of tokens (`&str` slices). A `Tokenizer` trait with a streaming `tokens()` method (returns `TokenStream<'a>`) and a callback-based `for_each_token()` for zero-allocation hot paths.

#### `Tokenizer` trait

The contract every tokenizer implements. `tokens()` returns a streaming iterator. `for_each_token()` is a default method overridden by perf-critical impls for zero allocation. `name()` returns a stable string baked into `FingerprintMetadata`.

```rust
pub trait Tokenizer: Send + Sync {
    fn tokens<'a>(&'a self, input: &'a str) -> TokenStream<'a>;
    fn name(&self) -> Cow<'static, str>;

    /// Zero-allocation hot path used by classical sketchers.
    fn for_each_token(&self, input: &str, f: &mut dyn FnMut(&str)) { /* default */ }
}
```

`name()` returns a stable identifier baked into `FingerprintMetadata`:

| Tokenizer                      | `name()`                           |
| ------------------------------ | ---------------------------------- |
| `WordTokenizer`                | `"word-uax29"`                     |
| `GraphemeTokenizer`            | `"grapheme-uax29"`                 |
| `ShingleTokenizer { k, inner }`| `"shingle-k=<k>/<inner>"`          |
| `CjkTokenizer` (jieba)         | `"cjk-jieba"` / `"cjk-jieba-hmm"`  |
| `CjkTokenizer` (lindera)       | `"cjk-lindera"` (v0.1.1+)          |

#### `WordTokenizer`

UAX #29 word-boundary segmenter. Filters out non-word segments (whitespace, punctuation). Zero-sized; `Copy`.

UAX #29 word boundaries. Filters out non-word segments (whitespace, punctuation). Zero-sized; `Copy`.

```rust
use txtfp::{Tokenizer, WordTokenizer};

let mut count = 0;
WordTokenizer.for_each_token("don't go!", &mut |tok| {
    println!("{tok}");
    count += 1;
});
assert!(count >= 2);  // "don't", "go"
```

#### `GraphemeTokenizer`

UAX #29 extended grapheme cluster segmenter. Family ZWJ sequences (`👨‍👩‍👧‍👦`) and flag pairs (`🇺🇸`) are single tokens. No whitespace filtering — every grapheme is yielded.

UAX #29 extended grapheme clusters. Family ZWJ sequences (`👨‍👩‍👧‍👦`) and flag pairs (`🇺🇸`) are single tokens.

```rust
use txtfp::{Tokenizer, GraphemeTokenizer};

let mut tokens = Vec::new();
GraphemeTokenizer.for_each_token("a\u{0301}🇺🇸", &mut |t| tokens.push(t.to_string()));
assert_eq!(tokens.len(), 2);
```

#### `ShingleTokenizer`

K-shingle adaptor over any inner `Tokenizer`. Joins k consecutive inner tokens with a single ASCII space. Production sweet spot: `k = 5` over `WordTokenizer` for English deduplication.

```rust
use txtfp::{ShingleTokenizer, Tokenizer, WordTokenizer};

let s = ShingleTokenizer { k: 3, inner: WordTokenizer };
let mut shingles = Vec::new();
s.for_each_token("the quick brown fox", &mut |t| shingles.push(t.to_string()));
assert_eq!(shingles, ["the quick brown", "quick brown fox"]);
```

The `for_each_token` impl uses a single re-used backing buffer and a range table — no per-shingle `String` allocation.

#### `CjkTokenizer` (`cjk` feature)

Chinese/Japanese/Korean segmenter using Jieba-rs (Simplified Chinese) or Lindera (Japanese/Korean). Dictionary loaded once via `OnceLock` on first use.

```rust
# #[cfg(feature = "cjk")]
# {
use txtfp::{CjkSegmenter, CjkTokenizer, Tokenizer};

let t = CjkTokenizer::new(CjkSegmenter::Jieba);
let mut tokens = Vec::new();
t.for_each_token("我爱北京天安门", &mut |s| tokens.push(s.to_string()));
assert!(tokens.contains(&"北京".to_string()));
# }
```

The Jieba dictionary is loaded once via `OnceLock` on first use. v0.1.0 ships Simplified Chinese only; Lindera (Japanese / Korean) lands in v0.1.1 once its transitive deps clear MSRV 1.85.

---

### Classical fingerprinters

Lossy compression algorithms that turn a token stream into a fixed-size sketch (bytes) preserving a specific similarity metric. Each algorithm is a struct generic over `Tokenizer`. Two operating modes: offline (`fingerprint(s)`) and streaming (`update(chunk)` / `finalize()`).

#### `Fingerprinter` and `StreamingFingerprinter` traits

Two traits implemented by every classical algorithm. `Fingerprinter::fingerprint` is one-shot. `StreamingFingerprinter` mirrors the `Digest`-style API (`update` / `finalize` / `reset`). Both traits have an associated `Output` type.

```rust
pub trait Fingerprinter {
    type Output;
    fn fingerprint(&self, input: &str) -> Result<Self::Output>;
}

pub trait StreamingFingerprinter {
    type Output;
    fn update(&mut self, chunk: &[u8]) -> Result<()>;
    fn finalize(self) -> Result<Self::Output>;
    fn reset(&mut self);
}
```

Every classical algorithm in `txtfp` implements both. `Fingerprinter` takes `&self` so a single instance is shared across worker threads.

---

#### MinHash (`minhash` feature)

A locality-sensitive sketch that estimates Jaccard similarity between two token sets. With `H = 128` slots, standard deviation ≤ 0.044. Pair with `LshIndex` for sub-linear retrieval at scale; pair with `jaccard()` for direct comparison.

##### `MinHashSig<const H: usize>`

`#[repr(C)]` struct with schema tag, padding, and `H` u64 hash slots. `bytemuck::Pod`. Total size: `8 + 8*H` bytes. Layout frozen since v0.1.0.

```rust
#[repr(C)]
pub struct MinHashSig<const H: usize> {
    pub schema: u16,
    pub _pad:   [u8; 6],   // zero
    pub hashes: [u64; H],  // little-endian on disk
}
```

`bytemuck::Pod`. Total size: `8 + 8*H` bytes. **Struct layout frozen since v0.1.0**; the slot *values* changed in v0.2.0 (default hasher flip).

##### `MinHashFingerprinter::new`

Constructs a fingerprinter with the canonicalizer and tokenizer of your choice. Defaults: `seed = 0x00C0_FFEE_5EED`, `HashFamily::Xxh3_64`.

```rust
pub fn new<T: Tokenizer>(canonicalizer: Canonicalizer, tokenizer: T) -> Self
```

Defaults (v0.2.0+): `seed = 0x00C0_FFEE_5EED`, `HashFamily::Xxh3_64`.
For v0.1.x bytes / Python `datasketch` parity, opt back into MurmurHash3
explicitly via `.with_hasher(HashFamily::MurmurHash3_x64_128)`.

##### `fingerprint`

The one-shot entry point. Canonicalizes, tokenizes, and folds each token's hash into the slot array. Returns `Error::InvalidInput("empty document")` for empty or whitespace-only input.

```rust
fn fingerprint(&self, input: &str) -> Result<MinHashSig<H>>
```

Empty input or all-whitespace input returns `Error::InvalidInput("empty document")` — never returns a degenerate `[u64::MAX; H]`.

##### `jaccard`

The pairwise comparator: counts agreeing slots and divides by `H`. Returns `f32` in `[0.0, 1.0]`. Standard deviation is `sqrt(p(1-p)/H)` — for `H = 128` and `p = 0.5`, ±0.044.

```rust
pub fn jaccard<const H: usize>(a: &MinHashSig<H>, b: &MinHashSig<H>) -> f32
```

Returns the fraction of slots that agree. Bounded `[0.0, 1.0]`. Estimator standard deviation is `sqrt(p(1-p)/H)` — for `H = 128` and `p = 0.5`, ±0.044.

##### Tweaking the hash family

Swap the underlying hash function via `.with_hasher(HashFamily::...)` and `.with_seed(u64)`. Both are builder-style and return `Self`. Changing either invalidates wire compatibility with existing signatures.

```rust
use txtfp::{Canonicalizer, HashFamily, MinHashFingerprinter, ShingleTokenizer, WordTokenizer};

// v0.2.0 default is Xxh3_64. Opt back into MurmurHash3 for datasketch
// parity:
let fp = MinHashFingerprinter::<_, 128>::new(
    Canonicalizer::default(),
    ShingleTokenizer { k: 5, inner: WordTokenizer },
)
.with_hasher(HashFamily::MurmurHash3_x64_128)
.with_seed(0xDEAD_BEEF);
```

`HashFamily::MurmurHash3_x64_128` matches datasketch / Python-MinHash byte-for-byte but is the slower path. `HashFamily::Xxh3_64` (default in v0.2.0+) is faster on AArch64 and modern x86_64; both halves of the double-hashing trick come from a single `xxh3_128` call internally.

##### Streaming MinHash

Wrapper around `MinHashFingerprinter` that accepts byte chunks. Buffers UTF-8-validated bytes (capped at 16 MiB) and runs the offline algorithm at `finalize`.

```rust
use txtfp::{
    Canonicalizer, MinHashFingerprinter, MinHashStreaming,
    ShingleTokenizer, StreamingFingerprinter, WordTokenizer,
};

let inner = MinHashFingerprinter::<_, 128>::new(
    Canonicalizer::default(),
    ShingleTokenizer { k: 3, inner: WordTokenizer },
);
let mut s = MinHashStreaming::new(inner);

s.update(b"the quick brown fox").unwrap();
s.update(b" jumps over the lazy dog").unwrap();
let sig = s.finalize().unwrap();
```

The streaming sketcher buffers bytes (UTF-8-validated, capped at 16 MiB) and runs the offline algorithm at `finalize`. True online positional MinHash (no buffering) is queued for a later release.

---

#### SimHash (`simhash` feature)

A 64-bit locality-sensitive sketch that estimates cosine similarity between weighted token bags. Compare with `hamming(a, b)` (number of differing bits) or `cosine_estimate(a, b)` (maps to [-1, 1]).

##### `SimHash64`

`#[repr(transparent)]` newtype wrapping `u64`. `bytemuck::Pod`. 8 bytes, little-endian on disk. Layout frozen since v0.1.0.

```rust
#[repr(transparent)]
pub struct SimHash64(pub u64);
```

`bytemuck::Pod`. 8 bytes, little-endian on disk. **Struct layout frozen since v0.1.0**; the bit values changed in v0.2.0 (default hasher flip).

##### `SimHashFingerprinter::new`

Constructor analogous to `MinHashFingerprinter::new`. Defaults: `Weighting::Tf`, `HashFamily::Xxh3_64`. Under `Weighting::Tf`, streams `±1` per token occurrence directly into the accumulator.

```rust
pub fn new<T: Tokenizer>(canonicalizer: Canonicalizer, tokenizer: T) -> Self
```

Defaults (v0.2.0+): `Weighting::Tf`, `HashFamily::Xxh3_64`. Pass
`HashFamily::MurmurHash3_x64_128` explicitly for v0.1.x byte parity.

Under `Weighting::Tf`, sketching streams `±1` per token occurrence
straight into the 64-slot accumulator with no per-token counts map —
the dominant cost in v0.1.x. `Uniform` and `IdfWeighted` retain a
dedup pass (the weights aren't linear in occurrence count).

##### `Weighting`

How much each token contributes to the accumulator:
- `Uniform`: every distinct token weight = 1
- `Tf`: weight = term frequency
- `IdfWeighted`: weight = TF × IDF (caller supplies the table)

```rust
pub enum Weighting {
    Uniform,                  // every distinct token weight = 1
    Tf,                       // weight = term frequency
    IdfWeighted(IdfTable),    // weight = TF × IDF (caller supplies the table)
}
```

##### `hamming` and `cosine_estimate`

- `hamming(a, b)`: `(a.0 ^ b.0).count_ones()` — POPCNT on x86_64, `cnt` on AArch64. Returns `0..=64`.
- `cosine_estimate(a, b)`: `cos((distance / 64) * π)` per Charikar 2002. Returns `[-1.0, 1.0]`.

```rust
pub fn hamming(a: SimHash64, b: SimHash64) -> u32              // 0..=64
pub fn cosine_estimate(a: SimHash64, b: SimHash64) -> f32      // [-1.0, 1.0]
```

`hamming` lowers to hardware POPCNT on x86_64 and `cnt` on AArch64 — effectively free.

`cosine_estimate(a, b) = cos((distance / 64) * π)` per Charikar 2002.

**Example with custom IDF:**

```rust
use txtfp::{Canonicalizer, IdfTable, SimHashFingerprinter, Weighting, WordTokenizer};

let table = IdfTable::from_pairs([("the", 0.1_f32), ("dog", 4.0_f32)]);
let fp = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer)
    .with_weighting(Weighting::IdfWeighted(table));
```

---

#### LSH (`lsh` feature)

Banded locality-sensitive hash index over `MinHashSig<H>`. Maps a probe signature to a small candidate set of likely-similar IDs in near-constant time. Collapses near-duplicate retrieval from O(N) to nearly constant time per query.

##### `LshIndexBuilder`

Builder for choosing `(bands, rows)` and constructing an `LshIndex`. `new(b, r)` constructs directly; `for_threshold(t, H)` numerically integrates the false-positive and false-negative rates and picks the optimal factorization.

```rust
pub struct LshIndexBuilder { pub bands: usize, pub rows: usize }

impl LshIndexBuilder {
    pub fn new(bands: usize, rows: usize) -> Self;
    pub fn for_threshold(threshold: f32, h: usize) -> Result<Self>;
    pub fn build<const H: usize>(self) -> LshIndex<H>;
    pub fn try_build<const H: usize>(self) -> Result<LshIndex<H>>;
}
```

`for_threshold` numerically integrates the false-positive and false-negative rates and picks the (bands, rows) factorization of `H` that minimizes their sum at `threshold`.

##### `LshIndex<const H: usize>`

The retrieval data structure. Internally a `Vec` of `b` band tables (`HashMap<u64, SmallVec<u64>>`) plus an id-keyed reverse map (`HashMap<u64, MinHashSig<H>>`). Band tables use identity hasher for xxh3_64 keys.

```rust
impl<const H: usize> LshIndex<H> {
    pub fn with_bands_rows(bands: usize, rows: usize) -> Result<Self>;
    pub fn insert(&mut self, id: u64, sig: MinHashSig<H>);
    pub fn remove(&mut self, id: u64) -> Option<MinHashSig<H>>;
    pub fn get(&self, id: u64) -> Option<&MinHashSig<H>>;
    pub fn query(&self, sig: &MinHashSig<H>) -> Vec<u64>;
    pub fn query_with_threshold(&self, sig: &MinHashSig<H>, threshold: f32) -> Vec<u64>;
    pub fn len(&self) -> usize;

    // parallel feature only:
    pub fn extend_par<I: IntoIterator<Item = (u64, MinHashSig<H>)>>(&mut self, items: I);
}
```

`query` returns hash-bucket candidates (deduplicated). `query_with_threshold` re-checks each candidate's actual Jaccard and prunes — use for precision-tuned retrieval.

Internally the band-key tables use an identity hasher (the keys are
already 64-bit `xxh3_64` digests, so re-hashing them through `ahash`
is pure overhead). The id-keyed reverse map keeps the default ahash
hasher because caller-supplied ids may be sequential.

**Example:**

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

// Recall-tuned: 64 bands × 2 rows triggers on Jaccard ≥ ~0.5
let mut idx: LshIndex<128> = LshIndex::with_bands_rows(64, 2)?;

for (id, doc) in [
    (1, "the quick brown fox jumps over the lazy dog at noon"),
    (2, "the quick brown fox jumps over the lazy dog at dusk"),
    (3, "astronomers detect cosmic background radiation"),
] {
    idx.insert(id, fp.fingerprint(doc)?);
}

let probe = fp.fingerprint("the quick brown fox jumps over the lazy dog at dawn")?;
let near = idx.query_with_threshold(&probe, 0.5);
println!("near-duplicates: {near:?}");                  // [1, 2]
# Ok(()) }
```

##### Choosing bands and rows

> **What.** A reference table for picking `(b, r)` when `H = 128`.
>
> **Why.** The four factorizations cover the practical range: exact-dedup (`8 × 16`), strict near-dup (`16 × 8`), moderate fuzzy (`32 × 4`), high-recall (`64 × 2`). Each entry's "sweet-spot threshold" is the Jaccard at which the factorization's S-curve crosses 0.5.
>
> **How.** Each row is the median empirical break-even from synthetic experiments. They're guidelines — your corpus's distribution may push them.
>
> **Does.** Use as a quick lookup; prefer `LshIndexBuilder::for_threshold(t, 128)` for principled choice.

For `H = 128`:

| `(bands, rows)` | Sweet spot threshold | Use case                              |
| --------------- | -------------------- | ------------------------------------- |
| `(8,  16)`      | 0.95                 | Exact deduplication only              |
| `(16, 8)`       | 0.85                 | Strict near-duplicates                |
| `(32, 4)`       | 0.65                 | Moderate fuzzy match                  |
| `(64, 2)`       | 0.45                 | High recall, will produce candidates  |

Always prefer `LshIndexBuilder::for_threshold(t, 128)` over hand-tuning unless you have measurements.

##### Thread safety

> **What.** Read-only access is `Send + Sync`; writes need exclusive access.
>
> **Why.** A typical workload is "build once, query many" — the index is loaded at startup and read concurrently. Insert and remove take `&mut self` because the band tables are non-trivial to update lock-free, and most callers don't need that complexity inline.
>
> **How.** Wrap in `RwLock` (writes rare) or `Mutex` (writes common). The `parallel` feature provides `extend_par` for the bulk-insert case without external locking.
>
> **Does.** `query` and `query_with_threshold` are `&self`, so multiple threads can probe in parallel.

`LshIndex` is `Send + Sync` for read-only access but `insert` / `remove` take `&mut self`. Wrap in `RwLock` / `Mutex` for shared writes — concurrency primitives live in `ucfp`, not here.

##### Parallel bulk insert (`parallel` feature)

> **What.** A rayon-powered bulk insert API.
>
> **Why.** Inserting `N` signatures serially takes `N × b` hash-table ops. A naïve parallel insert would contend on every band table. `extend_par` shards work so each rayon worker owns one band table for the call — contention-free, and roughly linear in core count up to the point bandwidth dominates.
>
> **How.** Splits the input across rayon threads, fans each `(id, sig)` into the relevant band, and updates the band table whose worker owns it. The id-keyed reverse map is updated separately (small, cheap).
>
> **Does.** Measured 1.74× speedup on 8 cores for a 10K-doc bench. Restricted to fresh ids — `debug_assert!`s on duplicates and on pre-existing ids. For mixed insert/replace traffic, keep using `insert` in a serial loop.

```rust
# #[cfg(all(feature = "lsh", feature = "parallel"))]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::{
    Canonicalizer, Fingerprinter, LshIndex,
    MinHashFingerprinter, ShingleTokenizer, WordTokenizer,
};

let canon = Canonicalizer::default();
let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
let fp = MinHashFingerprinter::<_, 128>::new(canon, tok);

let docs = ["alpha beta gamma", "delta epsilon zeta", "eta theta iota"];
let pairs: Vec<_> = docs
    .iter()
    .enumerate()
    .map(|(i, d)| Ok((i as u64, fp.fingerprint(d)?)))
    .collect::<Result<_, txtfp::Error>>()?;

let mut idx = LshIndex::<128>::with_bands_rows(16, 8)?;
idx.extend_par(pairs);                          // sharded by band, contention-free
assert_eq!(idx.len(), 3);
# Ok(()) }
```

`extend_par` shards work by band across the rayon thread pool: each
worker owns one band's hash table for the call. Measured **1.74×**
speedup on 8 cores for the 10K-doc bench (20.2 ms → 11.6 ms).
Restricted to fresh ids — `debug_assert!`s on duplicates and on
pre-existing ids. For mixed insert/replace traffic, keep using
`insert` in a serial loop.

---

#### TLSH (`tlsh` feature)

> **What.** Trend Micro Locality Sensitive Hash — a byte-level fuzzy hash for malware/binary similarity, also useful for log-line and short-document comparison.
>
> **Why.** TLSH is content-aware in a different way than MinHash/SimHash. It captures statistical features of the byte stream (q-gram histograms quantized to a body and a small header). It's the right pick when you want similarity over arbitrary byte content, not over tokenized text.
>
> **How.** Wraps the canonical TLSH algorithm with a `Canonicalizer` front end. Sketches over canonicalized bytes; lower distance means more similar.
>
> **Does.** Pair with `tlsh_distance(a, b)` for comparison. Treat `< 50` as "high similarity" for the 128/1 variant. Requires ≥ 50 bytes of input after canonicalization.

##### `TlshFingerprinter`

> **What.** The TLSH fingerprinter type.
>
> **Why.** Constructor takes a `Canonicalizer` so byte-level hashing happens after Unicode normalization — otherwise a U+0041 vs U+FF21 would be a complete miss.
>
> **How.** `sketch_bytes(&[u8])` skips the canonicalizer for raw-bytes input (e.g. binaries). `fingerprint(&str)` runs canonicalization first.
>
> **Does.** Returns `TlshFingerprint`. Below the minimum-byte threshold the call returns `Error::InvalidInput`.

```rust
pub struct TlshFingerprinter { /* opaque, holds Canonicalizer */ }

impl TlshFingerprinter {
    pub fn new(canonicalizer: Canonicalizer) -> Self;
    pub fn sketch_bytes(&self, bytes: &[u8]) -> Result<TlshFingerprint>;
}
```

##### `tlsh_distance`

The pairwise comparator. Lower scores mean more similar. `< 50` is "high similarity" for the 128/1 variant.

```rust
pub fn tlsh_distance(a: &TlshFingerprint, b: &TlshFingerprint) -> Result<i32>
```

Lower scores mean more similar. Treat `< 50` as "high similarity" for the 128/1 variant.

**Example:**

```rust
# #[cfg(feature = "tlsh")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::{Canonicalizer, Fingerprinter, TlshFingerprinter, tlsh_distance};

let fp = TlshFingerprinter::new(Canonicalizer::default());
let a = fp.fingerprint("the quick brown fox jumps over the lazy dog at noon today
the slow grey wolf creeps under the loud ravens at dusk
astronomers detect cosmic background radiation")?;
let b = fp.fingerprint("the quick brown fox jumps over the lazy dog at dusk today
the slow grey wolf creeps under the loud ravens at dawn
astronomers detect cosmic background radiation")?;

println!("TLSH distance: {}", tlsh_distance(&a, &b)?);     // small = similar
# Ok(()) }
```

TLSH requires ≥ 50 bytes of input (after canonicalization).

---

### Unified `Fingerprint` enum

A single enum that holds any of the supported fingerprint variants. Each variant is feature-gated. `name()` and `metadata()` provide cross-cutting accessors.

```rust
pub enum Fingerprint {
    #[cfg(feature = "minhash")]   MinHash(MinHashSig<128>),
    #[cfg(feature = "simhash")]   SimHash(SimHash64),
    #[cfg(feature = "tlsh")]      Tlsh(TlshFingerprint),
    #[cfg(feature = "semantic")]  Embedding(Embedding),
}
```

Used by the cross-modal `ucfp` integrator: one column in a database holds any signature variant while preserving variant-aware similarity routing.

##### `metadata()`

Returns variant-only metadata: algorithm tag, schema version, byte size, model id. Always succeeds.

```rust
pub fn metadata(&self) -> FingerprintMetadata
```

Returns `FingerprintMetadata { algorithm, config_hash: UNCOMPUTED_CONFIG_HASH, model_id, schema_version, byte_size }`. The `config_hash` is set to the `UNCOMPUTED_CONFIG_HASH` sentinel because the enum doesn't know the canonicalizer / tokenizer.

##### `metadata_with()

Producer-side metadata: takes the producing canonicalizer + tokenizer name + algorithm config string and computes the full metadata including `config_hash`.

```rust
pub fn metadata_with(
    &self,
    canonicalizer: &Canonicalizer,
    tokenizer_name: &str,
    algo_cfg: &str,
) -> FingerprintMetadata
```

Producer-side path: populates `config_hash` from the supplied triple. Recommended whenever the producing context is in scope.

##### `name()`

A short, stable display name. Frozen since v0.1.0 — safe to use as part of a persistent storage key.

Stable display name, frozen since v0.1.0:

| Variant     | `name()` format                                 |
| ----------- | ----------------------------------------------- |
| `MinHash`   | `"minhash-h128-v{schema}"`                      |
| `SimHash`   | `"simhash-b64-v{schema}"`                       |
| `Tlsh`      | `"tlsh-v1"`                                     |
| `Embedding` | `"embedding/{model_id}-v1"` or `"embedding-v1"` |

If you need a fully-qualified key including config disambiguation:

```rust
# #[cfg(feature = "minhash")]
# {
use txtfp::{Canonicalizer, Fingerprint, MinHashSig, config_hash};

let fp = Fingerprint::MinHash(MinHashSig::<128>::empty());
let cfg = config_hash(&Canonicalizer::default(), "word-uax29", "h128-mmh3");
let key = format!("{}-cfg={cfg:016x}", fp.name());
# }
```

##### Bulk persist a column of MinHash signatures

Zero-copy persistence pattern: `bytemuck::cast_slice(&sigs)` reinterprets `&[MinHashSig<H>]` as `&[u8]` with no copy.

```rust
# #[cfg(feature = "minhash")]
# {
use txtfp::MinHashSig;

let sigs: Vec<MinHashSig<128>> = (0..1000).map(|_| MinHashSig::<128>::empty()).collect();
let bytes: &[u8] = bytemuck::cast_slice(&sigs);             // zero-copy
assert_eq!(bytes.len(), sigs.len() * 1032);                  // 8 + 128*8 per sig
# }
```

---

### Semantic embeddings (`semantic` feature)

Dense-vector representations of text produced by neural models. Classical fingerprints capture *surface* similarity; semantic embeddings capture *meaning*. A provider trait abstracts the model source: local ONNX, OpenAI / Voyage / Cohere (cloud APIs).

#### `Embedding`

A dense vector (`Vec<f32>`) plus an optional `model_id` tag. `new` rejects empty vectors and non-finite values (NaN, ±Inf).

```rust
pub struct Embedding {
    pub vector:   Vec<f32>,
    pub model_id: Option<String>,
}
```

##### Constructors

Validating constructors: `new` returns `Err` on empty or non-finite. `with_model` adds a model id. `normalize` mutates in place to L2-norm 1.

```rust
impl Embedding {
    pub fn new(vector: Vec<f32>) -> Result<Self>;
    pub fn with_model(vector: Vec<f32>, model_id: Option<String>) -> Result<Self>;
    pub fn dim(&self) -> usize;
    pub fn l2_norm(&self) -> f32;
    pub fn normalize(&mut self);
    pub fn dot(&self, other: &Embedding) -> Result<f32>;
}
```

`new` rejects empty vectors and non-finite values (NaN, ±Inf) at construction time.

#### `EmbeddingProvider` trait

The contract every provider implements. Associated `Input` type lets a provider take `str`, `Path`, or anything else. `Send + Sync` — share one across worker threads.

```rust
pub trait EmbeddingProvider: Send + Sync {
    type Input: ?Sized;
    fn embed(&self, input: &Self::Input) -> Result<Embedding>;
    fn model_id(&self) -> &str;
    fn dimension(&self) -> usize;
}
```

The trait shape is **parity-compatible with `imgfprint`** — see [Cross-SDK parity](#cross-sdk-parity).

#### `semantic_similarity`

Cosine similarity between two embeddings. Returns `f32` in `[-1.0, 1.0]`. Refuses to compare:
- Embeddings whose `model_id`s differ → `Error::ModelMismatch`.
- Embeddings of different dimensions → `Error::DimensionMismatch`.
- Embeddings with zero L2 norm → `Error::InvalidInput`.

```rust
pub fn semantic_similarity(a: &Embedding, b: &Embedding) -> Result<f32>
```

Cosine similarity in `[-1.0, 1.0]`. Refuses to compare:

- Embeddings whose `model_id`s differ → `Error::ModelMismatch`.
- Embeddings of different dimensions → `Error::DimensionMismatch`.
- Embeddings with zero L2 norm → `Error::InvalidInput`.

#### `LocalProvider` (ONNX)

A provider that runs ONNX-format embedding models locally via `ort` 2.0. No per-call network latency, no rate limits, no spend per token. `from_pretrained(model_id)` fetches from Hugging Face Hub. `embed_query` and `embed_document` apply per-model prefix tables.

```rust
impl LocalProvider {
    pub fn from_pretrained(model_id: &str) -> Result<Self>;
    pub fn from_onnx(onnx_path: &Path, tokenizer_path: &Path, pooling: Pooling) -> Result<Self>;
    pub fn builder() -> LocalProviderBuilder;

    pub fn embed_document(&self, input: &str) -> Result<Embedding>;
    pub fn embed_query(&self, input: &str) -> Result<Embedding>;
}
```

Loads ONNX models from the Hugging Face Hub via `hf-hub`, tokenizes with `tokenizers`, and runs `ort` 2.0. Cheap to clone (`Arc` under the hood). All inference is serialized behind an internal mutex.

`from_pretrained` consults a per-model pooling table (`Pooling::Cls` for BGE, `Pooling::Mean` for E5/MiniLM/Nomic, …) and a query/document prefix table (`bge-*` prepends `"Represent this sentence for searching relevant passages: "` to queries; `e5-*` uses `"query: "` / `"passage: "`).

**Example:**

```rust,no_run
# #[cfg(feature = "semantic")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::{EmbeddingProvider, LocalProvider, semantic_similarity};

let p = LocalProvider::from_pretrained("BAAI/bge-small-en-v1.5")?;

let q = p.embed_query("a fluffy cat")?;
let d = p.embed_document("a small fluffy feline named Whiskers")?;

let s = semantic_similarity(&q, &d)?;
println!("similarity: {s:.3}");
# Ok(()) }
```

Builder for self-hosted ONNX:

```rust,no_run
# #[cfg(feature = "semantic")]
# fn demo() -> Result<(), txtfp::Error> {
use std::path::Path;
use txtfp::{LocalProvider, Pooling};

let p = LocalProvider::builder()
    .model_id("acme/in-house-embedder-v3")
    .onnx_path("/srv/models/embedder.onnx")
    .tokenizer_path("/srv/models/tokenizer.json")
    .pooling(Pooling::Cls)
    .max_seq_len(512)
    .intra_threads(8)
    .build()?;
# Ok(()) }
```

#### Cloud providers

HTTP-based providers: `OpenAiProvider`, `VoyageProvider`, `CohereProvider`. Best-in-class quality without managing model files; per-call cost. Each provider takes an API key; `with_model` overrides the default model.

##### `OpenAiProvider`

OpenAI Embeddings API client. Default to `text-embedding-3-small` (1536 dims) for retrieval; `text-embedding-3-large` (3072 dims) for top quality.

```rust
# #[cfg(feature = "openai")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::semantic::providers::OpenAiProvider;
use txtfp::EmbeddingProvider;

let p = OpenAiProvider::new(std::env::var("OPENAI_API_KEY").unwrap())?
    .with_model("text-embedding-3-small");

let e = p.embed("the quick brown fox")?;
assert_eq!(e.dim(), 1536);

// Batch:
let many = p.embed_batch(&["fox", "wolf", "lion"])?;
# Ok(()) }
```

##### `VoyageProvider`

Voyage AI Embeddings API client. Retrieval-tuned models (`voyage-3`, `voyage-large-2`) outperform OpenAI on out-of-domain retrieval. The `input_type` parameter (`"document"` / `"query"`) is required for asymmetric retrieval.

```rust
# #[cfg(feature = "voyage")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::semantic::providers::VoyageProvider;

let p = VoyageProvider::new(std::env::var("VOYAGE_API_KEY").unwrap())?;
let docs = p.embed_batch(&["lorem", "ipsum"], Some("document"))?;
# Ok(()) }
```

##### `CohereProvider`

Cohere Embed v3 API client. Strong multilingual quality with `input_type` taxonomy (`"search_document"`, `"search_query"`, `"classification"`, `"clustering"`).

```rust
# #[cfg(feature = "cohere")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::semantic::providers::CohereProvider;

let p = CohereProvider::new(std::env::var("COHERE_API_KEY").unwrap())?;
let docs = p.embed_batch(&["lorem", "ipsum"], "search_document")?;
# Ok(()) }
```

##### Retry / `Retry-After` / backoff

> **What.** A unified retry policy across all cloud providers.
>
> **Why.** Cloud APIs throw transient 429s and 5xx under load. A common policy means you don't hand-roll backoff per provider, and the wall-clock cap prevents a slow request from holding a worker indefinitely.
>
> **How.** Exponential backoff with jitter; honors `Retry-After` on 429s; bails out immediately on permanent errors (400, 401, 403, 404, 422).
>
> **Does.** Bubbles up the original error after the wall-clock cap or on a permanent failure. `Debug` redacts the API key, so log lines won't leak credentials.

All cloud providers retry transient failures (HTTP 408, 425, 429, 500, 502, 503, 504, network errors) with exponential backoff:

- Initial: 500 ms
- Multiplier: 2.0
- Jitter: ±30%
- Per-attempt cap: 30 s
- Total wall-clock cap: 90 s
- HTTP 429 with `Retry-After` honors the header (capped at 60 s)

Permanent failures (400, 401, 403, 404, 422) bubble up immediately.

`Debug` on each provider redacts the API key:

```text
OpenAiProvider { model: "text-embedding-3-small", base_url: "...", api_key: "<redacted>" }
```

#### `Pooling`

> **What.** How to reduce a sequence of token-level hidden states to a single sentence-level vector.
>
> **Why.** Different models are trained with different pooling — using the wrong one silently degrades quality (often by 5–15 points on retrieval benchmarks). BGE wants `Cls`; E5/MiniLM/Nomic want `Mean`; some want unnormalized variants.
>
> **How.** `Cls` takes the first token's hidden state. `Mean` averages over the attention-mask-respecting token slice. `MeanNoNorm` is mean without the L2 normalization step. `Max` is rare in modern models but kept for completeness.
>
> **Does.** `Pooling::apply(hidden, hidden_dim, attention_mask)` is exposed for callers building custom inference paths outside `LocalProvider`.

```rust
pub enum Pooling {
    Cls,           // BGE, Snowflake Arctic, mxbai
    Mean,          // E5, MiniLM, GTE, Nomic
    MeanNoNorm,    // mean without L2 normalization
    Max,
}
```

`Pooling::apply(hidden, hidden_dim, attention_mask)` is exposed for callers building custom inference paths.

#### `ChunkingStrategy`

> **What.** A configuration for splitting long inputs into model-sized chunks.
>
> **Why.** Embedding models have hard input limits (typically 512 or 8192 tokens). Naïvely truncating loses information; chunking-and-pooling preserves it. Different content types want different chunking — fixed windows for logs, sentence-bounded for prose, recursive for mixed Markdown.
>
> **How.** `FixedTokens` does greedy sliding windows with overlap. `SentenceBounded` (default) packs whole sentences up to `max_tokens`. `Recursive` splits by paragraph, falling back to sentence then word as needed.
>
> **Does.** `chunk_for_model(input, &strategy)` returns `Vec<String>`. Token count is approximated as `words × 1.3` when no model tokenizer is available — adjust `max_tokens` if your text or model differs significantly from English BPE.

```rust
pub struct ChunkingStrategy {
    pub max_tokens: usize,
    pub overlap:    usize,
    pub mode:       ChunkMode,
}

pub enum ChunkMode {
    FixedTokens,      // greedy fixed windows with overlap
    SentenceBounded,  // pack whole sentences up to max_tokens (default)
    Recursive,        // paragraph → sentence → word fallback
}

pub fn chunk_for_model(input: &str, strategy: &ChunkingStrategy) -> Vec<String>;
```

**Example:**

```rust
# #[cfg(feature = "semantic")]
# {
use txtfp::{ChunkMode, ChunkingStrategy, chunk_for_model};

let s = ChunkingStrategy {
    max_tokens: 256,
    overlap:    32,
    mode:       ChunkMode::Recursive,
};
let chunks = chunk_for_model("Para one.\n\nPara two.\n\nPara three.", &s);
# }
```

Token count is approximated as `words × 1.3` when no model tokenizer is available — this is the BPE-token-per-word ratio for English. Adjust `max_tokens` if your text or model differs.

---

## Markup and PDF helpers

Convenience converters that turn HTML, Markdown, or PDF bytes into plain text. HTML uses streaming SAX walker that drops script/style. Markdown uses `pulldown-cmark`. PDF uses `pdf-extract` on a worker thread with 30s timeout and 50MiB size cap.

```rust
# #[cfg(feature = "markup")]
# {
use txtfp::{html_to_text, markdown_to_text, MarkdownOptions, markdown_to_text_with};

let plain  = html_to_text("<p>hello</p><script>alert(1)</script>")?;
assert!(plain.contains("hello") && !plain.contains("alert"));

let md = markdown_to_text("# Heading\n\nBody with `inline code`")?;
assert!(md.contains("Heading"));

let opts = MarkdownOptions { include_code_blocks: false, ..Default::default() };
let no_code = markdown_to_text_with("```\nlet x = 1;\n```\nsurrounding", opts)?;
assert!(!no_code.contains("let x"));
# Ok::<_, txtfp::Error>(())
# }
```

```rust,no_run
# #[cfg(feature = "pdf")]
# {
use txtfp::{pdf_to_text, pdf_to_text_with, PdfOptions};

let bytes = std::fs::read("doc.pdf")?;
let text = pdf_to_text(&bytes)?;                                // 50 MiB cap, 30 s timeout

// Tighter ingest:
let opts = PdfOptions { max_bytes: 5 * 1024 * 1024, timeout_secs: 10 };
let text2 = pdf_to_text_with(&bytes, opts)?;
# Ok::<_, txtfp::Error>(())
# }
```

`pdf_to_text` runs the parser on a worker thread with a wall-clock timeout — hostile or pathologically-structured PDFs cannot hang an ingestion pipeline. NUL bytes in extracted text are replaced with U+FFFD.

---

## Streaming

A chunk-fed alternative to one-shot `fingerprint(s)`. Both MinHash and SimHash ship streaming variants. Internally buffer UTF-8-validated bytes (capped at 16 MiB) and run the offline algorithm at `finalize`.

Both MinHash and SimHash ship streaming variants. The streamer accumulates UTF-8 bytes (capped at 16 MiB by default) and runs the offline algorithm at `finalize`:

```rust
use txtfp::{
    Canonicalizer, MinHashFingerprinter, MinHashStreaming,
    ShingleTokenizer, StreamingFingerprinter, WordTokenizer,
};

let inner = MinHashFingerprinter::<_, 128>::new(
    Canonicalizer::default(),
    ShingleTokenizer { k: 3, inner: WordTokenizer },
);
let mut s = MinHashStreaming::new(inner).with_max_bytes(64 * 1024 * 1024);

for chunk in std::fs::read("doc.txt")?.chunks(8192) {
    s.update(chunk)?;
}
let sig = s.finalize()?;
# Ok::<_, txtfp::Error>(())
```

The streamer correctly handles UTF-8 sequences that span chunk boundaries: incomplete trailing bytes are carried into the next `update` call. A trailing incomplete sequence at `finalize` time returns `Error::InvalidInput`.

True online positional MinHash (no buffering) is queued for a later release.

---

## Serde

`Serialize` / `Deserialize` impls for public signature types, gated behind the `serde` feature. `MinHashSig<H>` uses hand-rolled impls so const-generic arrays round-trip through every serde format.

With the `serde` feature:

```rust
# #[cfg(feature = "serde")]
# {
use txtfp::MinHashSig;

let s: MinHashSig<128> = MinHashSig::empty();
let json = serde_json::to_string(&s)?;
let back: MinHashSig<128> = serde_json::from_str(&json)?;
assert_eq!(s, back);
# Ok::<_, serde_json::Error>(())
# }
```

`MinHashSig<H>` uses hand-rolled `Serialize` / `Deserialize` impls so const-generic arrays round-trip through every serde format (JSON struct with array, bincode tight tuple, etc.). Length validation is enforced on deserialize: a JSON `hashes` array of the wrong length is rejected.

`SimHash64` derives serde via `#[serde(transparent)]` over `u64`. `Embedding` is a regular derive.

---

## Error handling

A single `txtfp::Error` enum returned from every fallible API. `#[non_exhaustive]` — new variants can be added without breaking semver.

`txtfp` returns `Result<T, txtfp::Error>` from every fallible API. The error type is `#[non_exhaustive]`:

```rust
pub enum Error {
    InvalidInput(String),
    ModelMismatch     { a: String, b: String },
    DimensionMismatch { a: usize,  b: usize  },
    Config(String),
    Io(std::io::Error),                            // std feature
    Tokenizer(String),                             // semantic feature
    Onnx(String),                                  // semantic feature
    Http(String),                                  // openai/voyage/cohere
    EmptyEmbedding,                                // semantic feature
    SchemaMismatch    { expected: u16, actual: u16 },
    FeatureDisabled(&'static str),
    // ...
}
```

Match exhaustively only inside the crate; downstream code should use a wildcard arm.

**Common errors at a glance:**

| Where                                  | What                                           |
| -------------------------------------- | ---------------------------------------------- |
| `MinHashFingerprinter::fingerprint("")`| `InvalidInput("empty document")`               |
| `LshIndex::with_bands_rows(7, 9)` (H=128) | `Config("bands * rows must equal H")`       |
| `semantic_similarity` of two zero vectors | `InvalidInput("cannot compute cosine ...")` |
| Cloud provider HTTP 401                | `Http("OpenAI returned 401")`                  |
| `pdf_to_text` exceeds 30 s             | `InvalidInput("pdf parse exceeded 30-second timeout")` |

---

## Performance tips

Practical throughput knobs in roughly the order they pay off:

1. `RUSTFLAGS="-C target-cpu=native"` — native ISA codegen
2. Enable `[profile.release] lto = "thin"` or `"fat"` — 5-15% gain
3. Use mimalloc for high-throughput LSH — halves insert latency
4. Reuse the fingerprinter — share one instance across worker threads
5. Pre-canonicalize once for batch jobs — feed same canonical text to multiple algorithms
6. Pick `H` by variance, not throughput — `H=64` halves variance
7. Tune LSH for your actual threshold — `for_threshold(t, 128)`
8. Use `for_each_token` over `tokens()` — skips per-token String allocation
9. ASCII inputs hit the canonicalizer fast path (~540 ns per 5 KB)
10. For bulk LSH insert, use `extend_par` with `parallel` feature — 1.74× on 8 cores

1. **Compile with `RUSTFLAGS="-C target-cpu=native"`.** The MurmurHash3 inner loop and the H-derive loop both benefit from native ISA codegen.
2. **Enable `[profile.release]` LTO.** `lto = "thin"` gains ~5–15% on the classical sketchers. `lto = "fat"` (used in the bench profile) gains another 3–8%.
3. **Use mimalloc for high-throughput LSH.** Insert is alloc-heavy (small `SmallVec` band candidate lists). mimalloc roughly halves insert latency.
4. **Reuse the fingerprinter.** Constructing a `MinHashFingerprinter` allocates the `Canonicalizer` and `Tokenizer`; share one instance across worker threads (`&self` API).
5. **Pre-canonicalize once for batch jobs.** If you'll fingerprint the same document with multiple algorithms, call `Canonicalizer::canonicalize` once and feed the result through each fingerprinter manually (the algorithms only re-tokenize, not re-canonicalize).
6. **Pick `H` by variance, not throughput.** Going from `H = 128` to `H = 64` cuts ~20% off MinHash time but doubles the estimator's standard deviation. For corpora where Jaccard estimates feed downstream LSH, use `H = 128`.
7. **Tune LSH for the threshold you actually care about.** `LshIndexBuilder::for_threshold(t, 128)` minimizes total error around `t`. Hand-picking `bands=16, rows=8` will under-recall a Jaccard-0.4 corpus.
8. **`for_each_token` over `tokens()` in custom kernels.** The callback path skips per-token `String` allocation.
9. **ASCII inputs hit the canonicalizer fast path.** It's effectively free (~540 ns per 5 KB on 2024 hardware). The v0.2.0 fast path also covers ASCII text containing only droppable bidi/format chars (BOM-prefixed CSV, ZWSP-injected text) — 17× faster than running the full Unicode pipeline on those.
10. **For bulk LSH insert, use `extend_par` with the `parallel` feature.** Sharded per-band, contention-free; measured 1.74× on 8 cores. Requires fresh ids (no replacement).

---

## Feature flags

Cargo features that gate optional functionality. Heavyweight dependencies only land when their feature is on.

| Feature      | Default | Description                                                  |
| ------------ | :-----: | ------------------------------------------------------------ |
| `std`        |   ✅    | libstd. Without it, `no_std + alloc`.                        |
| `minhash`    |   ✅    | MinHash sketcher.                                            |
| `simhash`    |   ✅    | SimHash sketcher.                                            |
| `lsh`        |         | Banded LSH index over MinHash signatures.                    |
| `markup`     |         | `html_to_text`, `markdown_to_text`.                          |
| `pdf`        |         | `pdf_to_text` with timeout.                                  |
| `cjk`        |         | `CjkTokenizer` (jieba).                                      |
| `tlsh`       |         | `TlshFingerprinter` + `tlsh_distance`.                       |
| `security`   |         | UTS #39 confusable skeleton.                                 |
| `serde`      |         | `Serialize` / `Deserialize` (incl. const-generic MinHash).   |
| `parallel`   |         | Rayon-powered batch helpers (e.g. `LshIndex::extend_par`).   |
| `semantic`   |         | `LocalProvider` via `ort` + Hugging Face Hub.                |
| `openai`     |         | `OpenAiProvider`.                                            |
| `voyage`     |         | `VoyageProvider`.                                            |
| `cohere`     |         | `CohereProvider`.                                            |

---

## Cross-SDK parity

Types and constants kept identical across `audiofp`, `imgfprint`, and `txtfp` sibling crates. The cross-modal integrator `ucfp` depends on this parity.

`txtfp` is one of three sibling crates under the `themankindproject` umbrella:

- [`audiofp`](https://crates.io/crates/audiofp) — audio fingerprinting
- `imgfprint` — image fingerprinting
- **`txtfp`** (this crate) — text fingerprinting

The cross-modal integrator `ucfp` consumes all three. The contract that holds across them:

| Surface                 | Stability                                                       |
| ----------------------- | --------------------------------------------------------------- |
| `EmbeddingProvider`     | Same trait shape, same method signatures.                       |
| `Embedding`             | Same field layout (`vector: Vec<f32>`, `model_id: Option<String>`). |
| `semantic_similarity`   | Same error semantics (model id mismatch, dim mismatch).         |
| `FORMAT_VERSION: u32`   | Equal across all three crates within a release line.            |

```rust,ignore
assert_eq!(audiofp::FORMAT_VERSION, txtfp::FORMAT_VERSION);
assert_eq!(imgfprint::FORMAT_VERSION, txtfp::FORMAT_VERSION);
```

If you're vendoring all three, test the parity assertion in your integration suite — the integrator depends on it.
