//! MinHash signature byte layout.

/// Schema version for [`MinHashSig`]. **Frozen for v0.1.x** — bumping
/// this is a major-version event.
pub const SCHEMA_VERSION: u16 = 1;

/// MinHash signature with `H` hash slots.
///
/// Represented as a fixed-size, repr(C), `bytemuck::Pod` struct so callers
/// can memory-map, persist, or zero-copy serialize collections of
/// signatures.
///
/// # Layout
///
/// ```text
/// offset 0..2   : u16 schema version (= [`SCHEMA_VERSION`])
/// offset 2..8   : 6 bytes padding (zeroed)
/// offset 8..    : H * u64 hash slots, little-endian
/// ```
///
/// Total size: `8 + 8 * H` bytes. 8-byte aligned.
///
/// # Stability
///
/// The byte layout above is **semver-frozen** as of v0.1.0.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct MinHashSig<const H: usize> {
    /// Schema version. Must equal [`SCHEMA_VERSION`].
    pub schema: u16,
    /// Padding to align `hashes` on an 8-byte boundary. Must be zero.
    pub _pad: [u8; 6],
    /// The H min-hash slots.
    pub hashes: [u64; H],
}

// SAFETY: `MinHashSig<H>` is `repr(C)` with all-Pod fields:
//   - `schema: u16` at offset 0..2 (Pod)
//   - `_pad: [u8; 6]` at offset 2..8 (Pod, fills the natural padding before
//     the u64 array so the layout has no implicit padding)
//   - `hashes: [u64; H]` at offset 8.. (each `u64` is Pod; `[u64; H]` is
//     contiguous with no padding between elements)
// The struct's alignment is 8 (max of field alignments). Total size is
// `8 + 8 * H` bytes with no end padding because the alignment divides the
// size. Bytemuck's derive macro can't verify this for arbitrary const-generic
// `H`, so we assert it manually.
unsafe impl<const H: usize> bytemuck::Zeroable for MinHashSig<H> {}
// SAFETY: see Zeroable above. Pod requires Copy + Zeroable + 'static + no
// padding + valid for all bit patterns; all hold here because every byte of
// the struct is part of a Pod field.
unsafe impl<const H: usize> bytemuck::Pod for MinHashSig<H> {}

impl<const H: usize> MinHashSig<H> {
    /// Construct an "all maxima" signature. Useful for tests and as the
    /// initial state of a streaming sketch (every slot collapses to the
    /// minimum observed during sketching).
    #[inline]
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            schema: SCHEMA_VERSION,
            _pad: [0; 6],
            hashes: [u64::MAX; H],
        }
    }

    /// Number of hash slots — equals the const generic `H`.
    #[inline]
    #[must_use]
    pub const fn slot_count(&self) -> usize {
        H
    }

    /// View the signature as a byte slice. Zero-copy.
    ///
    /// Useful for hashing the signature itself (e.g. to build a content-
    /// addressed cache key) or for serializing to disk.
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_signature_has_schema_set() {
        let s: MinHashSig<128> = MinHashSig::empty();
        assert_eq!(s.schema, SCHEMA_VERSION);
        assert_eq!(s._pad, [0; 6]);
        assert!(s.hashes.iter().all(|h| *h == u64::MAX));
    }

    #[test]
    fn slot_count_matches_const_generic() {
        let s: MinHashSig<64> = MinHashSig::empty();
        assert_eq!(s.slot_count(), 64);
    }

    #[test]
    fn pod_roundtrip_through_bytes() {
        let s: MinHashSig<8> = MinHashSig {
            schema: SCHEMA_VERSION,
            _pad: [0; 6],
            hashes: [1, 2, 3, 4, 5, 6, 7, 8],
        };
        let bytes = s.as_bytes();
        assert_eq!(bytes.len(), 8 + 8 * 8);
        // Deserialize back via bytemuck.
        let s2: MinHashSig<8> = *bytemuck::from_bytes(bytes);
        assert_eq!(s, s2);
    }

    #[test]
    fn schema_version_is_frozen() {
        assert_eq!(SCHEMA_VERSION, 1);
    }

    #[test]
    fn signatures_are_pod_eq_hash() {
        // Compile-time assertions via the type system.
        fn assert_pod<T: bytemuck::Pod>() {}
        fn assert_eq_hash<T: Eq + core::hash::Hash>() {}
        assert_pod::<MinHashSig<128>>();
        assert_eq_hash::<MinHashSig<128>>();
    }
}
