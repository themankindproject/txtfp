//! Tokenizers — split canonicalized text into the token stream that
//! feeds the classical fingerprinters.
//!
//! All tokenizers borrow from the input `&str`; they do not allocate per
//! token. The exception is [`ShingleTokenizer`], which materializes the
//! inner iterator into a `Vec<&str>` so it can yield k-grams; see its
//! docs for the cost.
//!
//! # Naming
//!
//! Each tokenizer's [`Tokenizer::name`] returns a stable identifier used
//! in [`crate::FingerprintMetadata`]. The format is fixed at v0.1.0:
//!
//! - `"word-uax29"` — [`WordTokenizer`]
//! - `"grapheme-uax29"` — [`GraphemeTokenizer`]
//! - `"shingle-k=<k>/<inner>"` — [`ShingleTokenizer<T>`]
//! - `"cjk-jieba"` / `"cjk-lindera"` — `CjkTokenizer` (`cjk` feature)

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::string::String;

mod grapheme;
mod shingle;
mod word;

#[cfg(feature = "cjk")]
#[cfg_attr(docsrs, doc(cfg(feature = "cjk")))]
mod cjk;

pub use grapheme::GraphemeTokenizer;
pub use shingle::ShingleTokenizer;
pub use word::WordTokenizer;

#[cfg(feature = "cjk")]
#[cfg_attr(docsrs, doc(cfg(feature = "cjk")))]
pub use cjk::{CjkSegmenter, CjkTokenizer};

/// Type-erased token iterator returned by [`Tokenizer::tokens`].
///
/// Borrowed-slice tokens are zero-allocation; CJK and shingle tokenizers
/// may yield owned segments because their outputs do not align to
/// substring slices of the input.
///
/// Most callers should prefer [`Tokenizer::for_each_token`] for
/// hash-then-discard kernels — it is zero-allocation across all
/// implementors. `tokens()` (and therefore `TokenStream`) is provided
/// for cases where the caller actually needs an `Iterator`.
pub enum TokenStream<'a> {
    /// Tokens that borrow from the input.
    Borrowed(Box<dyn Iterator<Item = &'a str> + Send + 'a>),
    /// Tokens that own their backing storage.
    Owned(Box<dyn Iterator<Item = String> + Send + 'a>),
}

impl<'a> TokenStream<'a> {
    /// Drive the stream to completion, yielding `String`s.
    ///
    /// # Returns
    ///
    /// A boxed iterator of owned `String`s. For [`TokenStream::Borrowed`]
    /// inputs this allocates one `String` per token; prefer
    /// [`Tokenizer::for_each_token`] for zero-allocation paths.
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::{Tokenizer, WordTokenizer};
    ///
    /// let toks: Vec<String> = WordTokenizer
    ///     .tokens("hello world!")
    ///     .into_string_iter()
    ///     .collect();
    /// assert_eq!(toks, ["hello", "world"]);
    /// ```
    pub fn into_string_iter(self) -> Box<dyn Iterator<Item = String> + Send + 'a> {
        match self {
            TokenStream::Borrowed(it) => Box::new(it.map(String::from)),
            TokenStream::Owned(it) => it,
        }
    }
}

/// Trait implemented by every tokenizer in the crate.
///
/// Tokenizers must be `Send + Sync` so they can be shared across worker
/// threads; they are typically zero-sized and `Copy`-friendly.
///
/// # `name` return type
///
/// `name` returns [`Cow<'static, str>`]: tokenizers with a fully static
/// identifier (e.g. [`WordTokenizer`]) return `Cow::Borrowed`; tokenizers
/// whose identifier depends on runtime configuration (e.g.
/// [`ShingleTokenizer`] with its `k`) return `Cow::Owned`. This lets us
/// avoid leaking heap memory in the no_std + alloc default-feature build
/// while still producing stable identifiers for [`crate::FingerprintMetadata`].
pub trait Tokenizer: Send + Sync {
    /// Yield the token stream for `input`.
    fn tokens<'a>(&'a self, input: &'a str) -> TokenStream<'a>;

    /// Stable identifier for this tokenizer; see the module docs.
    fn name(&self) -> Cow<'static, str>;

    /// Visit each token via callback. The closure receives a transient
    /// `&str` valid only during the call — perfect for hash-then-discard
    /// kernels (MinHash, SimHash) that don't need to persist tokens.
    ///
    /// Implementors should override this for zero-allocation paths;
    /// the default routes through [`Tokenizer::tokens`] and pays one
    /// `String` allocation per token.
    fn for_each_token(&self, input: &str, f: &mut dyn FnMut(&str)) {
        for tok in self.tokens(input).into_string_iter() {
            f(&tok);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    fn collect(stream: TokenStream<'_>) -> Vec<String> {
        stream.into_string_iter().collect()
    }

    #[test]
    fn word_borrowed_then_owned_yields_same_strings() {
        let w = WordTokenizer;
        let toks: Vec<String> = collect(w.tokens("hello world!"));
        assert_eq!(toks, ["hello", "world"]);
    }

    #[test]
    fn names_are_stable() {
        assert_eq!(WordTokenizer.name(), "word-uax29");
        assert_eq!(GraphemeTokenizer.name(), "grapheme-uax29");
        let s = ShingleTokenizer {
            k: 3,
            inner: WordTokenizer,
        };
        assert_eq!(s.name(), "shingle-k=3/word-uax29");
    }
}
