//! Fuzz target: `Canonicalizer::canonicalize` must never panic on any
//! valid UTF-8 input, must return valid UTF-8, and must be idempotent
//! (`canonicalize(canonicalize(s)) == canonicalize(s)`).
//!
//! Closes the v0.1.0 changelog promise to ship cargo-fuzz harnesses.

#![no_main]

use libfuzzer_sys::fuzz_target;
use txtfp::Canonicalizer;

fuzz_target!(|input: &str| {
    let canon = Canonicalizer::default();
    let once = canon.canonicalize(input);

    // Output is always valid UTF-8 (`String` invariant); re-running the
    // canonicalizer over its own output must produce a fixed point.
    let twice = canon.canonicalize(&once);
    assert_eq!(once, twice, "canonicalize is not idempotent on input");
});
