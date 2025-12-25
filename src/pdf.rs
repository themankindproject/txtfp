//! PDF → plain text helper, behind the `pdf` feature.
//!
//! Wraps `pdf-extract` with safety caps (max-bytes, control-byte
//! sanitization) so a hostile PDF cannot exhaust memory or smuggle
//! null bytes into the downstream tokenizer.
//!
//! Implementation lands in v0.1.1.

use alloc::string::String;

use crate::error::{Error, Result};

/// Default cap on input PDF size, in bytes (50 MiB).
pub const DEFAULT_MAX_BYTES: usize = 50 * 1024 * 1024;

/// Convert a PDF byte buffer to plain text.
///
/// Rejects inputs larger than [`DEFAULT_MAX_BYTES`]. Replaces NUL
/// bytes (`\u{0}`) in the extracted text with U+FFFD before returning.
pub fn pdf_to_text(bytes: &[u8]) -> Result<String> {
    pdf_to_text_with_cap(bytes, DEFAULT_MAX_BYTES)
}

/// Like [`pdf_to_text`] but with a caller-supplied size cap.
pub fn pdf_to_text_with_cap(bytes: &[u8], max_bytes: usize) -> Result<String> {
    if bytes.len() > max_bytes {
        return Err(Error::InvalidInput(alloc::format!(
            "pdf input exceeds {max_bytes}-byte cap"
        )));
    }
    let raw = pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| Error::InvalidInput(alloc::format!("pdf parse error: {e}")))?;
    Ok(raw.replace('\u{0}', "\u{FFFD}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_input() {
        let big = alloc::vec![0u8; 1024];
        let r = pdf_to_text_with_cap(&big, 100);
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn invalid_pdf_errors_cleanly() {
        let r = pdf_to_text(b"not a pdf");
        assert!(r.is_err());
    }
}
