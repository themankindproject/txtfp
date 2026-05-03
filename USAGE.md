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

> **What.** A copy-pasteable starting point: install the crate and run a near-duplicate detection over MinHash signatures.
>
> **Why.** New readers want to see signal in 30 seconds — a working pipeline they can paste into a fresh `cargo new` and run. Everything else in this doc unpacks the pieces they touched here.
>
> **How.** The example chains the four stages of the standard pipeline (canonicalize → tokenize → fingerprint → compare) using sensible defaults. `ShingleTokenizer { k: 5, inner: WordTokenizer }` is the production-tested choice for English deduplication; `H = 128` is the default signature width.
>
> **Does.** Produces two `MinHashSig<128>` signatures and prints their estimated Jaccard similarity.

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

> **What.** A four-stage pipeline that turns a `&str` into a fixed-size fingerprint and a similarity score against another fingerprint.
>
> **Why.** Each stage solves a different problem and has independent failure modes. Canonicalization decides what counts as "the same character"; tokenization decides what counts as "the same word"; fingerprinting compresses the token bag into a fixed-size sketch; comparison turns two sketches into a number. Splitting them lets you swap any stage without rewriting the others — e.g. switch from `WordTokenizer` to `CjkTokenizer` for Chinese text without touching the MinHash code.
>
> **How.** Each stage is a trait (or set of traits) with multiple implementations. The fingerprinter holds a canonicalizer and a tokenizer by value, so calling `fp.fingerprint(s)` runs all four stages in order. There is no shared mutable state — `&self` everywhere — so one fingerprinter is shared across worker threads.
>
> **Does.** Guarantees byte-identical signatures for the same input under the same configuration. This determinism is what lets you index signatures from one process and query them from another.

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

> **What.** The first stage: a configurable string-to-string transform that maps "visually or semantically equivalent" inputs to the same bytes.
>
> **Why.** Without canonicalization, `"Hello"` and `"hello"` would produce different fingerprints; so would `"ﬁle"` (U+FB01) and `"file"`, or `"Hello\u{200B}World"` (with a zero-width space) and `"HelloWorld"`. Attackers exploit this — a Trojan Source attack uses RLO/LRO bidi controls to make malicious code look benign. Canonicalization neutralizes these gaps before the rest of the pipeline sees the text.
>
> **How.** A staged Unicode pipeline: NFKC normalization → drop bidi/format codepoints → simple casefold → optional UTS #39 confusable skeleton. Each stage can be turned off independently via the builder.
>
> **Does.** Produces a deterministic `String` that downstream tokenizers and fingerprinters will hash. Configuration is captured in `config_string()` so two consumers can verify they're comparing apples to apples.

#### `Canonicalizer`

> **What.** A stateless, thread-safe handle on a configured canonicalization pipeline.
>
> **Why.** Configuration is fixed at construction time so the hot path is just a function call — no `Mutex`, no per-call config decoding. `Send + Sync` means one instance is shared across worker threads.
>
> **How.** Internally an opaque struct holding `Normalization`, `CaseFold`, two strip flags, and (with `security`) a confusable mapper. Methods take `&self` and `&str` and allocate a single output `String`.
>
> **Does.** Calling `Canonicalizer::default()` gives you NFKC + simple casefold + bidi-strip + format-strip, which is the right setting for ~95% of de-dup workloads.

```rust
pub struct Canonicalizer { /* opaque */ }
impl Default for Canonicalizer { /* … */ }
```

Stateless, `Send + Sync`. Constructed via `Canonicalizer::default()` or `CanonicalizerBuilder::default().build()`.

##### `canonicalize()`

> **What.** The main entry point: takes `&str`, returns `String`.
>
> **Why.** Two reasons it's worth knowing what's inside: (1) you may need to debug why two "equal" strings produced different fingerprints — read the stages below to localize which one didn't fire; (2) for ASCII corpora the function is effectively free thanks to fast paths, which matters for throughput planning.
>
> **How.** The fast paths short-circuit for inputs where the slow path would be a no-op. Pure-ASCII bypasses Unicode tables entirely. ASCII-plus-droppable-format-chars (the most common attack-resistant case) does a single filter-and-lowercase pass instead of running NFKC.
>
> **Does.** Always produces output byte-identical to running the full pipeline — fast paths are never lossy. If you observe a divergence, file a bug.

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

> **What.** A short, stable, human-readable identifier for the configuration.
>
> **Why.** When you persist fingerprints to disk and read them back later, you need to know the configuration that produced them. Two corpora canonicalized with different settings cannot be meaningfully compared — fingerprints look like fingerprints either way, but Jaccard scores will be silently wrong. The config string lets a downstream system reject mismatched signatures up front.
>
> **How.** Concatenates the active stage tags (e.g. `"nfkc-cf-simple-bidi-fmt"`) in a fixed order. Pass it through `txtfp::config_hash` to get a 64-bit identifier suitable for indexing alongside signatures.
>
> **Does.** Returns `String`. Stable across versions for the same configuration.

```rust
pub fn config_string(&self) -> String
```

Returns a stable identifier such as `"nfkc-cf-simple-bidi-fmt"`. Feed into `txtfp::config_hash` to disambiguate stored fingerprints.

#### `CanonicalizerBuilder`

