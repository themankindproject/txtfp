# txtfp — Usage Guide

> From zero to full SDK mastery. Follow this guide line by line.

---

## Table of Contents

1. [Installation](#installation)
2. [Your First Fingerprint](#your-first-fingerprint)
3. [Understanding the Pipeline](#understanding-the-pipeline)
4. [Stage 1: Canonicalization](#stage-1-canonicalization)
5. [Stage 2: Tokenization](#stage-2-tokenization)
6. [Stage 3: Fingerprinting](#stage-3-fingerprinting)
   - [MinHash](#minhash)
   - [SimHash](#simhash)
   - [TLSH](#tlsh)
   - [LSH Index](#lsh-index)
7. [Stage 4: Comparison](#stage-4-comparison)
8. [Semantic Embeddings](#semantic-embeddings)
9. [Streaming Fingerprints](#streaming-fingerprints)
10. [Markup & PDF Helpers](#markup--pdf-helpers)
11. [Serialization](#serialization)
12. [Error Handling](#error-handling)
13. [Performance Guide](#performance-guide)
14. [Feature Flags Reference](#feature-flags-reference)
15. [Cross-SDK Parity](#cross-sdk-parity)

---

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
txtfp = "0.2"
```

This gives you the default features: `std`, `minhash`, `simhash`, `lsh`.

### Minimal build (no_std + alloc, WASM-compatible)

```toml
[dependencies]
txtfp = { version = "0.2", default-features = false, features = ["minhash", "simhash"] }
```

### Full classical surface (no heavy ONNX deps)

```toml
[dependencies]
txtfp = { version = "0.2", features = ["lsh", "tlsh", "markup", "security", "serde", "parallel"] }
```

### With local ONNX embeddings

```toml
[dependencies]
txtfp = { version = "0.2", features = ["semantic"] }
```

> **Upgrading from 0.1.x?** v0.2.0 changed the default hash family from
> `MurmurHash3_x64_128` to `Xxh3_64`. Signature bytes are different.
> Pin to `0.1` or pass `HashFamily::MurmurHash3_x64_128` explicitly for
> backward compatibility. See [Tweaking the hash family](#tweaking-the-hash-family).

---

## Your First Fingerprint

The simplest end-to-end example: fingerprint two sentences and check if they're near-duplicates.

```rust
use txtfp::{
    Canonicalizer, Fingerprinter, MinHashFingerprinter,
    ShingleTokenizer, WordTokenizer, jaccard,
};

fn main() -> Result<(), txtfp::Error> {
    // 1. Build the pipeline
    let canon = Canonicalizer::default();
    let tok = ShingleTokenizer { k: 5, inner: WordTokenizer };
    let fp = MinHashFingerprinter::<_, 128>::new(canon, tok);

    // 2. Fingerprint two documents
    let a = fp.fingerprint("the quick brown fox jumps over the lazy dog at noon today")?;
    let b = fp.fingerprint("the quick brown fox jumps over the lazy dog at dusk today")?;

    // 3. Compare
    let similarity = jaccard(&a, &b);
    println!("Jaccard estimate: {similarity:.2}");

    if similarity > 0.6 {
        println!("→ near-duplicate detected");
    }
    Ok(())
}
```

**What happened:**
1. `Canonicalizer::default()` applies NFKC + casefold + bidi/format strip
2. `ShingleTokenizer { k: 5, inner: WordTokenizer }` splits into words, then creates 5-grams
3. `MinHashFingerprinter::<_, 128>::new(...)` sketches each shingle into 128 hash slots
4. `jaccard(&a, &b)` counts matching slots / 128

---

## Understanding the Pipeline

Every fingerprint in txtfp flows through four stages:

```
&str input
    │
    ▼
Canonicalizer     →  normalized String
    │
    ▼
Tokenizer         →  stream of &str tokens
    │
    ▼
Fingerprinter     →  fixed-size signature
    │
    ▼
compare()         →  similarity score
```

Each stage is a trait with multiple implementations. You pick one implementation per stage and compose them. The same input with the same configuration always produces identical bytes.

---

## Stage 1: Canonicalization

Canonicalization maps "visually or semantically equivalent" inputs to the same bytes. This is critical: without it, `"Hello"` and `"hello"` would produce completely different fingerprints.

### Default: `Canonicalizer::default()`

Applies: NFKC normalization → Bidi/format strip → simple casefold.

```rust
use txtfp::Canonicalizer;

let c = Canonicalizer::default();

// Case folding
assert_eq!(c.canonicalize("Hello World"), "hello world");

// ZWSP (zero-width space) stripped
assert_eq!(c.canonicalize("Hello\u{200B}World"), "helloworld");

// Full-width → ASCII (NFKC)
assert_eq!(c.canonicalize("ＡＢＣ"), "abc");

// Trojan Source attack neutralized (RLO stripped)
assert_eq!(c.canonicalize("admin\u{202E}drow"), "admindrow");

// Ligature decomposed
assert_eq!(c.canonicalize("ﬁle"), "file");
```

### Custom: `CanonicalizerBuilder`

```rust
use txtfp::{CanonicalizerBuilder, CaseFold, Normalization};

// NFC instead of NFKC (preserves full-width chars)
let c = CanonicalizerBuilder {
    normalization: Normalization::Nfc,
    case_fold: CaseFold::Simple,
    strip_bidi: true,
    strip_format: true,
    apply_confusable: false,
}.build();
```

### Security: Confusable Skeleton (`security` feature)

Maps visually similar characters to a common form. Use for username/domain comparison, not full-text dedup (it's lossy).

```rust
# #[cfg(feature = "security")]
# {
use txtfp::CanonicalizerBuilder;

let c = CanonicalizerBuilder {
    apply_confusable: true,
    ..Default::default()
}.build();

// Cyrillic 'а' and Latin 'a' fold to the same skeleton
assert_eq!(c.canonicalize("раураl"), c.canonicalize("paypal"));
# }
```

### `config_string()` — Identifying Your Configuration

```rust
use txtfp::Canonicalizer;

let c = Canonicalizer::default();
println!("{}", c.config_string());  // "nfkc-cf-simple-bidi-fmt"
```

Feed this into `txtfp::config_hash()` to get a 64-bit identifier for storing alongside signatures.


---

## Stage 2: Tokenization

Tokenizers split canonicalized text into a stream of tokens. All tokenizers implement the `Tokenizer` trait:

```rust
pub trait Tokenizer: Send + Sync {
    fn tokens<'a>(&'a self, input: &'a str) -> TokenStream<'a>;
    fn name(&self) -> Cow<'static, str>;
    fn for_each_token(&self, input: &str, f: &mut dyn FnMut(&str));
}
```

Two consumption paths:
- `tokens()` — returns an iterator (may allocate per token)
- `for_each_token()` — zero-allocation callback (used by all classical sketchers internally)

### `WordTokenizer`

UAX #29 word boundaries. Filters out whitespace and punctuation. Zero-sized, `Copy`.

```rust
use txtfp::{Tokenizer, WordTokenizer};

let mut tokens = Vec::new();
WordTokenizer.for_each_token("don't go!", &mut |t| tokens.push(t.to_string()));
assert_eq!(tokens, ["don't", "go"]);
```

**Behavior notes:**
- Contractions are one token: `"don't"` → `["don't"]`
- Punctuation filtered: `"hello, world!"` → `["hello", "world"]`
- Numbers are tokens: `"v2.0"` → `["v2.0"]`

### `GraphemeTokenizer`

UAX #29 extended grapheme clusters. Every user-perceived character is one token. Does **not** filter whitespace.

```rust
use txtfp::{Tokenizer, GraphemeTokenizer};

let mut tokens = Vec::new();
GraphemeTokenizer.for_each_token("a\u{0301}🇺🇸", &mut |t| tokens.push(t.to_string()));
// á (combining) = 1 token, 🇺🇸 (flag) = 1 token
assert_eq!(tokens.len(), 2);
```

### `ShingleTokenizer`

K-gram adaptor over any inner tokenizer. Joins k consecutive tokens with a space. This is the standard input for MinHash.

```rust
use txtfp::{ShingleTokenizer, Tokenizer, WordTokenizer};

let s = ShingleTokenizer { k: 3, inner: WordTokenizer };
let mut shingles = Vec::new();
s.for_each_token("the quick brown fox", &mut |t| shingles.push(t.to_string()));
assert_eq!(shingles, ["the quick brown", "quick brown fox"]);
```

**Choosing k:**
- `k = 3` — more matches, more noise (higher recall)
- `k = 5` — production sweet spot for English dedup
- `k = 7..10` — stricter matching for long technical prose

**Edge cases:**
- `k = 0` → empty stream
- Fewer than k tokens → single shingle of all tokens joined

### `CjkTokenizer` (`cjk` feature)

Chinese/Japanese/Korean segmentation. Dictionary loaded once via `OnceLock`.

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

Available segmenters:
- `CjkSegmenter::Jieba` — Simplified Chinese (default, `cjk` feature)
- Lindera IPADIC — Japanese (`cjk-japanese` feature, +50 MiB)
- Lindera ko-dic — Korean (`cjk-korean` feature, +150 MiB)

### Tokenizer Names (stable identifiers)

| Tokenizer | `name()` |
|-----------|----------|
| `WordTokenizer` | `"word-uax29"` |
| `GraphemeTokenizer` | `"grapheme-uax29"` |
| `ShingleTokenizer { k: 5, inner: WordTokenizer }` | `"shingle-k=5/word-uax29"` |
| `CjkTokenizer` (jieba) | `"cjk-jieba"` |

These are baked into `FingerprintMetadata` and used by `config_hash()`.


---

## Stage 3: Fingerprinting

### Traits

Every classical algorithm implements two traits:

```rust
// One-shot: feed a whole document
pub trait Fingerprinter {
    type Output;
    fn fingerprint(&self, input: &str) -> Result<Self::Output>;
}

// Streaming: feed byte chunks
pub trait StreamingFingerprinter {
    type Output;
    fn update(&mut self, chunk: &[u8]) -> Result<()>;
    fn finalize(self) -> Result<Self::Output>;
    fn reset(&mut self);
}
```

`Fingerprinter::fingerprint` takes `&self` — share one instance across threads.

---

### MinHash

**What it does:** Estimates Jaccard set-similarity between two token sets.
**Output:** `MinHashSig<H>` — H minimum hash values (default H=128).
**Best for:** Document deduplication, near-duplicate detection at scale.

#### Basic usage

```rust
use txtfp::{
    Canonicalizer, Fingerprinter, MinHashFingerprinter,
    ShingleTokenizer, WordTokenizer, jaccard,
};

let fp = MinHashFingerprinter::<_, 128>::new(
    Canonicalizer::default(),
    ShingleTokenizer { k: 5, inner: WordTokenizer },
);

let a = fp.fingerprint("the quick brown fox jumps over the lazy dog")?;
let b = fp.fingerprint("the quick brown fox leaps over the lazy dog")?;

let j = jaccard(&a, &b);
println!("Jaccard: {j:.3}");  // ~0.6-0.8
# Ok::<_, txtfp::Error>(())
```

#### Tweaking the hash family

```rust
use txtfp::{Canonicalizer, HashFamily, MinHashFingerprinter, ShingleTokenizer, WordTokenizer};

// For datasketch / Python-MinHash byte parity:
let fp = MinHashFingerprinter::<_, 128>::new(
    Canonicalizer::default(),
    ShingleTokenizer { k: 5, inner: WordTokenizer },
)
.with_hasher(HashFamily::MurmurHash3_x64_128)
.with_seed(0xDEAD_BEEF);
```

| Family | Speed | Datasketch compatible |
|--------|-------|---------------------|
| `Xxh3_64` (default v0.2+) | ~3× faster | No |
| `MurmurHash3_x64_128` | Reference | Yes |

#### Using the builder

```rust
use txtfp::{
    Canonicalizer, MinHashFingerprinterBuilder,
    ShingleTokenizer, WordTokenizer,
};

let fp = MinHashFingerprinterBuilder::default()
    .seed(42)
    .build::<_, 128>(
        Canonicalizer::default(),
        ShingleTokenizer { k: 5, inner: WordTokenizer },
    );
```

#### Signature properties

- `MinHashSig<128>` is 1032 bytes (`8 + 8×128`)
- `bytemuck::Pod` — zero-copy serialization via `bytemuck::cast_slice`
- Schema version = 1 (frozen since v0.1.0)
- Slot values changed in v0.2.0 (hash family flip)

#### Bulk persistence (zero-copy)

```rust
# #[cfg(feature = "minhash")]
# {
use txtfp::MinHashSig;

let sigs: Vec<MinHashSig<128>> = vec![MinHashSig::empty(); 1000];
let bytes: &[u8] = bytemuck::cast_slice(&sigs);  // zero-copy
assert_eq!(bytes.len(), 1000 * 1032);

// Round-trip back
let view: &[MinHashSig<128>] = bytemuck::cast_slice(bytes);
assert_eq!(view.len(), 1000);
# }
```

---

### SimHash

**What it does:** Projects a weighted token bag into 64 bits preserving cosine similarity.
**Output:** `SimHash64` — a single u64.
**Best for:** Fast near-duplicate detection when you need tiny signatures.

#### Basic usage

```rust
use txtfp::{
    Canonicalizer, Fingerprinter, SimHashFingerprinter,
    WordTokenizer, hamming, cosine_estimate,
};

let fp = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer);

let a = fp.fingerprint("the quick brown fox jumps over the lazy dog")?;
let b = fp.fingerprint("the quick brown fox leaps over the lazy dog")?;

let dist = hamming(a, b);
let cos = cosine_estimate(a, b);
println!("Hamming: {dist}, Cosine: {cos:.3}");
# Ok::<_, txtfp::Error>(())
```

#### Weighting strategies

```rust
use txtfp::{Canonicalizer, IdfTable, SimHashFingerprinter, Weighting, WordTokenizer};

// Default: Tf (each occurrence contributes ±1)
let fp_tf = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer);

// Uniform: each distinct token contributes ±1 regardless of frequency
let fp_uni = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer)
    .with_weighting(Weighting::Uniform);

// IDF-weighted: TF × IDF from a custom table
let table = IdfTable::from_pairs([("the", 0.1_f32), ("dog", 4.0_f32)]);
let fp_idf = SimHashFingerprinter::new(Canonicalizer::default(), WordTokenizer)
    .with_weighting(Weighting::IdfWeighted(table));
```

#### Interpreting results

| Hamming distance | Cosine estimate | Meaning |
|-----------------|-----------------|---------|
| 0 | 1.0 | Identical |
| 1–8 | 0.92–1.0 | Very similar |
| 9–16 | 0.71–0.92 | Similar |
| 17–32 | 0.0–0.71 | Weakly related |
| 33+ | < 0.0 | Unrelated |

---

### TLSH (`tlsh` feature)

**What it does:** Byte-level locality-sensitive hash using trigram histograms.
**Output:** `TlshFingerprint` — 70-char hex string.
**Best for:** Binary similarity, log-line comparison, short documents.

#### Basic usage

```rust
# #[cfg(feature = "tlsh")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::{Canonicalizer, Fingerprinter, TlshFingerprinter, tlsh_distance};

let fp = TlshFingerprinter::new(Canonicalizer::default());

// TLSH needs ≥ 50 bytes of input
let a = fp.fingerprint(
    "the quick brown fox jumps over the lazy dog at noon today \
     the slow grey wolf creeps under the loud ravens at dusk"
)?;
let b = fp.fingerprint(
    "the quick brown fox jumps over the lazy dog at dusk today \
     the slow grey wolf creeps under the loud ravens at dawn"
)?;

let dist = tlsh_distance(&a, &b)?;
println!("TLSH distance: {dist}");  // lower = more similar
// < 50 = high similarity, < 100 = moderate
# Ok(()) }
```

#### Raw bytes (skip canonicalization)

```rust
# #[cfg(feature = "tlsh")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::{Canonicalizer, TlshFingerprinter};

let fp = TlshFingerprinter::new(Canonicalizer::default());
let sig = fp.sketch_bytes(&[0u8; 100])?;  // raw bytes, no canonicalization
# Ok(()) }
```

---

### LSH Index (`lsh` feature)

**What it does:** Sub-linear near-duplicate retrieval over MinHash signatures.
**Complexity:** O(1) average query time vs O(N) brute-force.
**Best for:** Large-scale dedup where you can't compare every pair.

#### Basic usage

```rust
# #[cfg(feature = "lsh")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::{
    Canonicalizer, Fingerprinter, LshIndex, LshIndexBuilder,
    MinHashFingerprinter, ShingleTokenizer, WordTokenizer,
};

let fp = MinHashFingerprinter::<_, 128>::new(
    Canonicalizer::default(),
    ShingleTokenizer { k: 5, inner: WordTokenizer },
);

// Auto-optimize bands/rows for Jaccard threshold 0.7
let mut idx: LshIndex<128> = LshIndexBuilder::for_threshold(0.7, 128)?.build();

// Index documents
idx.insert(0, fp.fingerprint("the quick brown fox jumps over the lazy dog at noon")?);
idx.insert(1, fp.fingerprint("the quick brown fox jumps over the lazy dog at dusk")?);
idx.insert(2, fp.fingerprint("astronomers detect cosmic background radiation")?);

// Query
let probe = fp.fingerprint("the quick brown fox jumps over the lazy dog at dawn")?;
let results = idx.query_with_threshold(&probe, 0.5);
println!("Near-duplicates: {results:?}");  // [0, 1]
# Ok(()) }
```

#### Manual bands/rows

```rust
# #[cfg(feature = "lsh")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::LshIndex;

// 64 bands × 2 rows = 128 slots. High recall, more candidates.
let mut idx: LshIndex<128> = LshIndex::with_bands_rows(64, 2)?;
# Ok(()) }
```

#### Choosing bands and rows (H=128)

| (bands, rows) | Threshold sweet spot | Use case |
|---------------|---------------------|----------|
| (8, 16) | ~0.95 | Exact dedup only |
| (16, 8) | ~0.85 | Strict near-dup |
| (32, 4) | ~0.65 | Moderate fuzzy |
| (64, 2) | ~0.45 | High recall |

**Rule of thumb:** Use `LshIndexBuilder::for_threshold(t, 128)` unless you have measurements.

#### Query methods

- `query(&sig)` — returns all bucket candidates (fast, may include false positives)
- `query_with_threshold(&sig, t)` — verifies each candidate with exact `jaccard()` (precise)

#### Parallel bulk insert (`parallel` feature)

```rust
# #[cfg(all(feature = "lsh", feature = "parallel"))]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::{
    Canonicalizer, Fingerprinter, LshIndex,
    MinHashFingerprinter, ShingleTokenizer, WordTokenizer,
};

let fp = MinHashFingerprinter::<_, 128>::new(
    Canonicalizer::default(),
    ShingleTokenizer { k: 5, inner: WordTokenizer },
);

let pairs: Vec<(u64, _)> = ["doc one", "doc two", "doc three"]
    .iter()
    .enumerate()
    .map(|(i, d)| Ok((i as u64, fp.fingerprint(d)?)))
    .collect::<Result<_, txtfp::Error>>()?;

let mut idx = LshIndex::<128>::with_bands_rows(16, 8)?;
idx.extend_par(pairs);  // sharded by band, contention-free, ~1.74× on 8 cores
# Ok(()) }
```

#### Thread safety

- `query` / `query_with_threshold` take `&self` — safe to share across threads
- `insert` / `remove` take `&mut self` — wrap in `RwLock` or `Mutex` for concurrent writes


---

## Stage 4: Comparison

Summary of all comparison functions:

| Function | Signatures | Returns | Meaning |
|----------|-----------|---------|---------|
| `jaccard(a, b)` | `MinHashSig<H>` | `f32 [0, 1]` | Fraction of matching slots ≈ Jaccard |
| `hamming(a, b)` | `SimHash64` | `u32 [0, 64]` | Number of differing bits |
| `cosine_estimate(a, b)` | `SimHash64` | `f32 [-1, 1]` | `cos((hamming/64) × π)` |
| `tlsh_distance(a, b)` | `TlshFingerprint` | `Result<i32>` | Lower = more similar |
| `semantic_similarity(a, b)` | `Embedding` | `Result<f32> [-1, 1]` | Cosine similarity |

---

## Semantic Embeddings

Dense vector representations that capture **meaning**, not just surface tokens. Requires the `semantic` feature (or `openai`/`voyage`/`cohere` for cloud providers).

### Local ONNX Provider

No network calls, no rate limits, no per-token cost.

```rust,no_run
# #[cfg(feature = "semantic")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::{EmbeddingProvider, LocalProvider, semantic_similarity};

// Downloads model from Hugging Face Hub on first call
let provider = LocalProvider::from_pretrained("BAAI/bge-small-en-v1.5")?;

let query = provider.embed_query("a fluffy cat")?;
let doc = provider.embed_document("a small fluffy feline named Whiskers")?;

let sim = semantic_similarity(&query, &doc)?;
println!("Semantic similarity: {sim:.3}");
# Ok(()) }
```

**`embed_query` vs `embed_document`:** Asymmetric models (BGE, E5) prepend different prefixes. Use `embed_query` for search queries, `embed_document` for corpus documents.

### Builder for self-hosted models

```rust,no_run
# #[cfg(feature = "semantic")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::{LocalProvider, Pooling};

let provider = LocalProvider::builder()
    .model_id("acme/in-house-embedder-v3")
    .onnx_path("/srv/models/embedder.onnx")
    .tokenizer_path("/srv/models/tokenizer.json")
    .pooling(Pooling::Cls)
    .max_seq_len(512)
    .intra_threads(8)
    .build()?;
# Ok(()) }
```

### Pooling strategies

| Variant | Models | Description |
|---------|--------|-------------|
| `Cls` | BGE, Snowflake Arctic, mxbai | First token's hidden state |
| `Mean` | E5, MiniLM, GTE, Nomic | Average over attention mask |
| `MeanNoNorm` | — | Mean without L2 normalization |
| `Max` | Rare | Element-wise max |

`from_pretrained` auto-selects the correct pooling per model.

### Cloud Providers

#### OpenAI (`openai` feature)

```rust
# #[cfg(feature = "openai")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::semantic::providers::OpenAiProvider;
use txtfp::EmbeddingProvider;

let p = OpenAiProvider::new(std::env::var("OPENAI_API_KEY").unwrap())?
    .with_model("text-embedding-3-small");

let e = p.embed("the quick brown fox")?;
assert_eq!(e.dim(), 1536);

// Batch embedding
let batch = p.embed_batch(&["fox", "wolf", "lion"])?;
# Ok(()) }
```

#### Voyage (`voyage` feature)

```rust
# #[cfg(feature = "voyage")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::semantic::providers::VoyageProvider;

let p = VoyageProvider::new(std::env::var("VOYAGE_API_KEY").unwrap())?;
let docs = p.embed_batch(&["lorem", "ipsum"], Some("document"))?;
# Ok(()) }
```

#### Cohere (`cohere` feature)

```rust
# #[cfg(feature = "cohere")]
# fn demo() -> Result<(), txtfp::Error> {
use txtfp::semantic::providers::CohereProvider;

let p = CohereProvider::new(std::env::var("COHERE_API_KEY").unwrap())?;
let docs = p.embed_batch(&["lorem", "ipsum"], "search_document")?;
# Ok(()) }
```

### Retry policy (all cloud providers)

All providers share a unified retry policy:
- Exponential backoff: 500ms initial, 2× multiplier, ±30% jitter
- Honors `Retry-After` header on 429s (capped at 60s)
- Total wall-clock cap: 90s
- Permanent failures (400, 401, 403, 404, 422) bubble up immediately
- API keys redacted in `Debug` output

### Chunking long documents

```rust
# #[cfg(feature = "semantic")]
# {
use txtfp::{ChunkMode, ChunkingStrategy, chunk_for_model};

let strategy = ChunkingStrategy {
    max_tokens: 256,
    overlap: 32,
    mode: ChunkMode::Recursive,  // paragraph → sentence → word fallback
};

let chunks = chunk_for_model("Long document text...", &strategy);
// Embed each chunk separately, then pool or store individually
# }
```

Chunk modes:
- `FixedTokens` — greedy sliding windows with overlap
- `SentenceBounded` — packs whole sentences up to max_tokens
- `Recursive` — paragraph → sentence → word fallback

---

## Streaming Fingerprints

For large files or network streams where you can't load the entire document into memory:

```rust
use txtfp::{
    Canonicalizer, MinHashFingerprinter, MinHashStreaming,
    ShingleTokenizer, StreamingFingerprinter, WordTokenizer,
};

let inner = MinHashFingerprinter::<_, 128>::new(
    Canonicalizer::default(),
    ShingleTokenizer { k: 3, inner: WordTokenizer },
);
let mut stream = MinHashStreaming::new(inner);

// Feed chunks as they arrive
stream.update(b"the quick brown fox")?;
stream.update(b" jumps over the lazy dog")?;

// Finalize when done
let sig = stream.finalize()?;
# Ok::<_, txtfp::Error>(())
```

### Configuring buffer size

```rust
# use txtfp::*;
# let inner = MinHashFingerprinter::<_, 128>::new(
#     Canonicalizer::default(), ShingleTokenizer { k: 3, inner: WordTokenizer });
let mut stream = MinHashStreaming::new(inner)
    .with_max_bytes(64 * 1024 * 1024);  // 64 MiB cap (default: 16 MiB)
```

### Key behaviors

- UTF-8 sequences spanning chunk boundaries are handled correctly
- Trailing incomplete UTF-8 at `finalize()` → `Error::InvalidInput`
- Empty stream at `finalize()` → `Error::InvalidInput`
- `reset()` clears the buffer for reuse without reallocating

---

## Markup & PDF Helpers

### HTML → text (`markup` feature)

```rust
# #[cfg(feature = "markup")]
# {
use txtfp::html_to_text;

let plain = html_to_text("<p>hello</p><script>alert(1)</script>")?;
assert!(plain.contains("hello"));
assert!(!plain.contains("alert"));  // script stripped
# Ok::<_, txtfp::Error>(())
# }
```

### Markdown → text (`markup` feature)

```rust
# #[cfg(feature = "markup")]
# {
use txtfp::{markdown_to_text, markdown_to_text_with, MarkdownOptions};

let text = markdown_to_text("# Heading\n\nBody with `code`")?;

// Exclude code blocks
let opts = MarkdownOptions { include_code_blocks: false, ..Default::default() };
let no_code = markdown_to_text_with("```\nlet x = 1;\n```\ntext", opts)?;
assert!(!no_code.contains("let x"));
# Ok::<_, txtfp::Error>(())
# }
```

### PDF → text (`pdf` feature)

```rust,no_run
# #[cfg(feature = "pdf")]
# {
use txtfp::{pdf_to_text, pdf_to_text_with, PdfOptions};

let bytes = std::fs::read("document.pdf")?;
let text = pdf_to_text(&bytes)?;  // 50 MiB cap, 30s timeout

// Custom limits
let opts = PdfOptions { max_bytes: 5 * 1024 * 1024, timeout_secs: 10 };
let text2 = pdf_to_text_with(&bytes, opts)?;
# Ok::<_, txtfp::Error>(())
# }
```

PDF parsing runs on a worker thread with a wall-clock timeout. NUL bytes are replaced with U+FFFD.


---

## Serialization

### Serde (`serde` feature)

```rust
# #[cfg(feature = "serde")]
# {
use txtfp::MinHashSig;

let sig: MinHashSig<128> = MinHashSig::empty();

// JSON round-trip
let json = serde_json::to_string(&sig)?;
let back: MinHashSig<128> = serde_json::from_str(&json)?;
assert_eq!(sig, back);
# Ok::<_, serde_json::Error>(())
# }
```

**Implementation details:**
- `MinHashSig<H>` uses hand-rolled serde impls (const-generic arrays don't auto-derive)
- Length validation on deserialize: wrong-length `hashes` array is rejected
- `SimHash64` uses `#[serde(transparent)]` over `u64`
- `Embedding` uses standard derive

### Zero-copy with bytemuck

For maximum throughput, skip serde entirely:

```rust
# #[cfg(feature = "minhash")]
# {
use txtfp::MinHashSig;

// Write: cast to bytes
let sigs: Vec<MinHashSig<128>> = vec![MinHashSig::empty(); 100];
let bytes: &[u8] = bytemuck::cast_slice(&sigs);
// Write `bytes` to disk/network...

// Read: cast back
let loaded: &[MinHashSig<128>] = bytemuck::cast_slice(bytes);
assert_eq!(loaded.len(), 100);
# }
```

---

## Error Handling

All fallible APIs return `Result<T, txtfp::Error>`. The error enum is `#[non_exhaustive]`:

```rust
pub enum Error {
    InvalidInput(String),
    ModelMismatch { a: String, b: String },
    DimensionMismatch { a: usize, b: usize },
    Config(String),
    Io(std::io::Error),           // std feature
    Tokenizer(String),            // semantic feature
    Onnx(String),                 // semantic feature
    Http(String),                 // openai/voyage/cohere
    EmptyEmbedding,               // semantic feature
    SchemaMismatch { expected: u16, actual: u16 },
    FeatureDisabled(&'static str),
    // ... (non_exhaustive)
}
```

### Common errors

| Call | Error |
|------|-------|
| `fp.fingerprint("")` | `InvalidInput("empty document")` |
| `fp.fingerprint("   \n")` | `InvalidInput("empty document")` |
| `LshIndex::with_bands_rows(7, 9)` | `Config("bands * rows must equal H")` |
| `semantic_similarity(a, b)` with different models | `ModelMismatch { ... }` |
| `semantic_similarity(a, b)` with different dims | `DimensionMismatch { ... }` |
| TLSH with < 50 bytes | `InvalidInput(...)` |
| Cloud provider 401 | `Http("... returned 401")` |
| PDF parse > 30s | `InvalidInput("pdf parse exceeded 30-second timeout")` |

### Best practice

```rust
use txtfp::{Error, Fingerprinter};

# fn example(fp: &impl Fingerprinter<Output = txtfp::MinHashSig<128>>) {
match fp.fingerprint("some text") {
    Ok(sig) => { /* use sig */ }
    Err(Error::InvalidInput(msg)) => eprintln!("Bad input: {msg}"),
    Err(e) => eprintln!("Unexpected: {e}"),  // wildcard for non_exhaustive
}
# }
```

---

## Performance Guide

Ordered by impact (highest first):

### 1. Compile flags

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

In `Cargo.toml`:
```toml
[profile.release]
lto = "thin"       # 5-15% gain on classical sketchers
codegen-units = 1  # better inlining
```

### 2. Use mimalloc for LSH-heavy workloads

```rust
use mimalloc::MiMalloc;
#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;
```

Roughly halves LSH insert latency (alloc-heavy SmallVec operations).

### 3. Reuse the fingerprinter

```rust
use std::sync::Arc;
use txtfp::{Canonicalizer, MinHashFingerprinter, ShingleTokenizer, WordTokenizer};

// Create once, share across threads
let fp = Arc::new(MinHashFingerprinter::<_, 128>::new(
    Canonicalizer::default(),
    ShingleTokenizer { k: 5, inner: WordTokenizer },
));

// fp.fingerprint() takes &self — safe to call from multiple threads
```

### 4. Pre-canonicalize for multi-algorithm jobs

```rust
use txtfp::{Canonicalizer, Fingerprinter, MinHashFingerprinter, SimHashFingerprinter, WordTokenizer, ShingleTokenizer};

let canon = Canonicalizer::default();
let text = "some document...";

// Canonicalize once
let canonical = canon.canonicalize(text);

// Feed to multiple algorithms (they only re-tokenize, not re-canonicalize)
// (Use sketch_canonical internally — or just call fingerprint() which is cheap
// since the ASCII fast path is ~540ns for 5KB)
```

### 5. Choose H by variance needs

| H | Size | σ at p=0.5 | Throughput |
|---|------|-----------|------------|
| 64 | 520 B | 0.063 | ~13K docs/s |
| 128 | 1032 B | 0.044 | ~9K docs/s |
| 256 | 2056 B | 0.031 | ~5K docs/s |

### 6. Use `extend_par` for bulk LSH insert

```rust
// With `parallel` feature: ~1.74× speedup on 8 cores
# #[cfg(all(feature = "lsh", feature = "parallel"))]
# fn demo(idx: &mut txtfp::LshIndex<128>, pairs: Vec<(u64, txtfp::MinHashSig<128>)>) {
idx.extend_par(pairs);
# }
```

### 7. ASCII inputs are nearly free to canonicalize

The canonicalizer's ASCII fast path runs in ~540ns per 5KB. If your corpus is ASCII (English text, code), canonicalization is effectively free.

---

## Feature Flags Reference

| Feature | Default | What it enables | Key deps |
|---------|:-------:|-----------------|----------|
| `std` | ✅ | libstd (without: `no_std + alloc`) | — |
| `minhash` | ✅ | `MinHashFingerprinter`, `MinHashSig`, `jaccard` | hashbrown |
| `simhash` | ✅ | `SimHashFingerprinter`, `SimHash64`, `hamming`, `cosine_estimate` | hashbrown |
| `lsh` | ✅ | `LshIndex`, `LshIndexBuilder` | hashbrown |
| `tlsh` | | `TlshFingerprinter`, `tlsh_distance` | tlsh2 |
| `markup` | | `html_to_text`, `markdown_to_text` | html2text, pulldown-cmark |
| `pdf` | | `pdf_to_text` (30s timeout, 50 MiB cap) | pdf-extract |
| `cjk` | | `CjkTokenizer` (Simplified Chinese) | jieba-rs |
| `cjk-japanese` | | Japanese tokenization (+50 MiB) | lindera |
| `cjk-korean` | | Korean tokenization (+150 MiB) | lindera |
| `security` | | UTS #39 confusable skeleton | unicode-security |
| `serde` | | `Serialize`/`Deserialize` on signatures | serde |
| `parallel` | | `LshIndex::extend_par` | rayon |
| `semantic` | | `LocalProvider`, `Embedding`, `semantic_similarity` | ort, tokenizers, hf-hub |
| `openai` | | `OpenAiProvider` | reqwest, serde_json, tokio |
| `voyage` | | `VoyageProvider` | reqwest, serde_json, tokio |
| `cohere` | | `CohereProvider` | reqwest, serde_json, tokio |

---

## Cross-SDK Parity

`txtfp` is one of three sibling crates under the `themankindproject` umbrella:

- [`audiofp`](https://crates.io/crates/audiofp) — audio fingerprinting
- `imgfprint` — image fingerprinting
- **`txtfp`** — text fingerprinting

The cross-modal integrator `ucfp` consumes all three. The contract:

| Surface | Guarantee |
|---------|-----------|
| `EmbeddingProvider` trait | Same shape, same method signatures |
| `Embedding` struct | Same fields: `vector: Vec<f32>`, `model_id: Option<String>` |
| `semantic_similarity()` | Same error semantics (model mismatch, dim mismatch) |
| `FORMAT_VERSION: u32` | Equal across all three crates within a release line |

```rust,ignore
assert_eq!(audiofp::FORMAT_VERSION, txtfp::FORMAT_VERSION);
assert_eq!(imgfprint::FORMAT_VERSION, txtfp::FORMAT_VERSION);
```

### The `Fingerprint` enum + `config_hash`

For multi-algorithm storage:

```rust
# #[cfg(feature = "minhash")]
# {
use txtfp::{Canonicalizer, Fingerprint, MinHashSig, config_hash};

let sig = MinHashSig::<128>::empty();
let fp = Fingerprint::MinHash(sig);

// Compute a config hash to prevent comparing incompatible signatures
let cfg = config_hash(&Canonicalizer::default(), "shingle-k=5/word-uax29", "h128-xxh3");
println!("Storage key: {}-cfg={cfg:016x}", fp.name());
// → "minhash-h128-v1-cfg=abcdef0123456789"
# }
```

#### `config_hash_classical` (recommended for MinHash/SimHash)

Automatically includes the hash family and seed — avoids the footgun of forgetting to encode them:

```rust
# #[cfg(feature = "minhash")]
# {
use txtfp::{Canonicalizer, HashFamily, config_hash_classical};

let cfg = config_hash_classical(
    &Canonicalizer::default(),
    "shingle-k=5/word-uax29",
    "h128",
    HashFamily::Xxh3_64,
    0x00C0_FFEE_5EED,
);
# }
```

Two fingerprints with different non-zero `config_hash` values must not be compared.
