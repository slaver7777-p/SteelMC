//! Clientbound set entity motion packet - sent to update entity velocity.

use std::io::{Result, Write};

use glam::DVec3;
use steel_macros::ClientPacket;
use steel_registry::packets::play::C_SET_ENTITY_MOTION;
use steel_utils::{codec::VarInt, serial::WriteTo};

use super::write_lp_vec3;
use crate::legacy::write_velocity_legacy;
use crate::version::MinecraftVersion;

/// Sent to update an entity's velocity on the client.
///
/// Velocity is sent in `LpVec3` format (same as spawn packet).
/// This is used for:
/// - Items landing on ground (velocity zeroed)
/// - Knockback effects
/// - Explosions
/// - Any physics-driven velocity change
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_SET_ENTITY_MOTION)]
#[legacy_payload]
pub struct CSetEntityMotion {
    /// The entity ID whose velocity is being updated.
    pub entity_id: i32,
    /// The entity velocity (blocks/tick).
    pub vel: DVec3,
}

impl CSetEntityMotion {
    /// Creates a new set entity motion packet.
    #[must_use]
    pub const fn new(entity_id: i32, vel: DVec3) -> Self {
        Self { entity_id, vel }
    }

    /// Writes the body, sending velocity as three fixed-point shorts before 1.21.9.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> Result<()> {
        VarInt(self.entity_id).write(writer)?;
        write_velocity_legacy(self.vel, version, writer)
    }
}

impl WriteTo for CSetEntityMotion {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.entity_id).write(writer)?;
        write_lp_vec3(writer, self.vel)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use steel_utils::serial::ReadFrom;

    use super::*;
    use crate::legacy::read_velocity_legacy;

    fn native_bytes(packet: &CSetEntityMotion) -> Vec<u8> {
        let mut buf = Vec::new();
        packet.write(&mut buf).expect("packet should encode");
        buf
    }

    #[test]
    fn payload_for_current_version_matches_native_write() {
        let packet = CSetEntityMotion::new(7, DVec3::new(0.2, -0.4, 1.5));
        let mut legacy = Vec::new();
        packet
            .write_payload_legacy(&mut legacy, MinecraftVersion::CURRENT)
            .expect("legacy payload should encode");
        assert_eq!(legacy, native_bytes(&packet));
    }

    #[test]
    fn payload_before_1_21_9_writes_three_shorts() {
        let packet = CSetEntityMotion::new(7, DVec3::new(0.2, -0.4, 1.5));
        let mut legacy = Vec::new();
        packet
            .write_payload_legacy(&mut legacy, MinecraftVersion::V1_21_7)
            .expect("legacy payload should encode");

        // Entity id is unchanged, velocity becomes exactly three big-endian shorts.
        assert_eq!(legacy[0], 7);
        assert_eq!(legacy.len(), 1 + 6);

        let mut data = Cursor::new(legacy.as_slice());
        assert_eq!(VarInt::read(&mut data).unwrap().0, 7);
        let vel = read_velocity_legacy(&mut data, MinecraftVersion::V1_21_7).unwrap();
        assert!((vel - packet.vel).abs().max_element() < 0.001);
    }
}
