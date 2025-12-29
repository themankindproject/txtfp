# Production-grade text fingerprinting

Text fingerprinting reduces a document to a compact, locality-preserving
signature that supports approximate matching at scale.

## Algorithms

The three classical building blocks are:

- **MinHash**: Estimates Jaccard similarity over shingled token sets.
  See [Broder 1997](https://example.com/broder).
- **SimHash**: Charikar 2002. Projects a token-weighted bag of words
  onto a single 64-bit fingerprint.
- **LSH**: Bands MinHash signatures so near-duplicate retrieval is
  near-constant time.

```rust
let fp = MinHashFingerprinter::<_, 128>::new(canon, tok);
let sig = fp.fingerprint("the quick brown fox").unwrap();
```

> Inline code spans like `frobnicate` should round-trip through the
> Markdown extractor.

A second paragraph contains *italic*, **bold**, and ~~strikethrough~~
text. None of these should affect the fingerprint after the markup
extractor renders to plain text and the canonicalizer drops format
codepoints.
