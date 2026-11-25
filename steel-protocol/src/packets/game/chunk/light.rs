use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_LIGHT_UPDATE;
use steel_utils::codec::VarInt;
use steel_utils::serial::WriteTo as _;

use super::LightUpdatePacketData;
use crate::version::MinecraftVersion;

#[derive(ClientPacket, Debug, Clone, WriteTo)]
#[packet_id(Play = C_LIGHT_UPDATE)]
#[legacy_payload]
pub struct CLightUpdate {
    #[write(as = VarInt)]
    pub x: i32,
    #[write(as = VarInt)]
    pub z: i32,
    pub light_data: LightUpdatePacketData,
}

impl CLightUpdate {
    /// Writes the body, translating only the light masks.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl std::io::Write,
        version: MinecraftVersion,
    ) -> std::io::Result<()> {
        VarInt(self.x).write(writer)?;
        VarInt(self.z).write(writer)?;
        self.light_data.write_legacy(writer, version)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use steel_utils::{
        codec::{BitSet, VarInt},
        serial::{ReadFrom, WriteTo},
    };

    use super::CLightUpdate;
    use crate::packets::game::LightUpdatePacketData;
    use crate::version::MinecraftVersion;

    #[test]
    fn writes_chunk_coordinates_as_varints_and_trims_empty_masks() {
        fn empty_mask() -> BitSet {
            BitSet(vec![0].into_boxed_slice())
        }

        let packet = CLightUpdate {
            x: 2,
            z: -3,
            light_data: LightUpdatePacketData {
                sky_y_mask: empty_mask(),
                block_y_mask: empty_mask(),
                empty_sky_y_mask: empty_mask(),
                empty_block_y_mask: empty_mask(),
                sky_updates: Vec::new(),
                block_updates: Vec::new(),
            },
        };

        let mut data = Vec::new();
        let Ok(()) = packet.write(&mut data) else {
            panic!("light update packet should encode");
        };

        let mut cursor = Cursor::new(data.as_slice());
        let Ok(x) = VarInt::read(&mut cursor) else {
            panic!("x varint missing");
        };
        let Ok(z) = VarInt::read(&mut cursor) else {
            panic!("z varint missing");
        };
        assert_eq!(x.0, 2);
        assert_eq!(z.0, -3);
        for _ in 0..4 {
            let Ok(mask_len) = VarInt::read(&mut cursor) else {
                panic!("light mask length missing");
            };
            assert_eq!(mask_len.0, 0);
        }
    }

    #[test]
    fn masks_use_word_count_before_26_3() {
        fn mask(words: Vec<u64>) -> BitSet {
            BitSet(words.into_boxed_slice())
        }
        let packet = CLightUpdate {
            x: 2,
            z: -3,
            light_data: LightUpdatePacketData {
                sky_y_mask: mask(vec![0b1011]),
                block_y_mask: mask(vec![0]),
                empty_sky_y_mask: mask(vec![0]),
                empty_block_y_mask: mask(vec![0]),
                sky_updates: Vec::new(),
                block_updates: Vec::new(),
            },
        };

        let mut legacy = Vec::new();
        packet
            .write_payload_legacy(&mut legacy, MinecraftVersion::V26_2)
            .expect("legacy light should encode");

        let mut expected = Vec::new();
        VarInt(2).write(&mut expected).unwrap();
        VarInt(-3).write(&mut expected).unwrap();
        for word in [0b1011u64, 0, 0, 0] {
            VarInt(1).write(&mut expected).unwrap();
            expected.extend_from_slice(&word.to_be_bytes());
        }
        VarInt(0).write(&mut expected).unwrap();
        VarInt(0).write(&mut expected).unwrap();
        assert_eq!(legacy, expected);

        // The native form is the trimmed little-endian byte layout instead.
        let mut native = Vec::new();
        packet
            .write(&mut native)
            .expect("native light should encode");
        assert_ne!(native, legacy);
    }
}
