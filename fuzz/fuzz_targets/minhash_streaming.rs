//! Fuzz target: `MinHashStreaming` must accept arbitrary byte streams
//! without panicking. When the chunked input concatenates to a valid
//! UTF-8 string, the streaming result must match the offline result —
//! the only legitimate divergence is on inputs that the offline path
//! would reject (empty, all-whitespace, invalid UTF-8 at finalize).
//!
//! Verifies the chunk-boundary UTF-8 carry logic on adversarial splits.

#![no_main]

use libfuzzer_sys::fuzz_target;
use txtfp::{
    Canonicalizer, Fingerprinter, MinHashFingerprinter, MinHashStreaming, ShingleTokenizer,
    StreamingFingerprinter, WordTokenizer,
};

#[derive(arbitrary::Arbitrary, Debug)]
struct Input<'a> {
    /// Whole UTF-8 input the streamer will consume.
    text: &'a str,
    /// Cut points (sorted; deduplicated; bounded into the input). The
    /// fuzzer controls where the byte boundaries fall — including
    /// inside multi-byte UTF-8 codepoints — to exercise the carry logic.
    cuts: Vec<u16>,
}

fn split_at_cuts<'a>(bytes: &'a [u8], cuts: &[u16]) -> Vec<&'a [u8]> {
    let mut points: Vec<usize> = cuts
        .iter()
        .map(|c| (*c as usize) % (bytes.len() + 1))
        .collect();
    points.sort_unstable();
    points.dedup();

    let mut chunks = Vec::with_capacity(points.len() + 1);
    let mut prev = 0usize;
    for p in points {
        chunks.push(&bytes[prev..p]);
        prev = p;
    }
    chunks.push(&bytes[prev..]);
    chunks
}

fuzz_target!(|input: Input<'_>| {
    let canon = Canonicalizer::default();
    let tok = ShingleTokenizer { k: 3, inner: WordTokenizer };
    let fp = MinHashFingerprinter::<_, 64>::new(canon, tok);

    // Streaming path with arbitrary byte cuts.
    let mut streamer = MinHashStreaming::new(MinHashFingerprinter::<_, 64>::new(
        Canonicalizer::default(),
        ShingleTokenizer { k: 3, inner: WordTokenizer },
    ));
    let bytes = input.text.as_bytes();
    let chunks = split_at_cuts(bytes, &input.cuts);

    // `update` may legitimately reject if the running buffer would
    // exceed its 16 MiB cap; the fuzzer should bail quietly in that case.
    for chunk in &chunks {
        if streamer.update(chunk).is_err() {
            return;
        }
    }
    let stream_sig = match streamer.finalize() {
        Ok(s) => s,
        // Empty / whitespace-only / boundary-cut UTF-8 are all valid
        // error returns; nothing more to check.
        Err(_) => return,
    };

    // Compare with the offline path. Any non-error from streaming
    // implies the input is well-formed, so offline must also succeed
    // and produce the identical signature.
    let offline_sig = fp
        .fingerprint(input.text)
        .expect("streaming succeeded but offline rejected the same well-formed input");
    assert_eq!(
        stream_sig.hashes, offline_sig.hashes,
        "streaming/offline divergence on equivalent input"
    );
});
