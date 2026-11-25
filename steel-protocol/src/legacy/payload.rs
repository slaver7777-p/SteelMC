//! # Legacy payload adapters
//!
//! Field-level encoders for the wire formats that changed inside the supported
//! 1.21–26.3 range. A packet whose payload differs from the native one overrides
//! [`crate::packet_traits::ClientPacket::write_payload_legacy`] and calls these helpers
//! instead of the plain field write.
//!
//! The native encoding is always the `>=` side of each boundary, so a packet that does not
//! override anything stays byte-identical to a server without multiversion support.

use std::io::{Cursor, Error, ErrorKind, Write};

use glam::DVec3;
use steel_utils::codec::{BitSet, LpVec3, VarInt};
use steel_utils::serial::{ReadFrom, WriteTo};

use crate::version::MinecraftVersion;

/// The version from which a container id is a `VarInt`; earlier it is a single byte.
///
/// Vanilla 1.21.2 widened the menu id so servers can open more than 255 concurrent menus.
const CONTAINER_ID_VARINT: MinecraftVersion = MinecraftVersion::V1_21_2;

/// The version from which velocity is an `LpVec3`; earlier it is three `i16` components.
///
/// Vanilla 1.21.9 replaced the fixed-point vector with the `LpVec3` codec.
const VELOCITY_LP_VEC3: MinecraftVersion = MinecraftVersion::V1_21_9;

/// The version whose bit sets use the little-endian byte form; earlier they are a `VarInt`
/// word count followed by big-endian `i64` words.
const BIT_SET_BYTES: MinecraftVersion = MinecraftVersion::V26_3;

/// Writes a container id as a byte before 1.21.2, as a `VarInt` from it.
pub fn write_container_id_legacy(
    container_id: i32,
    version: MinecraftVersion,
    writer: &mut impl Write,
) -> std::io::Result<()> {
    if version.at_least(CONTAINER_ID_VARINT) {
        VarInt(container_id).write(writer)
    } else {
        // Vanilla reads this field as an unsigned byte; low 8 bits on the wire.
        (container_id as u8).write(writer)
    }
}

/// Reads a container id written by [`write_container_id_legacy`].
pub fn read_container_id_legacy(
    data: &mut Cursor<&[u8]>,
    version: MinecraftVersion,
) -> std::io::Result<i32> {
    if version.at_least(CONTAINER_ID_VARINT) {
        Ok(VarInt::read(data)?.0)
    } else {
        Ok(i32::from(u8::read(data)?))
    }
}

/// Velocity magnitude above which the pre-1.21.9 wire saturates, in blocks per tick.
const LEGACY_VELOCITY_CLAMP: f64 = 3.9;
/// Fixed-point scale of the pre-1.21.9 velocity components, in units per block per tick.
const LEGACY_VELOCITY_SCALE: f64 = 8000.0;

/// Writes velocity as an `LpVec3` from 1.21.9, as three `i16` components before it.
pub fn write_velocity_legacy(
    velocity: DVec3,
    version: MinecraftVersion,
    writer: &mut impl Write,
) -> std::io::Result<()> {
    if version.at_least(VELOCITY_LP_VEC3) {
        return LpVec3(velocity).write(writer);
    }
    for component in [velocity.x, velocity.y, velocity.z] {
        encode_velocity_component(component).write(writer)?;
    }
    Ok(())
}

/// Reads velocity written by [`write_velocity_legacy`].
pub fn read_velocity_legacy(
    data: &mut Cursor<&[u8]>,
    version: MinecraftVersion,
) -> std::io::Result<DVec3> {
    if version.at_least(VELOCITY_LP_VEC3) {
        return Ok(LpVec3::read(data)?.0);
    }
    let mut components = [0.0_f64; 3];
    for component in &mut components {
        let scaled = i16::read(data)?;
        *component = f64::from(scaled) / LEGACY_VELOCITY_SCALE;
    }
    Ok(DVec3::from_array(components))
}

