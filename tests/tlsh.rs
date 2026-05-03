//! TLSH end-to-end: sketch realistic multi-paragraph corpora, rank by
//! distance, exercise canonicalization integration, sanity-check the
//! public `TLSH_MIN_INPUT_BYTES` constant and the `Default` impl.
//!
//! Per-symbol unit tests live alongside the implementation in
//! `src/classical/tlsh.rs`. This file covers the *integration surface*
//! a downstream user actually touches.

#![cfg(feature = "tlsh")]

use txtfp::{Canonicalizer, Fingerprinter, TLSH_MIN_INPUT_BYTES, TlshFingerprinter, tlsh_distance};

const ALPHA: &str = "the quick brown fox jumps over the lazy dog at noon today
the slow grey wolf creeps under the loud ravens at dusk
astronomers detect cosmic background radiation everywhere they look";

const ALPHA_SMALL_EDIT: &str = "the quick brown fox jumps over the lazy dog at dusk today
the slow grey wolf creeps under the loud ravens at dawn
astronomers detect cosmic background radiation everywhere they look";

const BETA_UNRELATED: &str = "lorem ipsum dolor sit amet consectetur adipiscing elit
sed do eiusmod tempor incididunt ut labore et dolore magna aliqua
ut enim ad minim veniam quis nostrud exercitation ullamco laboris";

#[test]
fn min_input_bytes_constant_matches_runtime_threshold() {
    let fp = TlshFingerprinter::default();
    let too_short = "x".repeat(TLSH_MIN_INPUT_BYTES - 1);
    assert!(fp.fingerprint(&too_short).is_err());

    // Just-at-threshold bytes is implementation-dependent (TLSH wants
    // entropy too), but anything shorter must always be rejected.
}

#[test]
fn identical_inputs_have_zero_distance() {
    let fp = TlshFingerprinter::default();
    let a = fp.fingerprint(ALPHA).unwrap();
    let b = fp.fingerprint(ALPHA).unwrap();
    assert_eq!(tlsh_distance(&a, &b).unwrap(), 0);
}

#[test]
fn similar_inputs_rank_below_unrelated() {
    let fp = TlshFingerprinter::default();
    let a = fp.fingerprint(ALPHA).unwrap();
    let near = fp.fingerprint(ALPHA_SMALL_EDIT).unwrap();
    let far = fp.fingerprint(BETA_UNRELATED).unwrap();

    let d_near = tlsh_distance(&a, &near).unwrap();
    let d_far = tlsh_distance(&a, &far).unwrap();

    assert!(
        d_near < d_far,
        "expected small-edit distance ({d_near}) < unrelated distance ({d_far})"
    );
}

#[test]
fn canonicalization_collapses_case_difference() {
    // The default canonicalizer applies simple casefold, so an
    // upper-case copy of the same text should produce the same hash
    // (up to the final byte representation) and hence distance 0.
    let fp = TlshFingerprinter::default();
    let lower = fp.fingerprint(ALPHA).unwrap();
    let upper_input = ALPHA.to_uppercase();
    let upper = fp.fingerprint(&upper_input).unwrap();
    assert_eq!(
        tlsh_distance(&lower, &upper).unwrap(),
        0,
        "casefold should make upper/lower equivalent"
    );
}

#[test]
fn sketch_bytes_skips_canonicalization() {
    // sketch_bytes is the raw-bytes path; it does NOT run the
    // canonicalizer. Two inputs that differ only by case should
    // therefore produce different (or at least non-zero-distance)
    // fingerprints under sketch_bytes — proving the path is the raw
    // one, not silently re-canonicalizing.
    let fp = TlshFingerprinter::new(Canonicalizer::default());
    let lower = fp.sketch_bytes(ALPHA.as_bytes()).unwrap();
    let upper_input = ALPHA.to_uppercase();
    let upper = fp.sketch_bytes(upper_input.as_bytes()).unwrap();

    let d = tlsh_distance(&lower, &upper).unwrap();
    assert!(
        d > 0,
        "sketch_bytes should not canonicalize; expected non-zero distance, got {d}"
    );
}

#[test]
fn default_constructor_matches_explicit_canonicalizer() {
    let a_fp = TlshFingerprinter::default();
    let b_fp = TlshFingerprinter::new(Canonicalizer::default());

    let a = a_fp.fingerprint(ALPHA).unwrap();
    let b = b_fp.fingerprint(ALPHA).unwrap();

    assert_eq!(tlsh_distance(&a, &b).unwrap(), 0);
}
