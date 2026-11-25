//! Clientbound default spawn position packet.

use std::io::{self, Write};

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_SET_DEFAULT_SPAWN_POSITION;
use steel_utils::GlobalPos;
use steel_utils::serial::WriteTo;

use crate::version::MinecraftVersion;

/// Updates the client's default world spawn marker.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_SET_DEFAULT_SPAWN_POSITION)]
#[legacy_payload]
pub struct CSetDefaultSpawnPosition {
    /// Dimension and block position of the default spawn.
    pub global_pos: GlobalPos,
    /// Spawn yaw.
    pub yaw: f32,
    /// Spawn pitch.
    pub pitch: f32,
}

impl CSetDefaultSpawnPosition {
    /// Writes the body for a client on an older protocol version.
    ///
    /// Before 1.21.9 the packet carries only the block position and the yaw: the client
    /// always assumes the dimension it is currently in and ignores pitch.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> io::Result<()> {
        if version.at_least(MinecraftVersion::V1_21_9) {
            return self.write(writer);
        }
        self.global_pos.pos.write(writer)?;
        self.yaw.write(writer)
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::BlockPos;
    use steel_utils::GlobalPos;
    use steel_utils::Identifier;
    use steel_utils::serial::WriteTo as _;

    use super::CSetDefaultSpawnPosition;
    use crate::version::MinecraftVersion;

    fn packet() -> CSetDefaultSpawnPosition {
        CSetDefaultSpawnPosition {
            global_pos: GlobalPos::new(
                Identifier::vanilla_static("overworld"),
                BlockPos::new(10, 64, -20),
            ),
            yaw: 90.0,
            pitch: 45.0,
        }
    }

    fn encode_legacy(packet: &CSetDefaultSpawnPosition, version: MinecraftVersion) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet
            .write_payload_legacy(&mut bytes, version)
            .expect("legacy payload should encode");
        bytes
    }

    #[test]
    fn from_1_21_9_matches_native() {
        let packet = packet();
        let mut native = Vec::new();
        packet.write(&mut native).expect("native encode");
        for version in [
            MinecraftVersion::V1_21_9,
            MinecraftVersion::V26_1,
            MinecraftVersion::V26_3,
        ] {
            assert_eq!(encode_legacy(&packet, version), native);
        }
    }

    #[test]
    fn before_1_21_9_drops_dimension_and_pitch() {
        let packet = packet();
        let bytes = encode_legacy(&packet, MinecraftVersion::V1_21);
        let mut expected = Vec::new();
        packet
            .global_pos
            .pos
            .write(&mut expected)
            .expect("block position should encode");
        packet.yaw.write(&mut expected).expect("yaw should encode");
        assert_eq!(bytes, expected);
    }

    #[test]
    fn before_1_21_9_matches_until_1_21_7() {
        let packet = packet();
        let bytes = encode_legacy(&packet, MinecraftVersion::V1_21_7);
        assert_eq!(bytes, encode_legacy(&packet, MinecraftVersion::V1_21));
    }
}
