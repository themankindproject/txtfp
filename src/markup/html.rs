//! HTML → plain text via [`html2text`].
//!
//! `<script>` and `<style>` regions are excised before the conversion
//! so their bodies (typically minified JavaScript or CSS) cannot bleed
//! into the fingerprint surface.

use crate::error::{Error, Result};

/// Convert HTML source to plain text.
///
/// Strips `<script>` and `<style>` regions before parsing so their
/// minified-JS / CSS bodies don't leak into the fingerprint surface.
/// The rendering width is `usize::MAX`, so the output preserves the
/// document's natural line structure rather than imposing artificial
/// hard wraps.
///
/// # Arguments
///
/// * `html` — UTF-8 HTML source. Decode raw bytes first via
///   `encoding_rs` if you have non-UTF-8 input.
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] when the HTML parser rejects the
/// input. Malformed-but-recoverable HTML is parsed leniently per the
/// HTML5 spec.
///
/// # Example
///
/// ```
/// # #[cfg(feature = "markup")]
/// # fn demo() -> Result<(), txtfp::Error> {
/// use txtfp::html_to_text;
///
/// let plain = html_to_text("<p>visible</p><script>secret</script>")?;
/// assert!(plain.contains("visible"));
/// assert!(!plain.contains("secret"));
/// # Ok(()) }
/// ```
pub fn html_to_text(html: &str) -> Result<String> {
    let cleaned = strip_script_and_style(html);
    let bytes = cleaned.as_bytes();
    html2text::from_read(bytes, usize::MAX)
        .map_err(|e| Error::InvalidInput(alloc::format!("html parse error: {e}")))
}

/// Strip `<script>...</script>` and `<style>...</style>` regions from
/// the HTML source, leaving the surrounding markup intact.
///
/// Single-pass linear scan with zero intermediate allocations: tag names
/// are matched case-insensitively and must be followed by a real
/// tag-name boundary (whitespace, `/`, `>`, or end-of-input), so prose
/// mentioning `<scripture>` or `<styling>` is left untouched and
/// `</script >`-style closers are honoured. This is adequate for
/// fingerprinting — real HTML parsers (`scraper`, `html5ever`)
/// round-trip the DOM through allocation-heavy machinery that we do not
/// need at this layer, and the cost of a missed pathological case is a
/// noisier fingerprint, not a security issue.
fn strip_script_and_style(html: &str) -> String {
    const OPEN_SCRIPT: &[u8] = b"<script";
    const OPEN_STYLE: &[u8] = b"<style";
    const CLOSE_SCRIPT: &[u8] = b"</script";
    const CLOSE_STYLE: &[u8] = b"</style";

    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut region_start = 0;
    let mut i = 0;

    while i < bytes.len() {
        let (name_len, close_pat) = if starts_with_ignore_case(&bytes[i..], OPEN_SCRIPT) {
            (OPEN_SCRIPT.len(), CLOSE_SCRIPT)
        } else if starts_with_ignore_case(&bytes[i..], OPEN_STYLE) {
            (OPEN_STYLE.len(), CLOSE_STYLE)
        } else {
            (0, b"" as &[u8])
        };

        if name_len == 0 || !is_tag_name_end(bytes.get(i + name_len).copied()) {
            i += 1;
            continue;
        }

        out.push_str(&html[region_start..i]);

        match find_close_tag(bytes, i + name_len, close_pat) {
            Some(end) => {
                // `end` is one past the closing `>`, which is always an
                // ASCII byte and therefore a UTF-8 char boundary.
                region_start = end;
                i = end;
            }
            None => {
                // Unclosed block — drop to end-of-input.
                region_start = bytes.len();
                i = bytes.len();
            }
        }
    }

    if region_start < bytes.len() {
        out.push_str(&html[region_start..]);
    }
    out
}

/// Case-insensitive prefix test on raw bytes.
#[inline]
fn starts_with_ignore_case(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.len() >= needle.len() && haystack[..needle.len()].eq_ignore_ascii_case(needle)
}

/// True if the byte after a tag name terminates the name: whitespace,
/// `/`, `>`, or end-of-input. Anything else (e.g. the `u` in
/// `<scripture>`) means the text is not a tag.
#[inline]
fn is_tag_name_end(b: Option<u8>) -> bool {
    matches!(b, None | Some(b' ' | b'\t' | b'\n' | b'\r' | b'/' | b'>'))
}

/// Scan from `from` for the closing tag `pat` (e.g. `b"</script"`).
///
/// The closer must itself be name-boundary-terminated and flushed with
/// `>` (allowing `</script >`). Returns the index one past the `>`, or
/// `None` if no well-formed closer exists.
fn find_close_tag(bytes: &[u8], from: usize, pat: &[u8]) -> Option<usize> {
    let mut j = from;
    while j + pat.len() <= bytes.len() {
        if starts_with_ignore_case(&bytes[j..], pat)
            && is_tag_name_end(bytes.get(j + pat.len()).copied())
        {
            let mut k = j + pat.len();
            while k < bytes.len() && matches!(bytes[k], b' ' | b'\t' | b'\n' | b'\r') {
                k += 1;
            }
            if k < bytes.len() && bytes[k] == b'>' {
                return Some(k + 1);
            }
        }
        j += 1;
    }
    None
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
        let s = html_to_text("<p>a</p><script>x</script><p>b</p><style>y</style><p>c</p>").unwrap();
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

    #[test]
    fn scripture_word_is_not_an_open_tag() {
        // Regression: bare prefix matching treated `<scripture>` as an
        // open tag and dropped the document tail. html2text renders
        // unknown-element content (tag names themselves are dropped),
        // so the signal is that everything after `<scripture` survives.
        let s = html_to_text("<p>mention <scripture>holy text</scripture> here</p>").unwrap();
        assert!(s.contains("holy text"));
        assert!(s.contains("here"));
    }

    #[test]
    fn styling_word_is_not_a_style_tag() {
        let s = html_to_text("a <styling>update</styling> b").unwrap();
        assert!(s.contains("update"));
        assert!(s.contains('b'));
    }

    #[test]
    fn open_tag_with_attribute_is_stripped() {
        let s = html_to_text("<script src=\"x.js\">var secret</script>visible").unwrap();
        assert!(!s.contains("secret"));
        assert!(s.contains("visible"));
    }

    #[test]
    fn close_tag_with_whitespace_is_stripped() {
        let s = html_to_text("<script>secret</script >visible").unwrap();
        assert!(!s.contains("secret"));
        assert!(s.contains("visible"));
    }

    #[test]
    fn strip_leaves_tail_after_script() {
        let s = html_to_text("<script>secret</script><p>kept</p>").unwrap();
        assert!(!s.contains("secret"));
        assert!(s.contains("kept"));
    }
}
