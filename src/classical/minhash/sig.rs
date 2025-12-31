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
    /// Construct an "all-maxima" signature.
    ///
    /// Every slot is initialized to `u64::MAX` so a sketcher can collapse
    /// each slot toward the running minimum as tokens flow in. This is
    /// the documented initial state for both
    /// [`MinHashFingerprinter`](super::MinHashFingerprinter) and
    /// [`MinHashStreaming`](super::MinHashStreaming).
    ///
    /// # Returns
    ///
    /// `MinHashSig<H>` with `schema = SCHEMA_VERSION`, padding zeroed,
    /// and `hashes = [u64::MAX; H]`.
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::MinHashSig;
    ///
    /// let s: MinHashSig<128> = MinHashSig::empty();
    /// assert_eq!(s.schema, 1);
    /// assert!(s.hashes.iter().all(|h| *h == u64::MAX));
    /// ```
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
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::MinHashSig;
    ///
    /// assert_eq!(MinHashSig::<128>::empty().slot_count(), 128);
    /// assert_eq!(MinHashSig::<64>::empty().slot_count(), 64);
    /// ```
    #[inline]
    #[must_use]
    pub const fn slot_count(&self) -> usize {
        H
    }

    /// View the signature as a byte slice. Zero-copy.
    ///
    /// Useful for hashing the signature itself (e.g. to build a content-
    /// addressed cache key) or for serializing to disk. The returned
    /// bytes match the on-disk layout documented at the type level.
    ///
    /// # Returns
    ///
    /// A `&[u8]` of length `8 + 8 * H`. Bytes are little-endian.
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::MinHashSig;
    ///
    /// let s: MinHashSig<8> = MinHashSig::empty();
    /// let bytes = s.as_bytes();
    /// assert_eq!(bytes.len(), 8 + 8 * 8);
    ///
    /// // Zero-copy round-trip via bytemuck.
    /// let s2: MinHashSig<8> = *bytemuck::from_bytes(bytes);
    /// assert_eq!(s, s2);
    /// ```
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}

// ── Manual serde impls (const-generic arrays) ───────────────────────────
//
// `serde`'s blanket impls for `[T; N]` work, but `#[derive(Serialize,
// Deserialize)]` on a struct with a `[u64; H]` field generates a `where
// [u64; H]: Serialize` bound that doesn't always resolve under
// `default-features = false`. We hand-roll the impls so const-generic
// MinHash signatures round-trip through every serde format.

