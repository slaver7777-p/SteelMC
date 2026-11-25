//! Clientbound login success packet - sent after successful authentication.

use std::io::{self, Write};

use steel_macros::ClientPacket;
use steel_registry::GameProfile;
use steel_registry::packets::login::C_LOGIN_FINISHED;
use steel_utils::codec::VarInt;
use steel_utils::serial::{PrefixedWrite, WriteTo};
use uuid::Uuid;

use crate::version::MinecraftVersion;

/// Sent when the player has been authenticated and the session is established.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Login = C_LOGIN_FINISHED)]
#[legacy_payload]
pub struct CLoginFinished<'a> {
    pub game_profile: &'a GameProfile,
    pub session_id: Uuid,
    /// Pre-1.21.2 clients receive this flag; 1.21.2 removed it because strict handling
    /// became the only mode.
    pub strict_error_handling: bool,
}

impl WriteTo for CLoginFinished<'_> {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        self.game_profile.write(writer)?;
        self.session_id.write(writer)
    }
}

impl<'a> CLoginFinished<'a> {
    #[must_use]
    pub const fn new(
        game_profile: &'a GameProfile,
        session_id: Uuid,
        strict_error_handling: bool,
    ) -> Self {
        Self {
            game_profile,
            session_id,
            strict_error_handling,
        }
    }

    /// Writes the body for a client on an older protocol version.
    ///
    /// The profile properties appeared in 1.19 and the session id in 26.2. Clients on
    /// 1.20.5 up to 1.21.1 also expect the strict error handling flag after the properties;
    /// 1.21.2 removed the field because strict handling became the only mode, so it is only
    /// written in that window and is always enabled there.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> io::Result<()> {
        self.game_profile.id().write(writer)?;
        self.game_profile.name().write_prefixed::<VarInt>(writer)?;
        if version.at_least(MinecraftVersion::V1_21) {
            self.game_profile.write_properties(writer)?;
        }
        if version.at_least(MinecraftVersion::V26_2) {
            self.session_id.write(writer)?;
        }
        if version.at_least(MinecraftVersion::V1_21) && !version.at_least(MinecraftVersion::V1_21_2)
        {
            self.strict_error_handling.write(writer)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use steel_registry::{GameProfile, ProfileProperty};
    use steel_utils::serial::WriteTo as _;
    use uuid::Uuid;

    use super::CLoginFinished;
    use crate::version::MinecraftVersion;

    fn profile() -> GameProfile {
        GameProfile::new(
            Uuid::from_u128(9),
            "Joiner".to_owned(),
            vec![
                ProfileProperty::new("textures".to_owned(), "value".to_owned(), None)
                    .expect("valid property"),
            ],
        )
        .expect("valid profile")
    }

    fn encode_legacy(packet: &CLoginFinished, version: MinecraftVersion) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet
            .write_payload_legacy(&mut bytes, version)
            .expect("legacy payload should encode");
        bytes
    }

    #[test]
    fn legacy_payload_26_3_matches_native() {
        let profile = profile();
        let packet = CLoginFinished::new(&profile, Uuid::from_u128(5), true);
        let mut native = Vec::new();
        packet.write(&mut native).expect("native encode");
        assert_eq!(encode_legacy(&packet, MinecraftVersion::V26_3), native);
    }

    #[test]
    fn legacy_payload_drops_session_id_before_26_2() {
        let profile = profile();
        let packet = CLoginFinished::new(&profile, Uuid::from_u128(5), true);
        let with = encode_legacy(&packet, MinecraftVersion::V26_2);
        let without = encode_legacy(&packet, MinecraftVersion::V26_1);
        assert_eq!(with.len(), without.len() + 16, "session id is 16 bytes");
    }
}
