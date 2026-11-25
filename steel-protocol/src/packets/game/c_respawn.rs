//! Clientbound respawn packet - sent to respawn a player or change dimensions.

use std::io::{self, Write};

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_RESPAWN;
use steel_utils::serial::WriteTo;

use super::c_login::CommonPlayerSpawnInfo;
use crate::version::MinecraftVersion;

/// Respawn a player in any dimension.
///
/// Sent by the server when a player respawns after death or changes dimensions.
/// The client will reset its world state and prepare for new chunk data.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_RESPAWN)]
#[legacy_payload]
pub struct CRespawn {
    /// Dimension and world state the player respawns into.
    pub common_player_spawn_info: CommonPlayerSpawnInfo,
    /// Bit field: 0x01 = keep attribute modifiers, 0x02 = keep entity data.
    /// Set to 0 for a full reset (normal respawn).
    pub data_kept: i8,
}

impl CRespawn {
    /// Writes the body for a client on an older protocol version.
    ///
    /// Every difference is a per-field version check inside
    /// [`CommonPlayerSpawnInfo::write_legacy`]; the trailing `data_kept` byte is stable
    /// across the supported range.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> io::Result<()> {
        self.common_player_spawn_info
            .write_legacy(writer, version)?;
        self.data_kept.write(writer)
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::Identifier;
    use steel_utils::serial::WriteTo as _;

    use super::CRespawn;
    use crate::packets::game::c_login::CommonPlayerSpawnInfo;
    use crate::version::MinecraftVersion;

    fn respawn() -> CRespawn {
        CRespawn {
            common_player_spawn_info: CommonPlayerSpawnInfo {
                dimension_type: 1,
                dimension: Identifier::vanilla_static("the_nether"),
                seed: -7,
                game_type: steel_utils::types::GameType::Survival,
                previous_game_type: None,
                is_debug: false,
                is_flat: false,
                last_death_location: None,
                portal_cooldown: 2,
                sea_level: 32,
            },
            data_kept: 3,
        }
    }

    fn encode_legacy(packet: &CRespawn, version: MinecraftVersion) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet
            .write_payload_legacy(&mut bytes, version)
            .expect("legacy payload should encode");
        bytes
    }

    #[test]
    fn legacy_payload_26_3_matches_native() {
        let packet = respawn();
        let mut native = Vec::new();
        packet.write(&mut native).expect("native encode");
        assert_eq!(encode_legacy(&packet, MinecraftVersion::V26_3), native);
    }

    #[test]
    fn legacy_payload_drops_sea_level_on_1_21() {
        let packet = respawn();
        let with_sea_level = encode_legacy(&packet, MinecraftVersion::V1_21_2);
        let without = encode_legacy(&packet, MinecraftVersion::V1_21);
        assert_eq!(with_sea_level.len(), without.len() + 1);
    }

    #[test]
    fn legacy_payload_keeps_data_kept_last() {
        let packet = respawn();
        for version in [
            MinecraftVersion::V1_21,
            MinecraftVersion::V1_21_9,
            MinecraftVersion::V26_2,
            MinecraftVersion::V26_3,
        ] {
            let bytes = encode_legacy(&packet, version);
            assert_eq!(*bytes.last().expect("data kept byte"), 3);
        }
    }
}