> **What.** A plain `pub`-fields struct for constructing a `Canonicalizer` with non-default stages.
>
> **Why.** The defaults work for general English/multilingual de-dup, but you may need to turn off NFKC (preserving e.g. width distinctions) or turn on the confusable mapper (defeating Cyrillic/Latin homoglyph attacks on usernames). Public fields make the builder feel like a config struct, not a fluent API.
>
> **How.** Fill the fields you care about, leave the rest at defaults via `..Default::default()`, and call `.build()`. There's no validation — every combination is valid.
>
> **Does.** Returns a `Canonicalizer` with the chosen stages wired up. The result is the same shape as `Canonicalizer::default()` — fully thread-safe, opaque.

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

> **What.** An optional final stage that maps visually similar codepoints to a common skeleton per UTS #39.
>
> **Why.** Identity systems care about what humans see, not what bytes they typed. Cyrillic 'а' (U+0430) and Latin 'a' (U+0061) are pixel-identical in nearly every font; an attacker registers `раураl.com` and the user can't tell. Confusable folding collapses both to the same skeleton so a duplicate-username check rejects the lookalike.
>
> **How.** Loads the UTS #39 confusables table (compiled in) and substitutes each input codepoint with its prototype. Runs after casefolding so case-confusables (e.g. Greek capital iota vs. Latin uppercase I) also collapse.
>
> **Does.** Best for usernames, domain names, and filename comparison. **Don't** turn it on for full-text dedup of natural-language documents — the skeleton is lossy and reduces signal on legitimate non-Latin scripts.

UTS #39 maps visually similar codepoints to a common skeleton. Use it when usernames, domains, or filenames must be compared as humans see them — Cyrillic 'а' and Latin 'a' fold to the same character.

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

> **What.** The second stage: turn a canonicalized `&str` into a stream of tokens (`&str` slices).
>
> **Why.** "What is a duplicate" depends on what you call a unit. Word-level tokenization makes "the quick brown fox" and "quick the brown fox" look identical (bag-of-words); shingle-level makes them different (preserves order). Grapheme-level matters for emoji and combining marks; CJK needs a real segmenter because there are no spaces. The tokenizer is where you encode the unit choice.
>
> **How.** A `Tokenizer` trait with a streaming `tokens()` method (returns `TokenStream<'a>`) and a callback-based `for_each_token()` for zero-allocation hot paths. Implementations are zero-sized (`WordTokenizer`, `GraphemeTokenizer`) or small structs (`ShingleTokenizer`, `CjkTokenizer`).
>
> **Does.** Each call iterates the input once and yields token slices borrowed from the input. The callback path skips intermediate allocations and is the one classical fingerprinters use.

#### `Tokenizer` trait

> **What.** The contract every tokenizer implements.
>
> **Why.** A trait — rather than a concrete type — lets `MinHashFingerprinter`, `SimHashFingerprinter`, etc. accept any tokenizer with one generic parameter. The `name()` method exists so signature metadata can identify the producing tokenizer (essential for cross-process comparison).
>
> **How.** `tokens()` returns a streaming iterator (good for ad-hoc use). `for_each_token()` is a default method overridden by perf-critical impls — it gets a `&mut dyn FnMut(&str)` so the inner loop allocates nothing.
>
> **Does.** All built-in tokenizers are `Send + Sync`. `name()` returns a stable string baked into the on-disk metadata format.

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

> **What.** UAX #29 word-boundary segmenter.
>
> **Why.** "Word" is the right unit for almost all natural-language de-dup. UAX #29 is the standardized algorithm — it handles apostrophes (`don't` is one token), hyphenated forms, and Latin-script diacritics correctly without a custom regex.
>
> **How.** Wraps the `unicode-segmentation` crate's word iterator and filters out non-word segments (whitespace, punctuation). Zero-sized type implementing `Copy`, so passing it by value is free.
>
> **Does.** Yields word tokens only — punctuation and whitespace are dropped. Use this as `inner` for `ShingleTokenizer` when you want word-shingles.

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

> **What.** UAX #29 extended grapheme cluster segmenter.
>
> **Why.** A "user-perceived character" is rarely a single codepoint. The flag emoji `🇺🇸` is two regional-indicator codepoints; the family `👨‍👩‍👧‍👦` is seven codepoints joined by ZWJ; `é` can be one codepoint or two (e + combining acute). Grapheme tokens are the right unit when you care about visual identity — useful for emoji-heavy social text and identifier comparison.
>
> **How.** Same `unicode-segmentation` backend as `WordTokenizer`, but iterates extended grapheme clusters.
>
> **Does.** A complex emoji is a single token. A combining-mark sequence is a single token. There is no whitespace filtering — every grapheme is yielded.

UAX #29 extended grapheme clusters. Family ZWJ sequences (`👨‍👩‍👧‍👦`) and flag pairs (`🇺🇸`) are single tokens.

```rust
use txtfp::{Tokenizer, GraphemeTokenizer};

let mut tokens = Vec::new();
GraphemeTokenizer.for_each_token("a\u{0301}🇺🇸", &mut |t| tokens.push(t.to_string()));
assert_eq!(tokens.len(), 2);
```

#### `ShingleTokenizer`

