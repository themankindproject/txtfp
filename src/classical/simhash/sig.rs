//! SimHash 64-bit signature byte layout.

/// Schema version embedded in the [`SimHash64`] envelope. Frozen for v0.1.x.
///
/// [`SimHash64`] itself is `repr(transparent)` over a `u64` — the schema
/// is implicitly v1 by virtue of using this type. The explicit constant
/// is exposed for round-trip validators that want to assert it
/// alongside other variants.
pub const SCHEMA_VERSION: u16 = 1;

/// 64-bit SimHash signature. Charikar 2002.
///
/// Layout: a single little-endian `u64`. `bytemuck::Pod` makes
/// `cast_slice` zero-copy for batched persistence.
#[repr(transparent)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SimHash64(pub u64);

impl SimHash64 {
    /// Construct from a raw `u64`.
    #[inline]
    #[must_use]
    pub const fn new(bits: u64) -> Self {
        Self(bits)
    }

    /// Extract the raw bits.
    #[inline]
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// View the signature as a byte slice. Zero-copy.
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}

impl From<u64> for SimHash64 {
    #[inline]
    fn from(v: u64) -> Self {
        Self(v)
    }
}

impl From<SimHash64> for u64 {
    #[inline]
    fn from(v: SimHash64) -> Self {
        v.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_through_bytes() {
        let s = SimHash64::new(0xDEAD_BEEF_CAFE_BABE);
        let bytes = s.as_bytes();
        assert_eq!(bytes.len(), 8);
        let s2: SimHash64 = *bytemuck::from_bytes(bytes);
        assert_eq!(s, s2);
    }

    #[test]
    fn pod_eq_hash() {
        fn assert_pod<T: bytemuck::Pod>() {}
        fn assert_eq_hash<T: Eq + core::hash::Hash>() {}
        assert_pod::<SimHash64>();
        assert_eq_hash::<SimHash64>();
    }

    #[test]
    fn schema_constant_is_one() {
        assert_eq!(SCHEMA_VERSION, 1);
    }

    #[test]
    fn from_into_u64() {
        let s: SimHash64 = 42u64.into();
        let n: u64 = s.into();
        assert_eq!(n, 42);
    }
}
