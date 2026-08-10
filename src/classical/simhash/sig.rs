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
    ///
    /// Mostly used for tests and round-tripping serialized signatures;
    /// production code obtains a `SimHash64` from
    /// [`SimHashFingerprinter::fingerprint`](super::SimHashFingerprinter).
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::SimHash64;
    /// let s = SimHash64::new(0xDEAD_BEEF);
    /// assert_eq!(s.bits(), 0xDEAD_BEEF);
    /// ```
    #[inline]
    #[must_use]
    pub const fn new(bits: u64) -> Self {
        Self(bits)
    }

    /// Extract the raw bits.
    ///
    /// `SimHash64` is `repr(transparent)` over `u64`, so this is a
    /// trivial field access.
    #[inline]
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }

    /// View the signature as a byte slice. Zero-copy.
    ///
    /// # Returns
    ///
    /// An 8-byte little-endian slice. Useful for bulk persistence and
    /// content-addressed cache keys.
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::SimHash64;
    /// let s = SimHash64::new(0x0102_0304_0506_0708);
    /// assert_eq!(s.as_bytes(), &[8, 7, 6, 5, 4, 3, 2, 1]);
    /// ```
    #[inline]
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }

    /// Deserialize from the 8-byte little-endian layout.
    ///
    /// [`SimHash64`] carries no explicit schema word (it is
    /// `repr(transparent)` over `u64`), so the only validation possible
    /// is the length check; schema compatibility is implied by the type.
    ///
    /// # Errors
    ///
    /// Returns [`crate::Error::InvalidInput`] when `bytes.len()`
    /// is not exactly 8.
    ///
    /// # Example
    ///
    /// ```
    /// use txtfp::SimHash64;
    ///
    /// let s = SimHash64::new(0xDEAD_BEEF);
    /// let back = SimHash64::from_bytes(s.as_bytes()).unwrap();
    /// assert_eq!(s, back);
    /// ```
    #[inline]
    pub fn from_bytes(bytes: &[u8]) -> crate::Result<Self> {
        let arr: [u8; 8] = bytes.try_into().map_err(|_| {
            crate::Error::InvalidInput(alloc::format!("SimHash64 is 8 bytes, got {}", bytes.len()))
        })?;
        Ok(Self(u64::from_le_bytes(arr)))
    }
}

impl core::convert::TryFrom<&[u8]> for SimHash64 {
    type Error = crate::Error;

    fn try_from(bytes: &[u8]) -> crate::Result<Self> {
        Self::from_bytes(bytes)
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

impl core::fmt::Display for SimHash64 {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "SimHash64({:016x})", self.0)
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
    fn from_bytes_round_trips() {
        let s = SimHash64::new(0xDEAD_BEEF_CAFE_BABE);
        let back = SimHash64::from_bytes(s.as_bytes()).unwrap();
        assert_eq!(s, back);
        let back2: SimHash64 = s.as_bytes().try_into().unwrap();
        assert_eq!(s, back2);
    }

    #[test]
    fn from_bytes_rejects_wrong_length() {
        assert!(matches!(
            SimHash64::from_bytes(&[0_u8; 7]),
            Err(crate::Error::InvalidInput(_))
        ));
        assert!(matches!(
            SimHash64::from_bytes(&[0_u8; 9]),
            Err(crate::Error::InvalidInput(_))
        ));
        assert!(SimHash64::from_bytes(&[0_u8; 8]).is_ok());
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
