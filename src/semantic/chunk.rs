//! Long-input chunking for embedding pipelines.
//!
//! Most embedding models cap input at 512 or 8192 tokens. This module
//! splits arbitrary text into overlapping chunks that stay under the
//! cap, using strategies that respect natural boundaries (sentences,
//! paragraphs) where possible.
//!
//! The token count is approximated as `words * 1.3` when no model
//! tokenizer is available — roughly the BPE-token-per-word ratio for
//! English. Adjust the strategy if your text or model differs.

use alloc::string::String;
use alloc::vec::Vec;

use unicode_segmentation::UnicodeSegmentation;

/// Splitting strategy.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ChunkMode {
    /// Greedy fixed-size token windows with overlap. Fastest; ignores
    /// structure.
    FixedTokens,
    /// Pack whole sentences into windows up to the cap.
    SentenceBounded,
    /// Recursive: split on paragraph (`\n\n+`) → sentence → word, with
    /// the LangChain-style fallback for over-long sentences.
    Recursive,
}

/// Options for [`chunk_for_model`].
#[derive(Copy, Clone, Debug)]
pub struct ChunkingStrategy {
    /// Maximum tokens per chunk.
    pub max_tokens: usize,
    /// Overlap (in tokens) between consecutive chunks. Must be
    /// `< max_tokens`. Useful for retrieval pipelines that benefit
    /// from boundary context.
    pub overlap: usize,
    /// Splitting mode.
    pub mode: ChunkMode,
}

impl Default for ChunkingStrategy {
    fn default() -> Self {
        Self {
            max_tokens: 256,
            overlap: 32,
            mode: ChunkMode::SentenceBounded,
        }
    }
}

/// Split `input` into chunks per `strategy`. Chunks are returned in
/// document order. Empty `input` yields an empty `Vec`.
#[must_use]
pub fn chunk_for_model(input: &str, strategy: &ChunkingStrategy) -> Vec<String> {
    if input.is_empty() || strategy.max_tokens == 0 {
        return Vec::new();
    }
    match strategy.mode {
        ChunkMode::FixedTokens => fixed_token_chunks(input, strategy),
        ChunkMode::SentenceBounded => sentence_chunks(input, strategy),
        ChunkMode::Recursive => recursive_chunks(input, strategy),
    }
}

/// Approximate token count: 1.3 × word count.
#[inline]
fn approx_tokens(words: usize) -> usize {
    (words as f32 * 1.3).ceil() as usize
}

/// Number of words in `s` per UAX #29.
fn word_count(s: &str) -> usize {
    s.unicode_words().count()
}

fn fixed_token_chunks(input: &str, s: &ChunkingStrategy) -> Vec<String> {
    let words: Vec<&str> = input.unicode_words().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let max_w = ((s.max_tokens as f32) / 1.3).floor() as usize;
    let max_w = max_w.max(1);
    let overlap_w = (((s.overlap as f32) / 1.3).floor() as usize).min(max_w.saturating_sub(1));
    let stride = max_w - overlap_w;

    let mut out = Vec::new();
    let mut start = 0;
    while start < words.len() {
        let end = (start + max_w).min(words.len());
        out.push(words[start..end].join(" "));
        if end == words.len() {
            break;
        }
        start += stride;
    }
    out
}