#[cfg(feature = "serde")]
const _: () = {
    use serde::de::{self, MapAccess, SeqAccess, Visitor};
    use serde::ser::{SerializeStruct, SerializeTuple};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    impl<const H: usize> Serialize for MinHashSig<H> {
        fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
            // Human-readable formats (JSON, YAML) get a struct with a
            // `hashes` array; binary formats (bincode) get a tight tuple
            // for size-optimal encoding.
            if ser.is_human_readable() {
                let mut s = ser.serialize_struct("MinHashSig", 2)?;
                s.serialize_field("schema", &self.schema)?;
                s.serialize_field("hashes", &SliceSer(&self.hashes[..]))?;
                s.end()
            } else {
                let mut t = ser.serialize_tuple(1 + H)?;
                t.serialize_element(&self.schema)?;
                for h in &self.hashes {
                    t.serialize_element(h)?;
                }
                t.end()
            }
        }
    }

    /// Helper that serializes a `&[u64]` as a sequence even though the
    /// underlying `[u64; H]` blanket impl would, because routing through
    /// it on `default-features = false` builds is unreliable.
    struct SliceSer<'a>(&'a [u64]);

    impl Serialize for SliceSer<'_> {
        fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
            use serde::ser::SerializeSeq;
            let mut s = ser.serialize_seq(Some(self.0.len()))?;
            for h in self.0 {
                s.serialize_element(h)?;
            }
            s.end()
        }
    }

    impl<'de, const H: usize> Deserialize<'de> for MinHashSig<H> {
        fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
            if de.is_human_readable() {
                de.deserialize_struct("MinHashSig", &["schema", "hashes"], StructVisitor::<H>)
            } else {
                de.deserialize_tuple(1 + H, TupleVisitor::<H>)
            }
        }
    }

    struct StructVisitor<const H: usize>;

    impl<'de, const H: usize> Visitor<'de> for StructVisitor<H> {
        type Value = MinHashSig<H>;

        fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "a MinHashSig<{H}> struct")
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut schema: Option<u16> = None;
            let mut hashes: Option<[u64; H]> = None;
            while let Some(key) = map.next_key::<alloc::string::String>()? {
                match key.as_str() {
                    "schema" => schema = Some(map.next_value()?),
                    "hashes" => hashes = Some(map.next_value::<HashesArray<H>>()?.0),
                    other => {
                        return Err(de::Error::unknown_field(other, &["schema", "hashes"]));
                    }
                }
            }
            let schema = schema.ok_or_else(|| de::Error::missing_field("schema"))?;
            let hashes = hashes.ok_or_else(|| de::Error::missing_field("hashes"))?;
            Ok(MinHashSig {
                schema,
                _pad: [0; 6],
                hashes,
            })
        }
    }

    struct HashesArray<const H: usize>([u64; H]);

    impl<'de, const H: usize> Deserialize<'de> for HashesArray<H> {
        fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
            de.deserialize_seq(HashesArrayVisitor::<H>)
        }
    }

    struct HashesArrayVisitor<const H: usize>;

    impl<'de, const H: usize> Visitor<'de> for HashesArrayVisitor<H> {
        type Value = HashesArray<H>;

        fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "an array of exactly {H} u64 hash slots")
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut out = [0_u64; H];
            for (i, slot) in out.iter_mut().enumerate() {
                *slot = seq
                    .next_element::<u64>()?
                    .ok_or_else(|| de::Error::invalid_length(i, &self))?;
            }
            // Reject excess elements.
            if seq.next_element::<u64>()?.is_some() {
                return Err(de::Error::invalid_length(H + 1, &self));
            }
            Ok(HashesArray(out))
        }
    }

    struct TupleVisitor<const H: usize>;

    impl<'de, const H: usize> Visitor<'de> for TupleVisitor<H> {
        type Value = MinHashSig<H>;

        fn expecting(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            write!(f, "a {}-element MinHashSig<{H}> tuple", 1 + H)
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let schema: u16 = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(0, &self))?;
            let mut hashes = [0_u64; H];
            for (i, slot) in hashes.iter_mut().enumerate() {
                *slot = seq
                    .next_element::<u64>()?
                    .ok_or_else(|| de::Error::invalid_length(1 + i, &self))?;
            }
            Ok(MinHashSig {
                schema,
                _pad: [0; 6],
                hashes,
            })
        }
    }
};

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

    #[cfg(feature = "serde")]
    #[test]
    fn serde_json_round_trip() {
        let s: MinHashSig<8> = MinHashSig {
            schema: SCHEMA_VERSION,
            _pad: [0; 6],
            hashes: [11, 22, 33, 44, 55, 66, 77, 88],
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"schema\":1"));
        assert!(json.contains("\"hashes\":[11,22,33,44,55,66,77,88]"));
        let back: MinHashSig<8> = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_json_h128_round_trip() {
        let s: MinHashSig<128> = MinHashSig::empty();
        let json = serde_json::to_string(&s).unwrap();
        let back: MinHashSig<128> = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_rejects_wrong_length_in_human_format() {
        // 7 hashes when H=8 should fail.
        let bad = "{\"schema\":1,\"hashes\":[1,2,3,4,5,6,7]}";
        let r: Result<MinHashSig<8>, _> = serde_json::from_str(bad);
        assert!(r.is_err());
    }
}
