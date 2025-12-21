//! Markdown → plain text via [`pulldown_cmark`].

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use crate::error::Result;

/// Knobs for [`markdown_to_text_with`].
#[derive(Copy, Clone, Debug)]
pub struct MarkdownOptions {
    /// Include the body of fenced/indented code blocks. Default `true`.
    /// Disable to focus the fingerprint on prose.
    pub include_code_blocks: bool,
    /// Include inline code spans. Default `true`.
    pub include_inline_code: bool,
    /// Insert a single space at soft and hard breaks (vs nothing). Default
    /// `true` — without spaces, breaking lines mid-sentence corrupts the
    /// downstream tokenizer.
    pub break_to_space: bool,
}

impl Default for MarkdownOptions {
    fn default() -> Self {
        Self {
            include_code_blocks: true,
            include_inline_code: true,
            break_to_space: true,
        }
    }
}

/// Convert Markdown source to plain text using default options.
pub fn markdown_to_text(md: &str) -> Result<String> {
    markdown_to_text_with(md, MarkdownOptions::default())
}

/// Convert Markdown source to plain text with caller-supplied options.
pub fn markdown_to_text_with(md: &str, opts: MarkdownOptions) -> Result<String> {
    let parser = Parser::new(md);
    let mut out = String::with_capacity(md.len());
    let mut in_code_block = false;
    let mut last_was_break = true;

    for ev in parser {
        match ev {
            Event::Start(Tag::CodeBlock(_)) => {
                in_code_block = true;
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                if opts.break_to_space && !last_was_break {
                    out.push(' ');
                    last_was_break = true;
                }
            }
            Event::Start(Tag::Paragraph)
            | Event::Start(Tag::Item)
            | Event::Start(Tag::Heading { .. }) => {
                if opts.break_to_space && !last_was_break {
                    out.push(' ');
                    last_was_break = true;
                }
            }
            Event::End(TagEnd::Paragraph)
            | Event::End(TagEnd::Item)
            | Event::End(TagEnd::Heading(_)) => {
                if opts.break_to_space && !last_was_break {
                    out.push(' ');
                    last_was_break = true;
                }
            }
            Event::Text(s) => {
                if in_code_block && !opts.include_code_blocks {
                    continue;
                }
                out.push_str(&s);
                last_was_break = false;
            }
            Event::Code(s) => {
                if !opts.include_inline_code {
                    continue;
                }
                out.push_str(&s);
                last_was_break = false;
            }
            Event::SoftBreak | Event::HardBreak => {
                if opts.break_to_space && !last_was_break {
                    out.push(' ');
                    last_was_break = true;
                }
            }
            _ => {}
        }
    }

    Ok(out.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paragraph() {
        let s = markdown_to_text("hello world").unwrap();
        assert_eq!(s, "hello world");
    }

    #[test]
    fn headings_are_text() {
        let s = markdown_to_text("# Heading\n\nbody").unwrap();
        assert!(s.contains("Heading"));
        assert!(s.contains("body"));
    }

    #[test]
    fn code_block_default_included() {
        let s = markdown_to_text("```\nlet x = 1;\n```\n").unwrap();
        assert!(s.contains("let x = 1"));
    }

    #[test]
    fn code_block_can_be_excluded() {
        let opts = MarkdownOptions {
            include_code_blocks: false,
            ..Default::default()
        };
        let s = markdown_to_text_with("text\n\n```\nlet x = 1;\n```\n\nmore", opts).unwrap();
        assert!(s.contains("text"));
        assert!(s.contains("more"));
        assert!(!s.contains("let x"));
    }

    #[test]
    fn inline_code_included_by_default() {
        let s = markdown_to_text("use the `frobnicate` function").unwrap();
        assert!(s.contains("frobnicate"));
    }

    #[test]
    fn inline_code_can_be_excluded() {
        let opts = MarkdownOptions {
            include_inline_code: false,
            ..Default::default()
        };
        let s = markdown_to_text_with("call `secret` now", opts).unwrap();
        assert!(s.contains("call"));
        assert!(s.contains("now"));
        assert!(!s.contains("secret"));
    }

    #[test]
    fn list_items() {
        let s = markdown_to_text("- one\n- two\n- three").unwrap();
        assert!(s.contains("one"));
        assert!(s.contains("two"));
        assert!(s.contains("three"));
    }

    #[test]
    fn link_text_kept() {
        let s = markdown_to_text("[click here](https://example.com)").unwrap();
        assert!(s.contains("click here"));
    }

    #[test]
    fn empty_input() {
        let s = markdown_to_text("").unwrap();
        assert_eq!(s, "");
    }

    #[test]
    fn break_to_space_off_yields_concat() {
        let opts = MarkdownOptions {
            break_to_space: false,
            ..Default::default()
        };
        let s = markdown_to_text_with("alpha\nbeta", opts).unwrap();
        assert_eq!(s, "alphabeta");
    }
}
