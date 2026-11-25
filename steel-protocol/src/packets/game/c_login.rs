//! Clientbound login packet - sent when a player first joins the world.

use std::io::{self, Write};

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_LOGIN;
use steel_utils::serial::{PrefixedWrite, WriteTo};
use steel_utils::{GlobalPos, Identifier, types::GameType};

use crate::version::MinecraftVersion;

/// Dimension and world state the client uses when it enters the world.
#[derive(Clone, Debug, WriteTo)]
pub struct CommonPlayerSpawnInfo {
    #[write(as = VarInt)]
    pub dimension_type: i32,
    pub dimension: Identifier,
    pub seed: i64,
    #[write(as = VarInt)]
    pub game_type: GameType,
    #[write(as = OptionVarInt)]
    pub previous_game_type: Option<GameType>,
    pub is_debug: bool,
    pub is_flat: bool,
    pub last_death_location: Option<GlobalPos>,
    #[write(as = VarInt)]
    pub portal_cooldown: i32,
    #[write(as = VarInt)]
    pub sea_level: i32,
}

impl CommonPlayerSpawnInfo {
    /// Writes the spawn info for a client on an older protocol version.
    ///
    /// Two things changed between 1.21 and 26.3. 26.3 turned the game modes into var ints
    /// (with the previous game mode as an optional var int) where older clients expect a
    /// byte and a signed byte with `-1` for "none". 1.21.2 added the sea level, so 1.21
    /// clients do not receive it at all.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_legacy(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> io::Result<()> {
        steel_utils::codec::VarInt(self.dimension_type).write(writer)?;
        self.dimension.write(writer)?;
        self.seed.write(writer)?;
        if version.at_least(MinecraftVersion::V26_3) {
            steel_utils::codec::VarInt(self.game_type as i32).write(writer)?;
            steel_utils::codec::VarInt(
                self.previous_game_type
                    .map_or(0, |game_type| game_type as i32 + 1),
            )
            .write(writer)?;
        } else {
            (self.game_type as i8 as u8).write(writer)?;
            self.previous_game_type
                .map_or(-1, |game_type| game_type as i8)
                .write(writer)?;
        }
        self.is_debug.write(writer)?;
        self.is_flat.write(writer)?;
        self.last_death_location.write(writer)?;
        steel_utils::codec::VarInt(self.portal_cooldown).write(writer)?;
        if version.at_least(MinecraftVersion::V1_21_2) {
            steel_utils::codec::VarInt(self.sea_level).write(writer)?;
        }
        Ok(())
    }
}

/// Initializes a player's world state.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_LOGIN)]
#[legacy_payload]
pub struct CLogin {
    pub player_id: i32,
    pub hardcore: bool,
    #[write(as = Prefixed(VarInt))]
    pub levels: Vec<Identifier>,
    #[write(as = VarInt)]
    pub max_players: i32,
    #[write(as = VarInt)]
    pub chunk_radius: i32,
    #[write(as = VarInt)]
    pub simulation_distance: i32,
    pub reduced_debug_info: bool,
    pub show_death_screen: bool,
    pub do_limited_crafting: bool,
    pub common_player_spawn_info: CommonPlayerSpawnInfo,
    pub online_mode: bool,
    pub enforces_secure_chat: bool,
}