> **What.** A k-shingle adaptor that wraps any inner `Tokenizer`.
>
> **Why.** Word-level bag-of-words throws away order; full-text comparison throws away resilience to small edits. K-shingles are the middle ground — overlapping windows of `k` consecutive tokens. Two documents that share most of their k-shingles are near-duplicates regardless of where the matching runs start. `k = 5` over `WordTokenizer` is the textbook choice for English near-dup detection.
>
> **How.** Buffers the last `k` inner-token byte ranges and emits each window as a single `&str` joined by ASCII spaces. The implementation reuses one backing `String` and a small range table — no per-shingle allocation in the hot path.
>
> **Does.** Output token count is `inner_count - k + 1` (zero if the input has fewer than `k` inner tokens). The joined form (single space separator) is stable and is what gets hashed downstream.

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

> **What.** A Chinese/Japanese/Korean segmenter, gated behind the `cjk` feature.
>
> **Why.** CJK scripts have no whitespace between words. UAX #29 falls back to per-codepoint tokens, which makes shingle-based de-dup useless (every signature looks similar). A statistical or dictionary-based segmenter is required.
>
> **How.** Wraps `jieba-rs` (Simplified Chinese, optionally with HMM unknown-word recovery). The Jieba dictionary is loaded once via `OnceLock` on first use, so the first call pays a few ms and the rest are fast. Lindera (Japanese/Korean) is queued for v0.1.1.
>
> **Does.** Yields word-level tokens for CJK input. For mixed-language text, runs Jieba over CJK runs and falls back cleanly elsewhere.

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

> **What.** Lossy compression algorithms that turn a token stream into a fixed-size sketch (bytes) preserving a specific similarity metric.
>
> **Why.** A 1 KB document and a 10 MB document both reduce to e.g. 1032 bytes (MinHash H=128) or 8 bytes (SimHash). At scale, comparing two sketches is O(H) regardless of original size — and pairs of sketches estimate the original similarity to a known variance. This is what makes web-scale de-dup tractable.
>
> **How.** Each algorithm is a struct generic over `Tokenizer`. The struct holds a `Canonicalizer` + tokenizer + algorithm parameters. `&self` everywhere — fingerprinters are share-across-threads handles.
>
> **Does.** Two operating modes per algorithm: offline (`fingerprint(s)`) for in-memory documents, and streaming (`update(chunk)` / `finalize()`) for inputs you can't hold whole.

#### `Fingerprinter` and `StreamingFingerprinter` traits

