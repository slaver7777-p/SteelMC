//! Clientbound entity position sync packet - authoritative entity position update.

use std::io::{self, Write};

use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_ENTITY_POSITION_SYNC;
use steel_utils::codec::VarInt;
use steel_utils::serial::{PrefixedWrite, WriteTo};

use crate::version::MinecraftVersion;

/// One position along a stepped movement path.
#[derive(Clone, Copy, Debug, PartialEq, WriteTo)]
pub struct PositionStep {
    /// Position reached at this step.
    pub position: DVec3,
    /// Ticks elapsed since the previous step.
    #[write(as = VarInt)]
    pub tick_offset: i32,
}

/// The path an entity took to reach its current position.
#[derive(Clone, Debug, PartialEq)]
pub enum PositionPath {
    /// The entity moved straight to a single position.
    Linear(DVec3),
    /// The entity moved through intermediate positions the client replays in order.
    Stepped(Vec<PositionStep>),
}

impl PositionPath {
    /// Returns the position the entity ends up at.
    #[must_use]
    pub fn end_position(&self) -> Option<DVec3> {
        match self {
            Self::Linear(position) => Some(*position),
            Self::Stepped(steps) => steps.last().map(|step| step.position),
        }
    }
}

impl WriteTo for PositionPath {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        match self {
            Self::Linear(position) => {
                VarInt(0).write(writer)?;
                position.write(writer)
            }
            Self::Stepped(steps) => {
                VarInt(1).write(writer)?;
                steps.write_prefixed::<VarInt>(writer)
            }
        }
    }
}

/// Synchronizes an entity's absolute position and rotation.
#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_ENTITY_POSITION_SYNC)]
#[legacy_payload]
pub struct CEntityPositionSync {
    /// The entity being synchronized.
    pub entity_id: i32,
    /// The path the entity took to its current position.
    pub path: PositionPath,
    /// The entity's current velocity, which pre-26.3 clients interpolate with.
    ///
    /// 26.3 replaced this wire field with the stepped `path`; older clients still need it,
    /// so it is kept here and only written for them.
    pub velocity: DVec3,
    /// Rotation around the Y axis, in degrees.
    pub y_rot: f32,
    /// Rotation around the X axis, in degrees.
    pub x_rot: f32,
    /// Whether the entity is on the ground.
    pub on_ground: bool,
}

impl WriteTo for CEntityPositionSync {
    fn write(&self, writer: &mut impl Write) -> io::Result<()> {
        VarInt(self.entity_id).write(writer)?;
        self.path.write(writer)?;
        self.y_rot.write(writer)?;
        self.x_rot.write(writer)?;
        self.on_ground.write(writer)
    }
}

impl CEntityPositionSync {
    /// Writes the body for a client on an older protocol version.
    ///
    /// 26.3 replaced the velocity field with the position path. Before it the packet is
    /// the entity id, the end position, the velocity and float angles; the path's steps do
    /// not exist, so only its end position is sent and the client interpolates from the
    /// velocity, exactly as those versions always did. Before 1.21.2 the angles are
    /// protocol angle bytes rather than floats.
    ///
    /// # Errors
    /// - If the packet fails to write.
    /// - If a stepped path has no end position to send.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> io::Result<()> {
        if version.at_least(MinecraftVersion::V26_3) {
            return self.write(writer);
        }
        let Some(position) = self.path.end_position() else {
            return Err(io::Error::other(
                "a stepped position path must end at a position",
            ));
        };
        VarInt(self.entity_id).write(writer)?;
        position.write(writer)?;
        self.velocity.write(writer)?;
        if version.at_least(MinecraftVersion::V1_21_2) {
            self.y_rot.write(writer)?;
            self.x_rot.write(writer)?;
        } else {
            super::to_angle_byte(self.y_rot).write(writer)?;
            super::to_angle_byte(self.x_rot).write(writer)?;
        }
        self.on_ground.write(writer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(packet: &CEntityPositionSync) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet.write(&mut bytes).expect("packet should encode");
        bytes
    }