/// Encodes one velocity component as the pre-1.21.9 fixed-point short.
///
/// Saturates at [`LEGACY_VELOCITY_CLAMP`] because a short cannot hold a larger scaled
/// magnitude, which is what vanilla does before 1.21.9.
#[must_use]
pub const fn encode_velocity_component(component: f64) -> i16 {
    (component.clamp(-LEGACY_VELOCITY_CLAMP, LEGACY_VELOCITY_CLAMP) * LEGACY_VELOCITY_SCALE) as i16
}

/// Writes a bit set as little-endian bytes from 26.3, as big-endian words before it.
pub fn write_bit_set_legacy(
    bit_set: &BitSet,
    version: MinecraftVersion,
    writer: &mut impl Write,
) -> std::io::Result<()> {
    if version.at_least(BIT_SET_BYTES) {
        return bit_set.write(writer);
    }
    VarInt(bit_set.0.len() as i32).write(writer)?;
    for word in &bit_set.0 {
        word.write(writer)?;
    }
    Ok(())
}

/// Reads a bit set written by [`write_bit_set_legacy`].
///
/// A word count is a client-supplied allocation size, so it is bounded by the bytes the
/// packet actually has left rather than by a capacity clamp.
///
/// # Errors
/// - If the word count is negative or larger than the remaining payload.
/// - If the payload is truncated.
pub fn read_bit_set_legacy(
    data: &mut Cursor<&[u8]>,
    version: MinecraftVersion,
) -> std::io::Result<BitSet> {
    if version.at_least(BIT_SET_BYTES) {
        return BitSet::read(data);
    }
    let word_count = VarInt::read(data)?.0;
    let Ok(word_count) = usize::try_from(word_count) else {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "bit set word count is negative",
        ));
    };
    let position = usize::try_from(data.position())
        .map_err(|_| Error::new(ErrorKind::UnexpectedEof, "bit set starts beyond the input"))?;
    let remaining = data.get_ref().len().saturating_sub(position);
    if word_count > remaining / size_of::<u64>() {
        return Err(Error::new(
            ErrorKind::UnexpectedEof,
            "bit set word count exceeds the packet",
        ));
    }
    let mut words = Vec::with_capacity(word_count);
    for _ in 0..word_count {
        words.push(u64::read(data)?);
    }
    Ok(BitSet(words.into_boxed_slice()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LpVec3` quantization tolerance for round-trip tests.
    const QUANTUM_TOLERANCE: f64 = 0.001;

    fn write_with<F>(version: MinecraftVersion, f: F) -> Vec<u8>
    where
        F: FnOnce(&mut Vec<u8>, MinecraftVersion) -> std::io::Result<()>,
    {
        let mut buf = Vec::new();
        f(&mut buf, version).expect("legacy field should encode");
        buf
    }

    #[test]
    fn container_id_is_a_varint_from_1_21_2() {
        for version in [
            MinecraftVersion::V1_21_2,
            MinecraftVersion::V26_2,
            MinecraftVersion::CURRENT,
        ] {
            let mut buf = Vec::new();
            write_container_id_legacy(300, version, &mut buf).unwrap();
            assert_eq!(buf, vec![0xAC, 0x02], "{version} must send a VarInt");
            let mut data = Cursor::new(buf.as_slice());
            assert_eq!(read_container_id_legacy(&mut data, version).unwrap(), 300);
        }
    }

    #[test]
    fn container_id_is_a_byte_before_1_21_2() {
        let mut buf = Vec::new();
        write_container_id_legacy(300, MinecraftVersion::V1_21, &mut buf).unwrap();
        // 300 does not fit a byte; the low bits are what vanilla's unsigned read yields.
        assert_eq!(buf, vec![44]);
        let mut data = Cursor::new(buf.as_slice());
        assert_eq!(
            read_container_id_legacy(&mut data, MinecraftVersion::V1_21).unwrap(),
            44
        );
    }

    #[test]
    fn velocity_is_lp_vec3_from_1_21_9() {
        let velocity = DVec3::new(0.25, -0.5, 1.0);
        let lp = write_with(MinecraftVersion::CURRENT, |buf, version| {
            write_velocity_legacy(velocity, version, buf)
        });
        let expected = {
            let mut buf = Vec::new();
            LpVec3(velocity).write(&mut buf).unwrap();
            buf
        };
        assert_eq!(
            lp, expected,
            "1.21.9+ must be byte-identical to native LpVec3"
        );
        let mut data = Cursor::new(lp.as_slice());
        let read = read_velocity_legacy(&mut data, MinecraftVersion::CURRENT).unwrap();
        // LpVec3 quantizes, so a round trip is within its quantum, not exact.
        assert!((read - velocity).abs().max_element() < QUANTUM_TOLERANCE);
    }

    #[test]
    fn velocity_is_three_shorts_before_1_21_9() {
        let velocity = DVec3::new(1.0, -2.0, 0.5);
        let buf = write_with(MinecraftVersion::V1_21, |buf, version| {
            write_velocity_legacy(velocity, version, buf)
        });
        assert_eq!(buf.len(), 6);
        assert_eq!(&buf[0..2], &encode_velocity_component(1.0).to_be_bytes());
        assert_eq!(&buf[2..4], &encode_velocity_component(-2.0).to_be_bytes());
        assert_eq!(&buf[4..6], &encode_velocity_component(0.5).to_be_bytes());

        let mut data = Cursor::new(buf.as_slice());
        let read = read_velocity_legacy(&mut data, MinecraftVersion::V1_21).unwrap();
        // The fixed-point format is lossy; a round trip must stay within one quantum.
        let quantum = 1.0 / LEGACY_VELOCITY_SCALE;
        assert!((read - velocity).abs().max_element() <= quantum);
    }

    #[test]
    fn velocity_saturates_at_the_legacy_clamp() {
        assert_eq!(
            encode_velocity_component(90.0),
            encode_velocity_component(LEGACY_VELOCITY_CLAMP)
        );
        assert_eq!(
            encode_velocity_component(-90.0),
            encode_velocity_component(-LEGACY_VELOCITY_CLAMP)
        );
        assert_eq!(encode_velocity_component(0.0), 0);
    }

    #[test]
    fn bit_set_is_bytes_from_26_3() {
        let bit_set = BitSet(vec![0x0102_0304_0506_0708, 0, 0xff].into_boxed_slice());
        let native = write_with(MinecraftVersion::CURRENT, |buf, version| {
            write_bit_set_legacy(&bit_set, version, buf)
        });
        let mut expected = Vec::new();
        bit_set.write(&mut expected).unwrap();
        assert_eq!(native, expected, "26.3 must be byte-identical to native");

        let mut data = Cursor::new(native.as_slice());
        assert_eq!(
            read_bit_set_legacy(&mut data, MinecraftVersion::CURRENT).unwrap(),
            bit_set
        );
    }

    #[test]
    fn bit_set_is_big_endian_words_before_26_3() {
        let bit_set = BitSet(vec![1, 2].into_boxed_slice());
        let buf = write_with(MinecraftVersion::V26_2, |buf, version| {
            write_bit_set_legacy(&bit_set, version, buf)
        });
        let mut expected = vec![2u8];
        expected.extend_from_slice(&1u64.to_be_bytes());
        expected.extend_from_slice(&2u64.to_be_bytes());
        assert_eq!(buf, expected);

        let mut data = Cursor::new(buf.as_slice());
        assert_eq!(
            read_bit_set_legacy(&mut data, MinecraftVersion::V26_2).unwrap(),
            bit_set
        );
    }

    #[test]
    fn bit_set_rejects_a_negative_word_count() {
        let buf = vec![0xFF, 0xFF, 0xFF, 0xFF, 0x0F];
        let mut data = Cursor::new(buf.as_slice());
        assert!(read_bit_set_legacy(&mut data, MinecraftVersion::V26_2).is_err());
    }
}
