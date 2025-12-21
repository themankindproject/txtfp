//! HTML → plain text via [`html2text`].
//!
//! `<script>` and `<style>` regions are excised before the conversion
//! so their bodies (typically minified JavaScript or CSS) cannot bleed
//! into the fingerprint surface.

use crate::error::{Error, Result};

/// Convert HTML source to plain text.
///
/// Returns the visible text content with lightweight structural cues
/// (paragraph breaks, list-item bullets, link target footnotes) but
/// **no hard wrapping** — the rendering width is `usize::MAX`, so
/// downstream canonicalization sees the natural line structure rather
/// than artificial breaks.
///
/// The input must be decoded UTF-8. If you have raw bytes, decode them
/// before calling this function (e.g. via the `encoding_rs` crate for
/// HTML-spec-compliant decoding).
pub fn html_to_text(html: &str) -> Result<String> {
    let cleaned = strip_script_and_style(html);
    let bytes = cleaned.as_bytes();
    html2text::from_read(bytes, usize::MAX)
        .map_err(|e| Error::InvalidInput(alloc::format!("html parse error: {e}")))
}

/// Strip `<script>...</script>` and `<style>...</style>` regions from
/// the HTML source, leaving the surrounding markup intact.
///
/// Naive but adequate for fingerprinting: real HTML parsers (`scraper`,
/// `html5ever`) round-trip the DOM through allocation-heavy machinery
/// that we do not need at this layer. The cost of a missed pathological
/// case is a noisier fingerprint, not a security issue.
fn strip_script_and_style(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;

    while !rest.is_empty() {
        // Lower-case search so `<SCRIPT>` and `<Script>` are handled.
        let lower = rest.to_ascii_lowercase();
        let next_script = lower.find("<script");
        let next_style = lower.find("<style");

        let next = match (next_script, next_style) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(a), None) => Some(a),
            (None, Some(b)) => Some(b),
            (None, None) => None,
        };

        match next {
            None => {
                out.push_str(rest);
                break;
            }
            Some(idx) => {
                out.push_str(&rest[..idx]);
                let after = &rest[idx..];
                let lower_after = lower[idx..].to_owned();
                let close = if lower_after.starts_with("<script") {
                    "</script>"
                } else {
                    "</style>"
                };
                if let Some(end) = lower_after.find(close) {
                    let resume = idx + end + close.len();
                    rest = &rest[resume..];
                } else {
                    // Unclosed block — drop to end-of-input.
                    let _ = after;
                    break;
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paragraph() {
        let s = html_to_text("<p>hello world</p>").unwrap();
        assert!(s.contains("hello world"));
    }

    #[test]
    fn strips_script_block() {
        let s = html_to_text("<p>visible</p><script>alert(1)</script>").unwrap();
        assert!(s.contains("visible"));
        assert!(!s.contains("alert"));
    }

    #[test]
    fn strips_style_block() {
        let s = html_to_text("<style>.x { color: red; }</style><p>visible</p>").unwrap();
        assert!(s.contains("visible"));
        assert!(!s.contains("color"));
    }

    #[test]
    fn strips_uppercase_script() {
        let s = html_to_text("<P>visible</P><SCRIPT>secret</SCRIPT>").unwrap();
        assert!(s.contains("visible"));
        assert!(!s.contains("secret"));
    }

    #[test]
    fn strips_multiple_blocks() {
        let s = html_to_text(
            "<p>a</p><script>x</script><p>b</p><style>y</style><p>c</p>",
        )
        .unwrap();
        assert!(s.contains('a'));
        assert!(s.contains('b'));
        assert!(s.contains('c'));
        assert!(!s.contains('x'));
        assert!(!s.contains('y'));
    }

    #[test]
    fn empty_input() {
        let s = html_to_text("").unwrap();
        assert_eq!(s.trim(), "");
    }

    #[test]
    fn entities_decoded() {
        let s = html_to_text("<p>caf&eacute; &amp; co</p>").unwrap();
        assert!(s.contains("café"));
        assert!(s.contains('&'));
    }

    #[test]
    fn unclosed_script_is_dropped() {
        let s = html_to_text("<p>visible</p><script>never closed").unwrap();
        assert!(s.contains("visible"));
    }
}