fn sentence_chunks(input: &str, s: &ChunkingStrategy) -> Vec<String> {
    let sentences: Vec<&str> = input.unicode_sentences().collect();
    if sentences.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_tokens = 0_usize;

    for sent in sentences {
        let sent = sent.trim();
        if sent.is_empty() {
            continue;
        }
        let toks = approx_tokens(word_count(sent));
        if toks > s.max_tokens {
            // Sentence alone exceeds the cap — fall through to fixed
            // token splitting for just this one sentence.
            if !current.is_empty() {
                out.push(core::mem::take(&mut current));
                current_tokens = 0;
            }
            let inner = fixed_token_chunks(sent, s);
            out.extend(inner);
            continue;
        }
        if current_tokens + toks > s.max_tokens && !current.is_empty() {
            out.push(core::mem::take(&mut current));
            current_tokens = 0;
            apply_overlap(&out, s.overlap, &mut current, &mut current_tokens);
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(sent);
        current_tokens += toks;
    }

    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Tail-overlap helper: when a chunk closes, optionally seed the next
/// chunk with the trailing `overlap` tokens of the previous one.
fn apply_overlap(out: &[String], overlap: usize, current: &mut String, current_tokens: &mut usize) {
    if overlap == 0 {
        return;
    }
    if let Some(last) = out.last() {
        let words: Vec<&str> = last.unicode_words().collect();
        let want_w = ((overlap as f32) / 1.3).floor() as usize;
        let take_from = words.len().saturating_sub(want_w);
        let tail = words[take_from..].join(" ");
        if !tail.is_empty() {
            current.push_str(&tail);
            *current_tokens = approx_tokens(words.len() - take_from);
        }
    }
}

fn recursive_chunks(input: &str, s: &ChunkingStrategy) -> Vec<String> {
    // Paragraph split.
    let paragraphs: Vec<&str> = input
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if paragraphs.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current = String::new();
    let mut current_tokens = 0_usize;

    for para in paragraphs {
        let toks = approx_tokens(word_count(para));
        if toks > s.max_tokens {
            // Long paragraph — recurse via sentence splitter.
            if !current.is_empty() {
                out.push(core::mem::take(&mut current));
                current_tokens = 0;
            }
            let inner = sentence_chunks(para, s);
            out.extend(inner);
            continue;
        }
        if current_tokens + toks > s.max_tokens && !current.is_empty() {
            out.push(core::mem::take(&mut current));
            current_tokens = 0;
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
        current_tokens += toks;
    }

    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_empty() {
        let s = ChunkingStrategy::default();
        assert!(chunk_for_model("", &s).is_empty());
    }

    #[test]
    fn short_input_one_chunk() {
        let s = ChunkingStrategy::default();
        let chunks = chunk_for_model("hello world", &s);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("hello"));
    }

    #[test]
    fn fixed_tokens_respects_cap() {
        let words: alloc::vec::Vec<alloc::string::String> =
            (0..1000).map(|i| alloc::format!("w{i}")).collect();
        let text = words.join(" ");
        let s = ChunkingStrategy {
            max_tokens: 50,
            overlap: 0,
            mode: ChunkMode::FixedTokens,
        };
        let chunks = chunk_for_model(&text, &s);
        assert!(!chunks.is_empty());
        for c in &chunks {
            let toks = approx_tokens(word_count(c));
            assert!(toks <= s.max_tokens + 1, "chunk too large: {toks}");
        }
    }

    #[test]
    fn fixed_tokens_overlap_works() {
        let words: alloc::vec::Vec<alloc::string::String> =
            (0..50).map(|i| alloc::format!("w{i}")).collect();
        let text = words.join(" ");
        let s = ChunkingStrategy {
            max_tokens: 20,
            overlap: 5,
            mode: ChunkMode::FixedTokens,
        };
        let chunks = chunk_for_model(&text, &s);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn sentence_bounded_packs_short_sentences() {
        let s = ChunkingStrategy {
            max_tokens: 1000,
            overlap: 0,
            mode: ChunkMode::SentenceBounded,
        };
        let chunks = chunk_for_model("Alpha. Beta. Gamma.", &s);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn sentence_bounded_splits_when_needed() {
        let s = ChunkingStrategy {
            max_tokens: 5,
            overlap: 0,
            mode: ChunkMode::SentenceBounded,
        };
        let text = "First sentence here. Second sentence here. Third sentence.";
        let chunks = chunk_for_model(text, &s);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn recursive_respects_paragraphs() {
        let s = ChunkingStrategy {
            max_tokens: 1000,
            overlap: 0,
            mode: ChunkMode::Recursive,
        };
        let text = "Para one.\n\nPara two.\n\nPara three.";
        let chunks = chunk_for_model(text, &s);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("Para one"));
        assert!(chunks[0].contains("Para three"));
    }

    #[test]
    fn recursive_falls_back_for_long_paragraph() {
        let words: alloc::vec::Vec<alloc::string::String> =
            (0..500).map(|i| alloc::format!("w{i}")).collect();
        let text = words.join(" ");
        let s = ChunkingStrategy {
            max_tokens: 50,
            overlap: 0,
            mode: ChunkMode::Recursive,
        };
        let chunks = chunk_for_model(&text, &s);
        assert!(chunks.len() > 1);
    }
}