impl CLogin {
    /// Writes the body for a client on an older protocol version.
    ///
    /// `online_mode` was added in 26.2; older clients do not know the field. Everything
    /// else is a per-field version check inside [`CommonPlayerSpawnInfo::write_legacy`].
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> io::Result<()> {
        self.player_id.write(writer)?;
        self.hardcore.write(writer)?;
        self.levels
            .write_prefixed::<steel_utils::codec::VarInt>(writer)?;
        steel_utils::codec::VarInt(self.max_players).write(writer)?;
        steel_utils::codec::VarInt(self.chunk_radius).write(writer)?;
        steel_utils::codec::VarInt(self.simulation_distance).write(writer)?;
        self.reduced_debug_info.write(writer)?;
        self.show_death_screen.write(writer)?;
        self.do_limited_crafting.write(writer)?;
        self.common_player_spawn_info
            .write_legacy(writer, version)?;
        if version.at_least(MinecraftVersion::V26_2) {
            self.online_mode.write(writer)?;
        }
        self.enforces_secure_chat.write(writer)
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::Identifier;
    use steel_utils::serial::WriteTo as _;

    use super::{CLogin, CommonPlayerSpawnInfo};
    use crate::version::MinecraftVersion;

    fn spawn_info() -> CommonPlayerSpawnInfo {
        CommonPlayerSpawnInfo {
            dimension_type: 3,
            dimension: Identifier::vanilla_static("overworld"),
            seed: 42,
            game_type: steel_utils::types::GameType::Creative,
            previous_game_type: Some(steel_utils::types::GameType::Survival),
            is_debug: false,
            is_flat: true,
            last_death_location: None,
            portal_cooldown: 0,
            sea_level: 63,
        }
    }

    fn login() -> CLogin {
        CLogin {
            player_id: 5,
            hardcore: false,
            levels: vec![Identifier::vanilla_static("overworld")],
            max_players: 20,
            chunk_radius: 10,
            simulation_distance: 8,
            reduced_debug_info: false,
            show_death_screen: true,
            do_limited_crafting: false,
            common_player_spawn_info: spawn_info(),
            online_mode: true,
            enforces_secure_chat: true,
        }
    }

    fn encode_legacy(packet: &CLogin, version: MinecraftVersion) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet
            .write_payload_legacy(&mut bytes, version)
            .expect("legacy payload should encode");
        bytes
    }

    #[test]
    fn legacy_payload_26_3_matches_native() {
        let packet = login();
        let mut native = Vec::new();
        packet.write(&mut native).expect("native encode");
        assert_eq!(encode_legacy(&packet, MinecraftVersion::V26_3), native);
    }

    #[test]
    fn legacy_payload_drops_online_mode_before_26_2() {
        let packet = login();
        let modern = encode_legacy(&packet, MinecraftVersion::V26_2);
        let before = encode_legacy(&packet, MinecraftVersion::V26_1);

        assert_eq!(
            modern.len(),
            before.len() + 1,
            "26.2 adds the online mode flag"
        );
        assert_eq!(
            modern[modern.len() - 2],
            1,
            "online mode sits before the secure chat flag"
        );
        let mut without_online_mode = modern.clone();
        without_online_mode.remove(modern.len() - 2);
        assert_eq!(before, without_online_mode);
    }

    #[test]
    fn legacy_payload_drops_sea_level_on_1_21() {
        let packet = login();
        let with_sea_level = encode_legacy(&packet, MinecraftVersion::V1_21_2);
        let without = encode_legacy(&packet, MinecraftVersion::V1_21);

        assert_eq!(
            with_sea_level.len(),
            without.len() + 1,
            "sea level is a single var int byte"
        );
    }

    #[test]
    fn legacy_payload_writes_game_modes_as_bytes_before_26_3() {
        let mut info = spawn_info();
        info.game_type = steel_utils::types::GameType::Adventure;
        info.previous_game_type = None;

        let mut bytes = Vec::new();
        info.write_legacy(&mut bytes, MinecraftVersion::V26_2)
            .expect("legacy spawn info should encode");

        // Skip the dimension type, dimension name and seed to reach the game modes.
        let mut expected_prefix = Vec::new();
        steel_utils::codec::VarInt(3)
            .write(&mut expected_prefix)
            .expect("dimension type");
        Identifier::vanilla_static("overworld")
            .write(&mut expected_prefix)
            .expect("dimension");
        42_i64.write(&mut expected_prefix).expect("seed");

        assert_eq!(&bytes[..expected_prefix.len()], expected_prefix.as_slice());
        assert_eq!(bytes[expected_prefix.len()], 2, "adventure is game mode 2");
        assert_eq!(
            bytes[expected_prefix.len() + 1],
            (-1i8) as u8,
            "no previous game mode is -1"
        );
    }
}
