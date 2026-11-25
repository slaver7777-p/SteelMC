//! Packet sent to spawn an entity (including players) for the client.

use glam::DVec3;
use steel_macros::ClientPacket;
use steel_registry::packets::play::C_ADD_ENTITY;
use steel_utils::codec::{LpVec3, VarInt};
use steel_utils::serial::WriteTo;
use uuid::Uuid;

use crate::legacy::write_velocity_legacy;
use crate::version::MinecraftVersion;

/// Spawns an entity on the client.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_ADD_ENTITY)]
#[legacy_payload]
pub struct CAddEntity {
    /// The entity ID (used for all future references to this entity)
    pub id: i32,
    /// The entity's UUID
    pub uuid: Uuid,
    /// The entity type (from registry)
    pub entity_type: i32,
    /// The entity position
    pub position: DVec3,
    /// The entity velocity (blocks per tick)
    pub velocity: DVec3,
    /// Pitch (vertical rotation) as angle byte
    pub x_rot: i8,
    /// Yaw (horizontal rotation) as angle byte
    pub y_rot: i8,
    /// Head yaw as angle byte
    pub head_y_rot: i8,
    /// Entity data value (varies by entity type)
    pub data: i32,
}

impl WriteTo for CAddEntity {
    fn write(&self, writer: &mut impl std::io::Write) -> std::io::Result<()> {
        VarInt(self.id).write(writer)?;
        self.uuid.write(writer)?;
        VarInt(self.entity_type).write(writer)?;
        writer.write_all(&self.position.x.to_be_bytes())?;
        writer.write_all(&self.position.y.to_be_bytes())?;
        writer.write_all(&self.position.z.to_be_bytes())?;

        // Write velocity as LpVec3
        write_lp_vec3(writer, self.velocity)?;

        self.x_rot.write(writer)?;
        self.y_rot.write(writer)?;
        self.head_y_rot.write(writer)?;
        VarInt(self.data).write(writer)
    }
}

impl CAddEntity {
    /// Writes the body for an older client.
    ///
    /// Before 1.21.9 velocity is three fixed-point shorts placed *after* the data value, not
    /// an `LpVec3` between position and rotation, so the field order itself changes.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl std::io::Write,
        version: MinecraftVersion,
    ) -> std::io::Result<()> {
        if version.at_least(MinecraftVersion::V1_21_9) {
            return self.write(writer);
        }
        VarInt(self.id).write(writer)?;
        self.uuid.write(writer)?;
        VarInt(self.entity_type).write(writer)?;
        writer.write_all(&self.position.x.to_be_bytes())?;
        writer.write_all(&self.position.y.to_be_bytes())?;
        writer.write_all(&self.position.z.to_be_bytes())?;
        self.x_rot.write(writer)?;
        self.y_rot.write(writer)?;
        self.head_y_rot.write(writer)?;
        VarInt(self.data).write(writer)?;
        write_velocity_legacy(self.velocity, version, writer)
    }
}

/// Writes a velocity vector in `LpVec3` format.
///
/// Zero velocity is encoded as a single 0 byte.
/// Non-zero velocity uses 6+ bytes with bit-packed components.
pub fn write_lp_vec3(writer: &mut impl std::io::Write, velocity: DVec3) -> std::io::Result<()> {
    LpVec3(velocity).write(writer)
}

impl CAddEntity {
    /// Creates a new `CAddEntity` packet for spawning a player.
    #[must_use]
    pub const fn player(
        id: i32,
        uuid: Uuid,
        entity_type_id: i32,
        position: DVec3,
        yaw: f32,
        pitch: f32,
    ) -> Self {
        Self {
            id,
            uuid,
            entity_type: entity_type_id,
            position,
            velocity: DVec3::ZERO,
            x_rot: super::to_angle_byte(pitch),
            y_rot: super::to_angle_byte(yaw),
            head_y_rot: super::to_angle_byte(yaw),
            data: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_velocity() {
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, DVec3::ZERO).unwrap();
        assert_eq!(buf, vec![0]);
    }

    #[test]
    fn test_tiny_velocity_is_zero() {
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, DVec3::splat(1e-6)).unwrap();
        assert_eq!(buf, vec![0]);
    }

    #[test]
    fn test_non_zero_velocity() {
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, DVec3::ZERO.with_x(1.0)).unwrap();
        // Non-zero velocity should be 6 bytes (no continuation needed for scale=1)
        assert_eq!(buf.len(), 6);
    }

    #[test]
    fn test_negative_velocity_uses_absolute_scale() {
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, DVec3::ZERO.with_x(-1.0)).unwrap();
        assert_eq!(buf.len(), 6);
    }

    #[test]
    fn test_velocity_with_scale() {
        // Test velocity that requires scale > 3 (continuation bit)
        let mut buf = Vec::new();
        write_lp_vec3(&mut buf, DVec3::ZERO.with_x(5.0)).unwrap();
        // scale=5, which is > 3, so needs continuation
        // First byte should have continuation flag set (bit 2)
        assert_eq!(buf[0] & 0x04, 0x04, "Continuation flag should be set");
        // Should be 6 bytes + VarInt for scale
        assert!(buf.len() > 6, "Should have continuation VarInt");
    }

    fn spawn() -> CAddEntity {
        CAddEntity {
            id: 9,
            uuid: Uuid::nil(),
            entity_type: 3,
            position: DVec3::new(1.5, 64.0, -2.25),
            velocity: DVec3::new(0.4, -0.1, 0.0),
            x_rot: 10,
            y_rot: 20,
            head_y_rot: 20,
            data: 0,
        }
    }

    #[test]
    fn payload_from_1_21_9_matches_native() {
        let packet = spawn();
        let mut legacy = Vec::new();
        packet
            .write_payload_legacy(&mut legacy, MinecraftVersion::V1_21_9)
            .expect("legacy payload should encode");
        let mut native = Vec::new();
        packet
            .write(&mut native)
            .expect("native payload should encode");
        assert_eq!(legacy, native);
    }

    #[test]
    fn payload_before_1_21_9_moves_velocity_after_data() {
        let packet = spawn();
        let mut legacy = Vec::new();
        packet
            .write_payload_legacy(&mut legacy, MinecraftVersion::V1_21)
            .expect("legacy payload should encode");
        let mut native = Vec::new();
        packet
            .write(&mut native)
            .expect("native payload should encode");

        // id(1) + uuid(16) + type(1) + pos(24) + 3 angle bytes = 45, then data(1), then
        // velocity as three shorts. Native instead has LpVec3 velocity after the position.
        assert_eq!(legacy.len(), 45 + 1 + 6);
        assert_ne!(legacy, native, "velocity must move to the tail");
        assert_eq!(&legacy[45..46], &[0], "data value stays a VarInt");

        let mut data = std::io::Cursor::new(&legacy[46..]);
        let vel = crate::legacy::read_velocity_legacy(&mut data, MinecraftVersion::V1_21)
            .expect("legacy velocity should decode");
        assert!((vel - packet.velocity).abs().max_element() < 0.001);
    }
}
