//! Clientbound animate packet - sent to play an entity animation.

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_ANIMATE;
use steel_utils::codec::VarInt;
use steel_utils::serial::WriteTo as _;

use crate::version::MinecraftVersion;

/// Animation action types.
///
/// The discriminants are the 26.3 numbering. Before 26.3 the arm swings and hurt took up
/// slots 0, 1 and 3, so leave-bed/critical/magic were numbered 2, 4 and 5; [`CAnimate`]
/// rewrites these values for an older client.
#[derive(Debug, Clone, Copy, PartialEq, Eq, WriteTo)]
#[repr(u8)]
#[write(as = u8)]
pub enum AnimateAction {
    /// Wake up from bed
    WakeUp = 0,
    /// Critical hit effect
    CriticalHit = 1,
    /// Magic critical hit effect (enchanted weapon)
    MagicCriticalHit = 2,
}

impl AnimateAction {
    /// The byte a pre-26.3 client expects for this animation.
    const fn legacy_byte(self) -> u8 {
        match self {
            Self::WakeUp => 2,
            Self::CriticalHit => 4,
            Self::MagicCriticalHit => 5,
        }
    }
}

/// Sent to play an animation on an entity.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_ANIMATE)]
#[legacy_payload]
pub struct CAnimate {
    /// The entity ID to animate.
    #[write(as = VarInt)]
    pub entity_id: i32,
    /// The animation action to play.
    pub action: AnimateAction,
}

impl CAnimate {
    /// Writes the body for a client on an older protocol version.
    ///
    /// 26.3 split the arm swings out into their own packet and renumbered the remaining
    /// animations down to 0/1/2. Every supported legacy client still uses the old slots
    /// (2/4/5), so the action byte is remapped while the entity id stays a `VarInt`.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl std::io::Write,
        version: MinecraftVersion,
    ) -> std::io::Result<()> {
        if version.at_least(MinecraftVersion::V26_3) {
            return self.write(writer);
        }
        VarInt(self.entity_id).write(writer)?;
        self.action.legacy_byte().write(writer)
    }

    /// Creates a new animate packet.
    #[must_use]
    pub const fn new(entity_id: i32, action: AnimateAction) -> Self {
        Self { entity_id, action }
    }

    /// Creates a critical hit animation.
    #[must_use]
    pub const fn critical_hit(entity_id: i32) -> Self {
        Self::new(entity_id, AnimateAction::CriticalHit)
    }

    /// Creates a magic critical hit animation.
    #[must_use]
    pub const fn magic_critical_hit(entity_id: i32) -> Self {
        Self::new(entity_id, AnimateAction::MagicCriticalHit)
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::serial::WriteTo as _;

    use super::{AnimateAction, CAnimate, MinecraftVersion};

    fn legacy_bytes(action: AnimateAction, version: MinecraftVersion) -> Vec<u8> {
        let packet = CAnimate::new(7, action);
        let mut bytes = Vec::new();
        packet
            .write_payload_legacy(&mut bytes, version)
            .expect("legacy payload should encode");
        bytes
    }

    #[test]
    fn native_matches_plain_write() {
        for action in [
            AnimateAction::WakeUp,
            AnimateAction::CriticalHit,
            AnimateAction::MagicCriticalHit,
        ] {
            let packet = CAnimate::new(7, action);
            let mut native = Vec::new();
            packet.write(&mut native).expect("native encode");
            assert_eq!(
                legacy_bytes(action, MinecraftVersion::V26_3),
                native,
                "26.3 must be byte-identical to native"
            );
        }
    }

    #[test]
    fn before_26_3_renumbers_actions() {
        // 26.3 renumbered the actions down; every older client still uses the 2/4/5 slots.
        assert_eq!(
            legacy_bytes(AnimateAction::WakeUp, MinecraftVersion::V26_2),
            vec![7, 2]
        );
        assert_eq!(
            legacy_bytes(AnimateAction::CriticalHit, MinecraftVersion::V1_21),
            vec![7, 4]
        );
        assert_eq!(
            legacy_bytes(AnimateAction::MagicCriticalHit, MinecraftVersion::V1_21_9),
            vec![7, 5]
        );
    }
}
