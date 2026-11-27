//! Network encoding of paletted container entries.
//!
//! Entries are packed like vanilla `SimpleBitStorage`: `64 / bits` entries per long, low
//! bits first, no entry spanning two longs, longs written big-endian.

use std::io::{Error, ErrorKind, Result};

/// Largest packed payload: a 16×16×16 section at 16 bits per entry.
pub(super) const MAX_PACKED_BYTES: usize = super::BlockPalette::VOLUME.div_ceil(64 / 16) * 8;

/// Packs ids, rejecting any id >= limit.
pub(super) fn pack_u32_iter(
    ids: impl ExactSizeIterator<Item = u32>,
    bits: u8,
    limit: u32,
    buffer: &mut [u8; MAX_PACKED_BYTES],
) -> Result<&[u8]> {
    // A width known at compile time lets each long be filled by a fixed-length loop.
    macro_rules! pack_with_width {
        ($($width:literal)*) => {
            match bits {
                $($width => pack_longs_iter::<$width>(ids, limit, buffer),)*
                _ => Err(invalid_data("unsupported bits per entry")),
            }
        };
    }
    pack_with_width!(1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16)
}

/// Packs ids into big-endian words, one word at a time.
fn pack_longs_iter<const BITS: usize>(
    ids: impl ExactSizeIterator<Item = u32>,
    limit: u32,
    buffer: &mut [u8; MAX_PACKED_BYTES],
) -> Result<&[u8]> {
    let values_per_long = 64 / BITS;
    let len = ids.len().div_ceil(values_per_long) * 8;
    let packed = buffer
        .get_mut(..len)
        .ok_or_else(|| invalid_data("container is too large to pack"))?;
    // Accumulate misses instead of branching per entry; the whole container is rejected.
    let (words, _) = packed.as_chunks_mut::<8>();
    let mut invalid = false;
    for (slot, id) in ids.enumerate() {
        invalid |= id >= limit;
        let word = &mut words[slot / values_per_long];
        let shift = (slot % values_per_long) * BITS;
        let value = (if id < limit { u64::from(id) } else { 0 }) << shift;
        *word = (u64::from_be_bytes(*word) | value).to_be_bytes();
    }
    if invalid {
        return Err(invalid_data(
            "container holds a value its palette cannot encode",
        ));
    }
    Ok(packed)
}

fn invalid_data(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests {
    use super::{MAX_PACKED_BYTES, pack_u32_iter};

    /// Packs values at every width from 1 to 16 bits, including 9 to 15, which no section
    /// uses with vanilla's registries, and reads them back the way vanilla's
    /// `SimpleBitStorage` does: `64 / bits` values per big-endian long, low bits first.
    #[test]
    fn packs_every_width_like_simple_bit_storage() {
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        for bits in 1..=16u8 {
            let mask = (1u64 << bits) - 1;
            let values: Vec<u32> = (0..4096)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    (state & mask) as u32
                })
                .collect();
            let mut buffer = [0; MAX_PACKED_BYTES];
            let packed = pack_u32_iter(values.iter().copied(), bits, 1u32 << bits, &mut buffer)
                .expect("every value fits the width");

            let values_per_long = 64 / usize::from(bits);
            let (longs, rest) = packed.as_chunks::<8>();
            assert!(
                rest.is_empty(),
                "{bits} bits: packed bytes should be whole longs"
            );
            assert_eq!(
                longs.len(),
                values.len().div_ceil(values_per_long),
                "{bits} bits"
            );
            for (index, &value) in values.iter().enumerate() {
                let long = u64::from_be_bytes(longs[index / values_per_long]);
                let entry = (long >> (index % values_per_long * usize::from(bits))) & mask;
                assert_eq!(entry, u64::from(value), "{bits} bits, value {index}");
            }
        }
    }
}