    fn encode_legacy(packet: &CEntityPositionSync, version: MinecraftVersion) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet
            .write_payload_legacy(&mut bytes, version)
            .expect("legacy payload should write");
        bytes
    }

    #[test]
    fn legacy_payload_26_3_uses_native_format() {
        let packet = CEntityPositionSync {
            entity_id: 12,
            path: PositionPath::Stepped(vec![PositionStep {
                position: DVec3::new(1.0, 2.0, 3.0),
                tick_offset: 2,
            }]),
            velocity: DVec3::new(0.1, 0.2, 0.3),
            y_rot: 90.0,
            x_rot: 45.0,
            on_ground: true,
        };
        // 26.3+ uses native format (full write)
        let legacy = encode_legacy(&packet, MinecraftVersion::V26_3);
        let native = encode(&packet);
        assert_eq!(legacy, native);
    }

    #[test]
    fn legacy_payload_26_2_flattens_stepped_path() {
        let packet = CEntityPositionSync {
            entity_id: 42,
            path: PositionPath::Stepped(vec![
                PositionStep {
                    position: DVec3::new(0.0, 0.0, 0.0),
                    tick_offset: 0,
                },
                PositionStep {
                    position: DVec3::new(10.0, 20.0, 30.0),
                    tick_offset: 5,
                },
            ]),
            velocity: DVec3::new(0.5, 0.0, -0.25),
            y_rot: 180.0,
            x_rot: 90.0,
            on_ground: false,
        };
        let legacy = encode_legacy(&packet, MinecraftVersion::V26_2);

        // Legacy format: entity_id (varint), position (3x f64), velocity (3x f64), y_rot (f32), x_rot (f32), on_ground (bool)
        let mut expected = Vec::new();
        VarInt(42).write(&mut expected).unwrap();
        DVec3::new(10.0, 20.0, 30.0).write(&mut expected).unwrap();
        DVec3::new(0.5, 0.0, -0.25).write(&mut expected).unwrap();
        180.0_f32.write(&mut expected).unwrap();
        90.0_f32.write(&mut expected).unwrap();
        false.write(&mut expected).unwrap();

        assert_eq!(legacy, expected);
    }

    #[test]
    fn legacy_payload_linear_path_26_2_uses_end_position() {
        let packet = CEntityPositionSync {
            entity_id: 7,
            path: PositionPath::Linear(DVec3::new(5.5, 6.5, 7.5)),
            velocity: DVec3::ZERO,
            y_rot: 0.0,
            x_rot: 0.0,
            on_ground: true,
        };
        let legacy = encode_legacy(&packet, MinecraftVersion::V26_2);

        let mut expected = Vec::new();
        VarInt(7).write(&mut expected).unwrap();
        DVec3::new(5.5, 6.5, 7.5).write(&mut expected).unwrap();
        DVec3::ZERO.write(&mut expected).unwrap();
        0.0_f32.write(&mut expected).unwrap();
        0.0_f32.write(&mut expected).unwrap();
        true.write(&mut expected).unwrap();

        assert_eq!(legacy, expected);
    }

    #[test]
    fn legacy_payload_stepped_without_end_errors() {
        let packet = CEntityPositionSync {
            entity_id: 1,
            path: PositionPath::Stepped(vec![]),
            velocity: DVec3::ZERO,
            y_rot: 0.0,
            x_rot: 0.0,
            on_ground: false,
        };
        let mut bytes = Vec::new();
        let result = packet.write_payload_legacy(&mut bytes, MinecraftVersion::V26_2);
        assert!(
            result.is_err(),
            "stepped path with no steps should error in legacy"
        );
    }

    #[test]
    fn legacy_payload_1_21_9_flattens_stepped_path() {
        let packet = CEntityPositionSync {
            entity_id: 100,
            path: PositionPath::Stepped(vec![PositionStep {
                position: DVec3::new(100.0, 200.0, 300.0),
                tick_offset: 10,
            }]),
            velocity: DVec3::new(-1.0, 2.0, -3.0),
            y_rot: 270.0,
            x_rot: 135.0,
            on_ground: true,
        };
        let legacy = encode_legacy(&packet, MinecraftVersion::V1_21_9);

        let mut expected = Vec::new();
        VarInt(100).write(&mut expected).unwrap();
        DVec3::new(100.0, 200.0, 300.0)
            .write(&mut expected)
            .unwrap();
        DVec3::new(-1.0, 2.0, -3.0).write(&mut expected).unwrap();
        270.0_f32.write(&mut expected).unwrap();
        135.0_f32.write(&mut expected).unwrap();
        true.write(&mut expected).unwrap();

        assert_eq!(legacy, expected);
    }

    #[test]
    fn legacy_payload_1_21_uses_angle_bytes() {
        let packet = CEntityPositionSync {
            entity_id: 100,
            path: PositionPath::Linear(DVec3::new(1.0, 2.0, 3.0)),
            velocity: DVec3::new(-1.0, 2.0, -3.0),
            y_rot: 90.0,
            x_rot: -90.0,
            on_ground: true,
        };
        let legacy = encode_legacy(&packet, MinecraftVersion::V1_21);

        let mut expected = Vec::new();
        VarInt(100).write(&mut expected).unwrap();
        DVec3::new(1.0, 2.0, 3.0).write(&mut expected).unwrap();
        DVec3::new(-1.0, 2.0, -3.0).write(&mut expected).unwrap();
        super::super::to_angle_byte(90.0)
            .write(&mut expected)
            .unwrap();
        super::super::to_angle_byte(-90.0)
            .write(&mut expected)
            .unwrap();
        true.write(&mut expected).unwrap();

        assert_eq!(legacy, expected);
        assert_eq!(legacy[legacy.len() - 3], 64, "90 degrees packs to 64");
        assert_eq!(legacy[legacy.len() - 2], (-64i8) as u8);
    }

    #[test]
    fn linear_path_writes_type_then_position() {
        let bytes = encode(&CEntityPositionSync {
            entity_id: 12,
            path: PositionPath::Linear(DVec3::new(1.0, 2.0, 3.0)),
            velocity: DVec3::ZERO,
            y_rot: 0.0,
            x_rot: 0.0,
            on_ground: true,
        });

        let mut expected = vec![12, 0];
        for value in [1.0f64, 2.0, 3.0] {
            expected.extend_from_slice(&value.to_be_bytes());
        }
        expected.extend_from_slice(&0.0f32.to_be_bytes());
        expected.extend_from_slice(&0.0f32.to_be_bytes());
        expected.push(1);

        assert_eq!(bytes, expected);
    }

    #[test]
    fn stepped_path_writes_prefixed_steps() {
        let bytes = encode(&CEntityPositionSync {
            entity_id: 12,
            path: PositionPath::Stepped(vec![PositionStep {
                position: DVec3::new(1.0, 2.0, 3.0),
                tick_offset: 2,
            }]),
            velocity: DVec3::ZERO,
            y_rot: 0.0,
            x_rot: 0.0,
            on_ground: false,
        });

        let mut expected = vec![12, 1, 1];
        for value in [1.0f64, 2.0, 3.0] {
            expected.extend_from_slice(&value.to_be_bytes());
        }
        expected.push(2);
        expected.extend_from_slice(&0.0f32.to_be_bytes());
        expected.extend_from_slice(&0.0f32.to_be_bytes());
        expected.push(0);

        assert_eq!(bytes, expected);
    }
}