> **What.** Two traits implemented by every classical algorithm.
>
> **Why.** A common shape lets generic code (e.g. `ucfp`'s pipeline orchestrator) accept any classical fingerprinter without algorithm-specific glue. `&self` on `Fingerprinter` is deliberate — it forces implementors to keep no per-call mutable state, which is what makes one fingerprinter shareable across threads.
>
> **How.** `Fingerprinter::fingerprint` is one-shot. `StreamingFingerprinter` mirrors the `Digest`-style API (`update` / `finalize` / `reset`) for chunked input.
>
> **Does.** Both traits have an associated `Output` type — different per algorithm. Errors flow through the crate's `Result<T>` alias.

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

> **What.** A locality-sensitive sketch that estimates Jaccard similarity (set-overlap fraction) between two token sets.
>
> **Why.** Jaccard is the right metric for "do these documents share content," and MinHash is the canonical sketch for it. With `H = 128` slots, Jaccard estimates have standard deviation ≤ 0.044 — accurate enough that a 0.05 threshold meaningfully rejects unrelated pairs. The sketch is a fixed 1032 bytes regardless of document size.
>
> **How.** For each of `H` independent hash functions, keep the minimum hash value seen across the token stream. Two documents' MinHash signatures agree on slot `i` with probability equal to the Jaccard of their token sets — so the fraction of agreeing slots is an unbiased estimator of Jaccard.
>
> **Does.** Pair with `LshIndex` for sub-linear retrieval at scale; pair with `jaccard()` for direct comparison.

##### `MinHashSig<const H: usize>`

> **What.** The on-the-wire signature: a `Pod` struct with a schema tag, padding, and `H` u64 hash slots.
>
> **Why.** `bytemuck::Pod` plus little-endian on-disk layout means a column of signatures is `bytemuck::cast_slice(&sigs)` — zero-copy bulk persist. The schema field lets a future format change be detected at load time.
>
> **How.** `#[repr(C)]` with explicit padding so the layout is fixed across compilers. Total size: `8 + 8*H` bytes (1032 for H=128).
>
> **Does.** The struct layout is frozen since v0.1.0; the slot *values* changed in v0.2.0 because the default hash family flipped from MurmurHash3 to xxh3. If you have v0.1.x signatures on disk, either keep using v0.1 or rebuild with `HashFamily::MurmurHash3_x64_128`.

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

> **What.** Constructs a fingerprinter with the canonicalizer and tokenizer of your choice.
>
> **Why.** The signature width `H` is a const generic, so the compiler unrolls the inner loop and inlines the slot count. Constructor-time choice of canonicalizer/tokenizer matches the principle that one instance is shared across threads.
>
> **How.** Both arguments are stored by value. The fingerprinter then runs canon → tokens → hash → min-update for each call to `fingerprint`. Hash family and seed default to the safe production picks; both are tunable via builder methods.
>
> **Does.** Returns `MinHashFingerprinter<T, H>`. The default seed (`0x00C0_FFEE_5EED`) and family (`Xxh3_64` from v0.2.0) reproduce the wire format published by this crate at this version.

```rust
pub fn new<T: Tokenizer>(canonicalizer: Canonicalizer, tokenizer: T) -> Self
```

Defaults (v0.2.0+): `seed = 0x00C0_FFEE_5EED`, `HashFamily::Xxh3_64`.
For v0.1.x bytes / Python `datasketch` parity, opt back into MurmurHash3
explicitly via `.with_hasher(HashFamily::MurmurHash3_x64_128)`.

##### `fingerprint`

> **What.** The one-shot entry point.
>
> **Why.** Returning `Result` (rather than swallowing edge cases) catches the empty-document case explicitly: an all-whitespace input would otherwise produce `[u64::MAX; H]`, which is a valid signature shape but a nonsensical one — every comparison against it would estimate Jaccard 1.0 regardless of the other operand.
>
> **How.** Canonicalizes, tokenizes via `for_each_token`, and folds each token's hash into the slot array. Skips per-token allocation; reuses the slot array as the only mutable state.
>
> **Does.** `O(n)` in input bytes plus the per-token hashing cost. Throughput on a 5 KB ASCII document is in the millions of fingerprints per second per core.

```rust
fn fingerprint(&self, input: &str) -> Result<MinHashSig<H>>
```

Empty input or all-whitespace input returns `Error::InvalidInput("empty document")` — never returns a degenerate `[u64::MAX; H]`.

##### `jaccard`

> **What.** The pairwise comparator.
>
> **Why.** This is the whole point of MinHash — given two signatures, return an estimate of their token-set Jaccard. It's an unbiased estimator with known variance, so you can pick `H` to bound your error.
>
> **How.** Counts agreeing slots and divides by `H`. That's it; no Unicode work, no allocation.
>
> **Does.** Returns `f32` in `[0.0, 1.0]`. Standard deviation of the estimate is `sqrt(p(1-p)/H)` — for `H = 128` and true Jaccard 0.5, ±0.044.

```rust
pub fn jaccard<const H: usize>(a: &MinHashSig<H>, b: &MinHashSig<H>) -> f32
```

Returns the fraction of slots that agree. Bounded `[0.0, 1.0]`. Estimator standard deviation is `sqrt(p(1-p)/H)` — for `H = 128` and `p = 0.5`, ±0.044.

##### Tweaking the hash family

> **What.** Swap the underlying hash function used for slot updates.
>
> **Why.** Two reasons: (1) byte-for-byte parity with another implementation (Python `datasketch`, the v0.1.x txtfp wire format); (2) raw throughput — xxh3 is faster on AArch64 and modern x86_64 because both halves of the double-hashing trick come from a single `xxh3_128` call.
>
> **How.** `.with_hasher(HashFamily::...)` replaces the default; `.with_seed(u64)` overrides the seed. Both are builder-style and return `Self`.
>
> **Does.** Changing either invalidates wire compatibility with existing signatures. If you have a populated index, plan a re-fingerprint pass before flipping.

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

> **What.** A wrapper around `MinHashFingerprinter` that accepts byte chunks.
>
> **Why.** When the input is a 100 MB log file or an HTTP body of unknown length, you don't want to read it whole into memory just to hash it. Streaming lets you feed bounded chunks.
>
> **How.** The current implementation buffers UTF-8-validated bytes (capped at 16 MiB) and runs the offline algorithm at `finalize`. UTF-8 sequences split across chunk boundaries are stitched correctly. True online positional MinHash (no buffering) is queued for a later release.
>
> **Does.** Same output type and semantics as offline `fingerprint`, but with `update`/`finalize` shape. Trailing incomplete UTF-8 at finalize time is an error — don't drop the last byte of a 4-byte codepoint.

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

> **What.** A 64-bit locality-sensitive sketch that estimates cosine similarity between weighted token bags.
>
> **Why.** SimHash is dramatically smaller than MinHash (8 bytes vs 1032), making it the right pick when storage or RAM bandwidth dominates — billions of documents, embedded targets, or wide-fanout retrieval where you want to keep the whole corpus in L3. It also approximates *cosine* similarity, which is the right metric when token weights matter (TF-IDF flavored de-dup, plagiarism detection).
>
> **How.** For each token, hash it, then for each of 64 bits add or subtract the token's weight from the corresponding accumulator slot. Sign of each accumulator becomes the corresponding output bit.
>
> **Does.** Compare with `hamming(a, b)` (number of differing bits) or `cosine_estimate(a, b)` (analytic mapping back to cosine). Both are essentially free thanks to hardware popcount.

##### `SimHash64`

> **What.** A `#[repr(transparent)]` newtype wrapping a `u64`.
>
> **Why.** The transparent wrapping means a `Vec<SimHash64>` has the same layout as a `Vec<u64>` — zero-copy persistence and arithmetic-friendly. `Pod` for `bytemuck`.
>
> **How.** Layout has been frozen since v0.1.0; bit values flipped in v0.2.0 because the default hash family flipped.
>
> **Does.** 8 bytes, little-endian on disk. Use `bytemuck::cast_slice` for bulk persist.

```rust
#[repr(transparent)]
pub struct SimHash64(pub u64);
```

`bytemuck::Pod`. 8 bytes, little-endian on disk. **Struct layout frozen since v0.1.0**; the bit values changed in v0.2.0 (default hasher flip).

##### `SimHashFingerprinter::new`

> **What.** Constructor analogous to `MinHashFingerprinter::new`.
>
> **Why.** Same rationale as MinHash — fix the configuration up front, share `&self` across threads. SimHash also has a `Weighting` knob that MinHash doesn't.
>
> **How.** Stores canonicalizer, tokenizer, weighting, hash family, and seed. Hot path is the 64-slot accumulator update.
>
> **Does.** v0.2.0+ defaults: `Weighting::Tf` and `HashFamily::Xxh3_64`. The TF weighting streams `±1` per occurrence directly into the accumulator, skipping the per-document counts map that dominated v0.1.x time.

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

> **What.** How much each token contributes to the accumulator.
>
> **Why.** Picking a weighting changes what the sketch is similar to. `Uniform` ignores frequency — every distinct token is one vote, good for short-text de-dup. `Tf` upweights frequent tokens — good when repetition signals topicality. `IdfWeighted` downweights stopwords using a caller-supplied IDF table — the standard choice when you have corpus statistics.
>
> **How.** `Uniform` and `IdfWeighted` need to know whether each token is repeated, so they take a dedup pass. `Tf` is linear in occurrence count, so the streamed `±1` accumulator update suffices.
>
> **Does.** Behavior is independent of `H` (always 64-bit). `IdfWeighted` is the only variant that needs an external table.

```rust
pub enum Weighting {
    Uniform,                  // every distinct token weight = 1
    Tf,                       // weight = term frequency
    IdfWeighted(IdfTable),    // weight = TF × IDF (caller supplies the table)
}
```

##### `hamming` and `cosine_estimate`

> **What.** Two comparators over `SimHash64` pairs.
>
> **Why.** `hamming` is the raw distance — useful for thresholding and for radix-tree style indexing (e.g. by leading-bit prefix). `cosine_estimate` maps that distance to the angle space you originally cared about, per the Charikar 2002 mapping.
>
> **How.** `hamming` is `(a.0 ^ b.0).count_ones()` — POPCNT on x86_64, `cnt` on AArch64. `cosine_estimate(a, b) = cos((distance / 64) * π)`.
>
> **Does.** `hamming` returns `0..=64`, `cosine_estimate` returns `[-1.0, 1.0]`. Both are deterministic functions of the bit values; no allocation.

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

> **What.** A banded locality-sensitive hash index over `MinHashSig<H>`. Maps a probe signature to a small candidate set of likely-similar IDs in near-constant time.
>
> **Why.** Comparing a probe against `N` stored signatures is `O(N)`. With a billion documents that's a non-starter. Banding partitions each signature into `b` bands of `r` slots each (`b·r = H`) and hashes each band to a bucket — two signatures collide in *some* band with probability that rises sharply around the chosen Jaccard threshold. The probe only re-checks the documents it shares a band-bucket with, which is typically a tiny fraction of the corpus.
>
> **How.** `LshIndex<H>` holds `b` band-keyed `HashMap`s plus an id-keyed reverse map. `insert` adds the signature; `query` collects bucket members and deduplicates; `query_with_threshold` adds an exact Jaccard re-check to prune false positives.
>
> **Does.** Use `LshIndexBuilder::for_threshold(t, H)` to pick `(b, r)` for the threshold you actually care about. Hand-tuning is the second-best option.

Banded LSH over MinHash signatures. Collapses near-duplicate retrieval from O(N) to nearly constant time per query.

##### `LshIndexBuilder`

> **What.** A small builder for choosing `(bands, rows)` and constructing an `LshIndex`.
>
> **Why.** The `(b, r)` choice is the LSH performance/accuracy knob. `for_threshold` does the math — it numerically integrates the false-positive and false-negative rates for each valid factorization of `H` and picks the minimizer at the supplied threshold. Hand-tuning works once you have measurements; `for_threshold` is the right starting point.
>
> **How.** `new(b, r)` constructs the builder directly; `for_threshold(t, H)` solves for it. `build` panics on invalid configurations (`b * r != H`); `try_build` returns `Result`.
>
> **Does.** Returns `LshIndex<H>` with the chosen banding wired up.

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

> **What.** The retrieval data structure itself.
>
> **Why.** Provides insert/remove/get/query primitives; `query_with_threshold` adds the exact-Jaccard verification pass for precision-sensitive callers. The const-generic `H` matches the signature width so the compiler proves at type-check time that you can't insert a 64-slot signature into a 128-slot index.
>
> **How.** Internally a `Vec` of `b` band tables (`HashMap<u64, SmallVec<u64>>`) plus an id-keyed reverse map (`HashMap<u64, MinHashSig<H>>`). The band tables use an *identity hasher* — keys are already xxh3_64 digests, so re-hashing is pure overhead; the reverse map keeps ahash because caller IDs may be sequential.
>
> **Does.** `query` returns hash-bucket candidates (deduplicated). `query_with_threshold` re-checks each candidate's actual Jaccard and filters — use this for precision. With the `parallel` feature, `extend_par` does sharded bulk insert.

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

> **What.** The pairwise comparator.
>
> **Why.** TLSH outputs a non-similarity number — bigger is more different. The distance is calibrated so that small values correlate with "humans would call these similar."
>
> **How.** Computes the canonical TLSH distance: header diff plus body Hamming-style diff over the quantized q-gram histogram.
>
> **Does.** `< 50` is a reasonable "high similarity" cutoff for the default 128/1 variant. Treat as ordinal — comparing distances across different inputs is fine, but the absolute value isn't a fraction.

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

> **What.** A single enum that holds any of the supported fingerprint variants.
>
> **Why.** The cross-modal integrator `ucfp` consumes audio, image, and text fingerprints from the same column of a database. A type-erased holder lets one storage layout serve every variant; pattern-matching on the variant dispatches comparison correctly. Without this, callers would need a parallel column or a hand-rolled tagged union.
>
> **How.** Each variant is feature-gated, so the enum is small in minimal builds and grows with enabled features. `name()` and `metadata()` provide the cross-cutting accessors.
>
> **Does.** Wrap a typed signature in `Fingerprint::MinHash(sig)` (etc.) when handing it to a generic store. Unwrap on the read side.

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

> **What.** Returns variant-only metadata: algorithm tag, schema version, byte size, model id (if any).
>
> **Why.** This is the consumer-side path — given a `Fingerprint` you found in storage, you want to know what it is. The enum doesn't carry the canonicalizer/tokenizer, so the `config_hash` is the `UNCOMPUTED_CONFIG_HASH` sentinel; if you need that field populated, see `metadata_with`.
>
> **How.** Pattern-matches the variant and reads cheap, intrinsic fields.
>
> **Does.** Returns `FingerprintMetadata`. Always succeeds.

```rust
pub fn metadata(&self) -> FingerprintMetadata
```

Returns `FingerprintMetadata { algorithm, config_hash: UNCOMPUTED_CONFIG_HASH, model_id, schema_version, byte_size }`. The `config_hash` is set to the `UNCOMPUTED_CONFIG_HASH` sentinel because the enum doesn't know the canonicalizer / tokenizer.

##### `metadata_with()`

> **What.** Producer-side metadata: takes the producing canonicalizer + tokenizer name + algorithm config string and computes the full metadata including `config_hash`.
>
> **Why.** Whenever you have the producing context in scope, populate the config hash — downstream consumers use it to detect mismatched producer settings before comparing.
>
> **How.** Concatenates the three identifiers, hashes, and stamps the result on the metadata struct.
>
> **Does.** Recommended at the point of fingerprint creation, not later.

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

> **What.** A short, stable display name — useful for logs, telemetry, and as part of an index key.
>
> **Why.** The wire format encodes a schema version per variant; `name()` rolls that into a human-readable form. Frozen since v0.1.0 — safe to use as part of a persistent storage key.
>
> **How.** Pattern-match plus formatted string.
>
> **Does.** Format follows the table below; see the example for the canonical "fully-qualified key including config disambiguation" form.

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

> **What.** A zero-copy persistence pattern over `Vec<MinHashSig<H>>`.
>
> **Why.** When you're persisting a million signatures, you don't want to round-trip through serde. `MinHashSig<H>` is `Pod` and has a fixed `#[repr(C)]` layout, so the slice can be cast to bytes and written or sent directly.
>
> **How.** `bytemuck::cast_slice(&sigs)` reinterprets `&[MinHashSig<H>]` as `&[u8]` with no copy and no bounds checking — pure pointer math. Length is `sigs.len() * (8 + 8*H)`.
>
> **Does.** Round-trips byte-for-byte; deserialize on read with `bytemuck::cast_slice` in reverse, or use `bytemuck::pod_read_unaligned` for arbitrary alignment.

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

> **What.** Dense-vector representations of text produced by neural models, plus the comparison plumbing for them.
>
> **Why.** Classical fingerprints capture *surface* similarity (tokens / characters / bytes). Semantic embeddings capture *meaning* — `"a fluffy cat"` and `"a small fluffy feline"` get high similarity even though they share almost no tokens. The right choice when retrieval should match meaning rather than wording.
>
> **How.** A provider trait abstracts the model source: local ONNX (no network), OpenAI / Voyage / Cohere (cloud APIs). Output is an `Embedding` (dense `Vec<f32>` plus optional `model_id`). Cosine similarity via `semantic_similarity`.
>
> **Does.** `LocalProvider` is the right pick when you need to embed at high volume, control latency, or run offline. Cloud providers shine for low-volume use, infrequent embeds, or when you want best-of-class quality without managing model files.

#### `Embedding`

> **What.** A dense vector plus an optional `model_id` tag.
>
> **Why.** The `model_id` is what makes cross-provider comparison detectable. `semantic_similarity` refuses to compare embeddings whose `model_id`s differ, because two models' vector spaces are not meaningfully comparable.
>
> **How.** Plain `Vec<f32>` for the vector — easy to serde, easy to feed into other pipelines (Faiss, hnsw, …). Constructors validate at creation time.
>
> **Does.** `new` rejects empty vectors and non-finite values (NaN, ±Inf). `dim()`, `l2_norm()`, `normalize()`, `dot()` are the obvious accessors.

```rust
pub struct Embedding {
    pub vector:   Vec<f32>,
    pub model_id: Option<String>,
}
```

##### Constructors

> **What.** Validating constructors and a few cheap accessors.
>
> **Why.** Catching NaN/±Inf at construction means downstream cosine similarity can assume well-formed inputs. Without that, a single bad embedding can poison every index lookup that touches it.
>
> **How.** Linear scan over the vector at construction time; constant-time accessors thereafter.
>
> **Does.** `new` returns `Err` on empty or non-finite. `with_model` is the same plus a model id. `normalize` mutates in place to L2-norm 1.

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

> **What.** The contract every provider implements.
>
> **Why.** Lets generic code (e.g. retrieval pipelines, batch jobs) swap providers without touching downstream comparison logic. The shape is deliberately the same as in the sibling `imgfprint` crate — `ucfp` consumes both with one trait.
>
> **How.** Associated `Input` type lets a provider take `str`, `Path`, or anything else; `embed` returns an `Embedding`. `model_id` and `dimension` exist for sanity checks before/after.
>
> **Does.** `Send + Sync` on every provider in this crate; share one across worker threads.

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

> **What.** Cosine similarity between two embeddings.
>
> **Why.** Cosine is the standard distance metric for sentence/document embeddings — it's invariant to vector magnitude, so models that don't normalize at output don't bias the comparison. The wrapper exists to enforce model-id and dimension checks in one place rather than at every callsite.
>
> **How.** Refuses to compare under three conditions (different `model_id`s, different dimensions, zero L2 norm) — each gets a typed error. Otherwise computes the standard cosine.
>
> **Does.** Returns `f32` in `[-1.0, 1.0]`. Negative values do occur for some embedding spaces but are uncommon for sentence transformers.

```rust
pub fn semantic_similarity(a: &Embedding, b: &Embedding) -> Result<f32>
```

Cosine similarity in `[-1.0, 1.0]`. Refuses to compare:

- Embeddings whose `model_id`s differ → `Error::ModelMismatch`.
- Embeddings of different dimensions → `Error::DimensionMismatch`.
- Embeddings with zero L2 norm → `Error::InvalidInput`.

#### `LocalProvider` (ONNX)

> **What.** A provider that runs ONNX-format embedding models locally via `ort` 2.0.
>
> **Why.** No per-call network latency, no rate limits, no spend per token, no data egress. The trade-off is operating cost: you ship the ONNX file (50–500 MB typical) and budget the RAM/CPU/GPU for inference. For high-volume offline embedding (batch jobs, ingestion pipelines), this is the obvious choice.
>
> **How.** `from_pretrained(model_id)` fetches from Hugging Face Hub via `hf-hub`, picks the right pooling and query/document prefix from a built-in table, and warms the session. `from_onnx(...)` and the builder cover self-hosted models. All inference is serialized behind an internal mutex (ONNX Runtime sessions aren't safe to call from multiple threads simultaneously).
>
> **Does.** Cheap to clone (`Arc` under the hood) so one instance fans out to workers without re-loading. `embed_query` and `embed_document` apply the per-model prefix tables — important for asymmetric models like BGE and E5 where queries and passages are encoded differently.

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

> **What.** HTTP-based providers — `OpenAiProvider`, `VoyageProvider`, `CohereProvider`.
>
> **Why.** Best-in-class quality without managing model files; per-call cost instead of fixed operating cost; useful when embed volume is small or sporadic. Voyage and Cohere also support input-type prefixes natively (search vs. document) which improves retrieval relevance for those models.
>
> **How.** Each provider takes an API key; `with_model` overrides the default model. All three share a common retry policy (below). `Debug` impls redact the API key — safe to log.
>
> **Does.** Synchronous-looking API; internally uses `reqwest::blocking`. Each `embed_batch` call submits one HTTP request — bigger batches mean fewer round trips and lower per-embed latency.

##### `OpenAiProvider`

> **What.** OpenAI Embeddings API client.
>
> **Why.** Default to `text-embedding-3-small` (1536 dims) for retrieval; `text-embedding-3-large` (3072 dims) when you want top quality.
>
> **How.** Standard `embed(s)` and `embed_batch(&[s, s])` methods.
>
> **Does.** Returns `Embedding` with `model_id` set to the chosen model.

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

> **What.** Voyage AI Embeddings API client.
>
> **Why.** Voyage's retrieval-tuned models (`voyage-3`, `voyage-large-2`) outperform OpenAI on out-of-domain retrieval benchmarks. The `input_type` parameter (`"document"` / `"query"`) is required for asymmetric retrieval.
>
> **How.** `embed_batch(&[s, s], Some("document"))` — note the input-type argument.
>
> **Does.** Returns embeddings tagged with the chosen model.

```rust
# #[cfg(feature = "voyage")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::semantic::providers::VoyageProvider;

let p = VoyageProvider::new(std::env::var("VOYAGE_API_KEY").unwrap())?;
let docs = p.embed_batch(&["lorem", "ipsum"], Some("document"))?;
# Ok(()) }
```

##### `CohereProvider`

> **What.** Cohere Embed v3 API client.
>
> **Why.** Cohere offers strong multilingual quality and a different `input_type` taxonomy (`"search_document"`, `"search_query"`, `"classification"`, `"clustering"`).
>
> **How.** `embed_batch(&[s, s], "search_document")` — the input type is required.
>
> **Does.** Returns embeddings tagged with the Cohere model id.

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

> **What.** Convenience converters that turn HTML, Markdown, or PDF bytes into plain text suitable for the canonicalizer.
>
> **Why.** Real ingest pipelines rarely receive plain text. Stripping markup is a routine preprocessing step, and getting it wrong (e.g. concatenating `<script>` content into the text body) introduces noise that surfaces as false-positive duplicates downstream. The PDF parser is wrapped in a wall-clock timeout because adversarial PDFs can hang naïve parsers indefinitely.
>
> **How.** HTML uses a streaming SAX-style walker that drops script/style content. Markdown uses `pulldown-cmark` with options for code-block inclusion. PDF uses `pdf-extract` on a worker thread with a 30 s default timeout and 50 MiB default size cap.
>
> **Does.** Pipe the output directly into `Canonicalizer::canonicalize`. NUL bytes from PDF extraction are replaced with U+FFFD so they don't trip downstream string handling.

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

> **What.** A chunk-fed alternative to one-shot `fingerprint(s)`.
>
> **Why.** When the input is large enough that holding it in memory is wasteful — a multi-megabyte log file, a streamed HTTP body, an `mmap`'d file you don't want to read whole — the streaming variant lets you feed bounded chunks. It also makes it natural to stop early on size limits.
>
> **How.** Both MinHash and SimHash ship streaming variants today. Internally they buffer UTF-8-validated bytes (capped at 16 MiB by default; tunable via `with_max_bytes`) and run the offline algorithm at `finalize`. UTF-8 sequences split across chunks are handled correctly by carrying the trailing partial bytes into the next `update`.
>
> **Does.** Identical signature output to the offline path for the same total input. True online positional MinHash (no buffering) is queued for a later release. Use streaming when memory matters; otherwise stick with `fingerprint(s)`.

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

> **What.** `Serialize` / `Deserialize` impls for the public signature types, gated behind the `serde` feature.
>
> **Why.** Two persistence stories cover most needs: zero-copy via `bytemuck` (best for huge corpora) and serde (best for human-readable configs and mixed-format storage). Serde is what you want when you're embedding signatures in JSON logs, Postgres `jsonb` columns, or RPC payloads.
>
> **How.** `MinHashSig<H>` uses hand-rolled impls so const-generic arrays round-trip through every serde format — JSON struct with array, bincode tight tuple, etc. `SimHash64` uses `#[serde(transparent)]` over `u64`. `Embedding` is a regular derive.
>
> **Does.** Length validation is enforced on deserialize: a `MinHashSig<128>` JSON `hashes` array of the wrong length is rejected, not silently truncated.

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

> **What.** A single `txtfp::Error` enum returned from every fallible API.
>
> **Why.** One error type — instead of a per-module zoo — makes call-site `?`-propagation trivial and lets downstream code match on a single shape. `#[non_exhaustive]` means new variants can be added without breaking semver.
>
> **How.** Variants are organized by source: validation (`InvalidInput`, `Config`), comparison (`ModelMismatch`, `DimensionMismatch`, `SchemaMismatch`), I/O / external (`Io`, `Tokenizer`, `Onnx`, `Http`), and feature gating (`FeatureDisabled`).
>
> **Does.** Match exhaustively only inside the crate. Downstream code should always include a wildcard arm — see the table below for the most common variants you'll handle.

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

> **What.** Practical throughput knobs in roughly the order they pay off.
>
> **Why.** The defaults are conservative — they assume an arbitrary x86_64 with no LTO and the stable allocator. Most workloads can squeeze 2–4× by flipping the right two or three settings. The list below is ordered by how often each tip materially moves the needle.
>
> **How.** Each tip is a single configuration or pattern change. None require code restructuring; most are `Cargo.toml` or environment edits.
>
> **Does.** Combine them — `target-cpu=native` + `lto=fat` + mimalloc + `extend_par` is roughly the production-bench profile this crate publishes against.

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

> **What.** Cargo features that gate optional functionality.
>
> **Why.** The minimal feature set keeps compile time and binary size small for embedded and `no_std` targets. Heavyweight dependencies (`ort`, `reqwest`, `pdf-extract`, `jieba-rs`) only land in the dep tree when their feature is on.
>
> **How.** Toggle in `Cargo.toml`'s `[dependencies]` block: `txtfp = { version = "0.2", features = ["lsh", "semantic"] }`. Default features (`std`, `minhash`, `simhash`) are on unless you set `default-features = false`.
>
> **Does.** Code that calls a gated API behind the wrong feature gets a compile error or a `FeatureDisabled` runtime error, depending on the API shape. Mix and match freely — features are designed to be orthogonal.

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

> **What.** A small set of types and constants kept identical across the three sibling crates (`audiofp`, `imgfprint`, `txtfp`).
>
> **Why.** The cross-modal integrator `ucfp` consumes all three. If `EmbeddingProvider` had different shapes per crate, `ucfp` would need three trait shims; if `FORMAT_VERSION` could drift, `ucfp` would silently load incompatible signatures. The parity contract is what makes one storage layer serve every modality.
>
> **How.** Each release line locks the parity surface (trait shape, struct layout, error semantics, format constant). The CI in `ucfp` asserts the constants are equal across the three; downstream vendoring should do the same.
>
> **Does.** If you're vendoring all three, copy the assertion below into your integration suite. A failing assertion at build time is cheaper than a silent comparison bug at runtime.

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
