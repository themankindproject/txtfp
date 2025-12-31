//! PDF → plain text helper, behind the `pdf` feature.
//!
//! Wraps `pdf-extract` with three production safeguards:
//!
//! 1. **Size cap**: rejects byte buffers larger than
//!    [`DEFAULT_MAX_BYTES`] (50 MiB by default) before invoking the
//!    parser.
//! 2. **Wall-clock timeout**: aborts extraction if `pdf-extract` does
//!    not return within [`DEFAULT_TIMEOUT_SECS`] (30 s by default).
//!    Hostile or pathologically-structured PDFs occasionally trip the
//!    parser into super-linear behavior; without a timeout an
//!    ingestion pipeline hangs indefinitely.
//! 3. **NUL sanitization**: replaces any extracted `\u{0}` with U+FFFD
//!    so downstream tokenizers don't treat embedded nulls as input
//!    boundaries.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::error::{Error, Result};

/// Default cap on input PDF size, in bytes (50 MiB).
pub const DEFAULT_MAX_BYTES: usize = 50 * 1024 * 1024;

/// Default wall-clock timeout, in seconds (30 s).
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Knobs for [`pdf_to_text_with`].
#[derive(Copy, Clone, Debug)]
pub struct PdfOptions {
    /// Maximum input size, in bytes. Inputs larger than this return
    /// [`Error::InvalidInput`].
    pub max_bytes: usize,
    /// Maximum wall-clock seconds to wait for `pdf-extract` to return.
    /// Exceeding this returns [`Error::InvalidInput`].
    pub timeout_secs: u64,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }
}

/// Convert a PDF byte buffer to plain text, applying [`PdfOptions::default`].
///
/// # Arguments
///
/// * `bytes` — raw PDF bytes (including the `%PDF-` header).
///
/// # Errors
///
/// Returns [`Error::InvalidInput`] when:
/// - `bytes.len()` exceeds [`DEFAULT_MAX_BYTES`] (50 MiB),
/// - parsing exceeds [`DEFAULT_TIMEOUT_SECS`] (30 s) wall clock,
/// - the input is malformed PDF, or
/// - the parser thread panicked.
///
/// # Example
///
/// ```no_run
/// # #[cfg(feature = "pdf")]
/// # fn demo() -> Result<(), txtfp::Error> {
/// use txtfp::pdf_to_text;
///
/// let bytes = std::fs::read("doc.pdf")?;
/// let text = pdf_to_text(&bytes)?;
/// # Ok(()) }
/// ```
pub fn pdf_to_text(bytes: &[u8]) -> Result<String> {
    pdf_to_text_with(bytes, PdfOptions::default())
}

/// Like [`pdf_to_text`] but with caller-supplied options.
///
/// # Arguments
///
/// * `bytes` — raw PDF bytes.
/// * `opts` — caps for input size and parse time. See [`PdfOptions`].
///
/// # Errors
///
/// See [`pdf_to_text`]; the cap and timeout values are
/// `opts.max_bytes` / `opts.timeout_secs` instead of the defaults.
///
/// # Example
///
/// ```no_run
/// # #[cfg(feature = "pdf")]
/// # fn demo() -> Result<(), txtfp::Error> {
/// use txtfp::{PdfOptions, pdf_to_text_with};
///
/// // Tighter ingest path: 5 MiB max, 10 s timeout.
/// let opts = PdfOptions { max_bytes: 5 * 1024 * 1024, timeout_secs: 10 };
/// let bytes = std::fs::read("untrusted.pdf")?;
/// let text = pdf_to_text_with(&bytes, opts)?;
/// # Ok(()) }
/// ```
pub fn pdf_to_text_with(bytes: &[u8], opts: PdfOptions) -> Result<String> {
    if bytes.len() > opts.max_bytes {
        return Err(Error::InvalidInput(alloc::format!(
            "pdf input ({} bytes) exceeds {}-byte cap",
            bytes.len(),
            opts.max_bytes
        )));
    }

    let raw = run_with_timeout(bytes, Duration::from_secs(opts.timeout_secs))?;
    Ok(sanitize(&raw))
}

/// Run `pdf-extract::extract_text_from_mem` on a worker thread,
/// aborting the wait at `timeout`.
///
/// We can't preempt the worker — Rust threads aren't cancellable —
/// but the timeout caps how long the *caller* blocks. The orphaned
/// worker finishes in the background and is reaped by the runtime.
fn run_with_timeout(bytes: &[u8], timeout: Duration) -> Result<String> {
    // `pdf-extract` takes a byte slice. We need an owned buffer to ship
    // to the worker thread, so we copy once. Cost is bounded by the
    // already-enforced size cap.
    let buf: Vec<u8> = bytes.to_vec();
    let (tx, rx) = mpsc::channel::<Result<String>>();
    let _handle = thread::spawn(move || {
        let r = pdf_extract::extract_text_from_mem(&buf)
            .map_err(|e| Error::InvalidInput(alloc::format!("pdf parse error: {e}")));
        // The receiver may have given up; ignore the send error.
        let _ = tx.send(r);
    });

    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(Error::InvalidInput(alloc::format!(
            "pdf parse exceeded {}-second timeout",
            timeout.as_secs()
        ))),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(Error::InvalidInput("pdf parser panicked".to_string()))
        }
    }
}

/// Replace embedded NULs with U+FFFD.
fn sanitize(text: &str) -> String {
    if !text.contains('\u{0}') {
        return text.to_owned();
    }
    text.replace('\u{0}', "\u{FFFD}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_oversized_input() {
        let big = alloc::vec![0u8; 1024];
        let r = pdf_to_text_with(
            &big,
            PdfOptions {
                max_bytes: 100,
                timeout_secs: 30,
            },
        );
        assert!(matches!(r, Err(Error::InvalidInput(_))));
    }

    #[test]
    fn invalid_pdf_errors_cleanly() {
        let r = pdf_to_text(b"not a pdf");
        assert!(r.is_err());
    }

    #[test]
    fn sanitize_replaces_nul() {
        assert_eq!(sanitize("a\u{0}b"), "a\u{FFFD}b");
        assert_eq!(sanitize("plain"), "plain");
    }

    #[test]
    fn defaults_are_documented_constants() {
        let o = PdfOptions::default();
        assert_eq!(o.max_bytes, DEFAULT_MAX_BYTES);
        assert_eq!(o.timeout_secs, DEFAULT_TIMEOUT_SECS);
    }
}
