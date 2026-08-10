//! Fuzz target: `html_to_text` / `markdown_to_text` must never panic on
//! any valid UTF-8 input. Parser-reported errors are legitimate
//! outcomes; the invariant under test is panic-freedom.
//!
//! The HTML path funnels through `html5ever` / `html2text` — a real
//! parser — which is exactly the surface worth fuzzing.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|input: &str| {
    let _ = txtfp::html_to_text(input);
    let _ = txtfp::markdown_to_text(input);
});