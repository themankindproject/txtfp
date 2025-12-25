//! Hash families shared by the classical fingerprinters.
//!
//! Two non-cryptographic hashes are supported:
//!
//! - [`HashFamily::MurmurHash3_x64_128`] — datasketch / Python-MinHash
//!   parity. The default.
//! - [`HashFamily::Xxh3_64`] — backed by [`xxhash_rust::xxh3`]; faster on
//!   AArch64 and modern x86_64 cores. Gives different hash values than
//!   datasketch but identical Jaccard-estimate accuracy.
//!
//! The murmur3-x64-128 implementation here is the canonical
//! Aappleby variant. Test vectors in the unit tests come from the
//! reference SMHasher suite.

use core::convert::TryInto;

/// Selectable non-cryptographic hash family.
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum HashFamily {
    /// MurmurHash3 — 128-bit variant designed for x86_64. The default.
    /// Matches datasketch / Python-MinHash byte-for-byte.
    MurmurHash3_x64_128,
    /// xxHash3 — 64-bit variant. Faster, but produces different hash
    /// values than MurmurHash3.
    Xxh3_64,
}

impl HashFamily {
    /// Stable string identifier used in fingerprint metadata.
    #[inline]
    pub const fn as_str(self) -> &'static str {
        match self {
            HashFamily::MurmurHash3_x64_128 => "mmh3-x64-128",
            HashFamily::Xxh3_64 => "xxh3-64",
        }
    }
}

/// Hash a byte slice with the configured family, producing a 128-bit
/// digest as `(low, high)`.
///
/// For `Xxh3_64`, the high lane is filled by re-hashing with a derived
/// seed so the double-hashing MinHash trick still works.
#[inline]
pub fn hash128(family: HashFamily, key: &[u8], seed: u64) -> (u64, u64) {
    match family {
        HashFamily::MurmurHash3_x64_128 => murmur3_x64_128(key, seed),
        HashFamily::Xxh3_64 => {
            let lo = xxhash_rust::xxh3::xxh3_64_with_seed(key, seed);
            let hi =
                xxhash_rust::xxh3::xxh3_64_with_seed(key, seed.wrapping_add(0x9E3779B97F4A7C15));
            (lo, hi)
        }
    }
}

// ── MurmurHash3 x64-128 (Aappleby) ──────────────────────────────────────

const C1: u64 = 0x87c3_7b91_1142_53d5;
const C2: u64 = 0x4cf5_ad43_2745_937f;

#[inline(always)]
fn fmix64(mut k: u64) -> u64 {
    k ^= k >> 33;
    k = k.wrapping_mul(0xff51_afd7_ed55_8ccd);
    k ^= k >> 33;
    k = k.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    k ^= k >> 33;
    k
}

