//! HTML and Markdown → plain text helpers, behind the `markup` feature.
//!
//! These helpers are I/O-free at the crate boundary: they take the
//! decoded `&str` (HTML or Markdown source) and return owned plain
//! text suitable for canonicalization and fingerprinting. Reading
//! from disk, decompressing, and content-sniffing are the caller's
//! responsibility.

mod html;
mod markdown;

pub use html::html_to_text;
pub use markdown::{MarkdownOptions, markdown_to_text, markdown_to_text_with};