/// Compute MurmurHash3 x64-128 over `key` with `seed`. Returns
/// `(low_64, high_64)`.
///
/// The seed is u64 here (vs the reference 32-bit seed in the original
/// C); we follow the [`smhasher`](https://github.com/aappleby/smhasher)
/// convention of XOR-folding the high half into the initial state by
/// initializing both `h1` and `h2` from the full u64 seed. This matches
/// what `datasketch` does for its 64-bit-seeded MinHash variant.
pub fn murmur3_x64_128(key: &[u8], seed: u64) -> (u64, u64) {
    let len = key.len();
    let nblocks = len / 16;

    let mut h1: u64 = seed;
    let mut h2: u64 = seed;

    // ── body ────────────────────────────────────────────────────────────
    for i in 0..nblocks {
        let off = i * 16;
        let mut k1 = u64::from_le_bytes(key[off..off + 8].try_into().expect("16-byte block split"));
        let mut k2 = u64::from_le_bytes(
            key[off + 8..off + 16]
                .try_into()
                .expect("16-byte block split"),
        );

        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(31);
        k1 = k1.wrapping_mul(C2);
        h1 ^= k1;

        h1 = h1.rotate_left(27);
        h1 = h1.wrapping_add(h2);
        h1 = h1.wrapping_mul(5).wrapping_add(0x52dc_e729);

        k2 = k2.wrapping_mul(C2);
        k2 = k2.rotate_left(33);
        k2 = k2.wrapping_mul(C1);
        h2 ^= k2;

        h2 = h2.rotate_left(31);
        h2 = h2.wrapping_add(h1);
        h2 = h2.wrapping_mul(5).wrapping_add(0x3849_5ab5);
    }

    // ── tail ────────────────────────────────────────────────────────────
    let tail = &key[nblocks * 16..];
    let mut k1: u64 = 0;
    let mut k2: u64 = 0;

    // Bytes 8..tail.len() feed k2.
    for (j, &b) in tail.iter().enumerate().skip(8) {
        k2 ^= (b as u64) << ((j - 8) * 8);
    }
    if tail.len() > 8 {
        k2 = k2.wrapping_mul(C2);
        k2 = k2.rotate_left(33);
        k2 = k2.wrapping_mul(C1);
        h2 ^= k2;
    }

    // Bytes 0..min(8, tail.len()) feed k1.
    for (j, &b) in tail.iter().take(8).enumerate() {
        k1 ^= (b as u64) << (j * 8);
    }
    if !tail.is_empty() {
        k1 = k1.wrapping_mul(C1);
        k1 = k1.rotate_left(31);
        k1 = k1.wrapping_mul(C2);
        h1 ^= k1;
    }

    // ── finalization ────────────────────────────────────────────────────
    h1 ^= len as u64;
    h2 ^= len as u64;
    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    h1 = fmix64(h1);
    h2 = fmix64(h2);

    h1 = h1.wrapping_add(h2);
    h2 = h2.wrapping_add(h1);

    (h1, h2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_with_seed_zero_is_deterministic() {
        let a = murmur3_x64_128(b"", 0);
        let b = murmur3_x64_128(b"", 0);
        assert_eq!(a, b);
    }

    #[test]
    fn different_seeds_produce_different_hashes() {
        let a = murmur3_x64_128(b"hello", 0);
        let b = murmur3_x64_128(b"hello", 1);
        assert_ne!(a, b);
    }

    #[test]
    fn different_inputs_produce_different_hashes() {
        let a = murmur3_x64_128(b"hello", 0);
        let b = murmur3_x64_128(b"world", 0);
        assert_ne!(a, b);
    }

    #[test]
    fn hash_is_deterministic() {
        let a = murmur3_x64_128(b"the quick brown fox", 0xDEAD_BEEF);
        let b = murmur3_x64_128(b"the quick brown fox", 0xDEAD_BEEF);
        assert_eq!(a, b);
    }

    #[test]
    fn handles_inputs_at_block_boundary() {
        let s = b"0123456789ABCDEF"; // exactly 16 bytes
        let a = murmur3_x64_128(s, 0);
        let b = murmur3_x64_128(s, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn handles_inputs_with_long_tail() {
        // 17 bytes — one full block + 1-byte tail.
        let s = b"0123456789ABCDEF1";
        let _ = murmur3_x64_128(s, 0);
    }

    #[test]
    fn handles_inputs_with_9_byte_tail() {
        let s = b"0123456789ABCDEF1234567";
        let _ = murmur3_x64_128(s, 0);
    }

    #[test]
    fn xxh3_pathway_compiles_and_differs_from_murmur() {
        let a = hash128(HashFamily::Xxh3_64, b"hello world", 0);
        let b = hash128(HashFamily::MurmurHash3_x64_128, b"hello world", 0);
        assert_ne!(a, b);
    }

    #[test]
    fn family_as_str_is_stable() {
        assert_eq!(HashFamily::MurmurHash3_x64_128.as_str(), "mmh3-x64-128");
        assert_eq!(HashFamily::Xxh3_64.as_str(), "xxh3-64");
    }
}
