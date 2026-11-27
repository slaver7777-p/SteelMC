use std::io::{self, Write};

use glam::DVec3;
use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{
    packets::play::C_EXPLODE, particle_type::ParticleData, sound_event::SoundEventHolder,
};
use steel_utils::codec::VarInt;
use steel_utils::serial::WriteTo;
use thiserror::Error;

use crate::version::MinecraftVersion;

/// Vanilla's `Explosion.BlockInteraction.KEEP` ordinal.
const BLOCK_INTERACTION_KEEP: i32 = 0;

/// A block-debris particle and the client-side scaling applied to it.
#[derive(WriteTo, Clone, Debug)]
pub struct ExplosionParticleInfo {
    pub particle: ParticleData,
    pub scaling: f32,
    pub speed: f32,
}

/// A weighted entry in an explosion's block-particle palette.
#[derive(WriteTo, Clone, Debug)]
struct WeightedExplosionParticleInfo {
    value: ExplosionParticleInfo,
    #[write(as = VarInt)]
    weight: i32,
}

/// A validated weighted palette for an explosion's block particles.
#[derive(WriteTo, Clone, Debug)]
pub struct ExplosionParticlePalette(Vec<WeightedExplosionParticleInfo>);

impl ExplosionParticlePalette {
    /// Returns a palette without block particles.
    #[must_use]
    pub const fn empty() -> Self {
        Self(Vec::new())
    }

    /// Validates Vanilla's non-negative weights and signed 32-bit list limits.
    pub fn try_new(
        entries: Vec<(ExplosionParticleInfo, i32)>,
    ) -> Result<Self, ExplosionParticleWeightError> {
        if i32::try_from(entries.len()).is_err() {
            return Err(ExplosionParticleWeightError::TooManyEntries(entries.len()));
        }

        let mut total = 0_i32;
        let mut weighted = Vec::with_capacity(entries.len());
        for (value, weight) in entries {
            if weight < 0 {
                return Err(ExplosionParticleWeightError::Negative(weight));
            }
            total = total
                .checked_add(weight)
                .ok_or(ExplosionParticleWeightError::TotalOverflow)?;
            weighted.push(WeightedExplosionParticleInfo { value, weight });
        }
        Ok(Self(weighted))
    }
}

/// An invalid block-particle weight supplied for an explosion packet.
#[derive(Error, Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExplosionParticleWeightError {
    /// Vanilla rejects individual negative weights.
    #[error("explosion particle weight cannot be negative: {0}")]
    Negative(i32),
    /// Vanilla rejects weighted lists whose total exceeds `i32::MAX`.
    #[error("total explosion particle weight exceeds i32::MAX")]
    TotalOverflow,
    /// Vanilla's list codec uses a signed 32-bit entry count.
    #[error("explosion particle palette has too many entries: {0}")]
    TooManyEntries(usize),
}

/// Sent to apply the client-visible effects of a server-side explosion.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_EXPLODE)]
#[legacy_payload]
pub struct CExplode {
    pub center: DVec3,
    pub radius: f32,
    pub block_count: i32,
    pub player_knockback: Option<DVec3>,
    pub explosion_particle: ParticleData,
    pub explosion_sound: SoundEventHolder,
    pub block_particles: ExplosionParticlePalette,
    pub play_sound: bool,
}

impl CExplode {
    /// Writes the body for a client on an older protocol version.
    ///
    /// Wire layout evolved through four stages:
    ///
    /// **1.21** — `ClientboundExplodePacket` was still a plain class: center
    /// (double×3), radius (float), the destroyed `List<BlockPos>`, knockback as
    /// three `float`s, a `BlockInteraction` ordinal, the small particle, the large
    /// particle, and finally the sound holder.
    ///
    /// **1.21.2–1.21.8** — the packet became a record of (center,
    /// `Optional<Vec3>` knockback, particle, sound). Radius, block count, the
    /// weighted list, and `playSound` do not exist yet.
    ///
    /// **1.21.9–26.2** — the record gained radius, `blockCount`, `playerKnockback`,
    /// and the `WeightedList<ExplosionParticleInfo>` block-particle palette.
    ///
    /// **26.3+** — the record gained a trailing `playSound` boolean.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> io::Result<()> {
        if version.at_least(MinecraftVersion::V26_3) {
            return self.write(writer);
        }

        if version.at_least(MinecraftVersion::V1_21_9) {
            self.center.write(writer)?;
            self.radius.write(writer)?;
            self.block_count.write(writer)?;
            self.player_knockback.write(writer)?;
            self.explosion_particle.write(writer)?;
            self.explosion_sound.write(writer)?;
            self.block_particles.write(writer)?;
            return Ok(());
        }

        if version.at_least(MinecraftVersion::V1_21_2) {
            self.center.write(writer)?;
            self.player_knockback.write(writer)?;
            self.explosion_particle.write(writer)?;
            self.explosion_sound.write(writer)?;
            return Ok(());
        }

        // 1.21: old format — center (double×3), radius (float),
        // List<BlockPos> (empty in Steel), knockback (float×3),
        // BlockInteraction enum (ordinal), small particle, large particle, sound.
        self.center.write(writer)?;
        self.radius.write(writer)?;
        // Block list is always empty — Steel doesn't track per-block positions.
        VarInt(0).write(writer)?;
        // Knockback: Vanilla's `g`, `h`, `i` fields are three `float`s.
        let knockback = self.player_knockback.unwrap_or(DVec3::ZERO);
        (knockback.x as f32).write(writer)?;
        (knockback.y as f32).write(writer)?;
        (knockback.z as f32).write(writer)?;
        // BlockInteraction enum: ordinal 0 = KEEP (no blocks affected).
        VarInt(BLOCK_INTERACTION_KEEP).write(writer)?;
        // Vanilla 1.21 carries both the small and the large explosion particle; the
        // client picks one from the radius and whether blocks were hit. Steel resolves
        // that choice server-side, so both slots carry the same particle.
        self.explosion_particle.write(writer)?;
        self.explosion_particle.write(writer)?;
        // Sound.
        self.explosion_sound.write(writer)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use glam::DVec3;
    use steel_registry::{
        RegistryEntry, init_vanilla_registry, particle_type::ParticleData,
        sound_event::SoundEventHolder, sound_events, vanilla_particle_types,
    };
    use steel_utils::{Identifier, codec::VarInt, serial::WriteTo};

    use super::{
        CExplode, ExplosionParticleInfo, ExplosionParticlePalette, ExplosionParticleWeightError,
    };
    use crate::version::MinecraftVersion;

    #[test]
    fn writes_fields_in_vanilla_wire_order() {
        init_vanilla_registry();
        let packet = CExplode {
            center: DVec3::new(1.25, -2.5, 3.75),
            radius: 4.0,
            block_count: 17,
            player_knockback: Some(DVec3::new(-0.25, 0.5, 1.0)),
            explosion_particle: ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            explosion_sound: SoundEventHolder::registry(&sound_events::ENTITY_GENERIC_EXPLODE),
            block_particles: palette(vec![
                (
                    ExplosionParticleInfo {
                        particle: ParticleData::simple(&vanilla_particle_types::POOF),
                        scaling: 0.5,
                        speed: 1.0,
                    },
                    2,
                ),
                (
                    ExplosionParticleInfo {
                        particle: ParticleData::simple(&vanilla_particle_types::SMOKE),
                        scaling: 1.0,
                        speed: 0.75,
                    },
                    3,
                ),
            ]),
            play_sound: true,
        };

        let mut encoded = Vec::new();
        assert!(packet.write(&mut encoded).is_ok());

        let mut expected = Vec::new();
        expected.extend_from_slice(&1.25_f64.to_be_bytes());
        expected.extend_from_slice(&(-2.5_f64).to_be_bytes());
        expected.extend_from_slice(&3.75_f64.to_be_bytes());
        expected.extend_from_slice(&4.0_f32.to_be_bytes());
        expected.extend_from_slice(&17_i32.to_be_bytes());
        expected.push(1);
        expected.extend_from_slice(&(-0.25_f64).to_be_bytes());
        expected.extend_from_slice(&0.5_f64.to_be_bytes());
        expected.extend_from_slice(&1.0_f64.to_be_bytes());
        write_registry_id(&mut expected, vanilla_particle_types::EXPLOSION.id());
        assert!(
            VarInt(sound_events::ENTITY_GENERIC_EXPLODE.packet_holder_id())
                .write(&mut expected)
                .is_ok()
        );

        assert!(VarInt(2).write(&mut expected).is_ok());
        write_registry_id(&mut expected, vanilla_particle_types::POOF.id());
        expected.extend_from_slice(&0.5_f32.to_be_bytes());
        expected.extend_from_slice(&1.0_f32.to_be_bytes());
        assert!(VarInt(2).write(&mut expected).is_ok());
        write_registry_id(&mut expected, vanilla_particle_types::SMOKE.id());
        expected.extend_from_slice(&1.0_f32.to_be_bytes());
        expected.extend_from_slice(&0.75_f32.to_be_bytes());
        assert!(VarInt(3).write(&mut expected).is_ok());
        expected.push(1);

        assert_eq!(encoded, expected);
    }

    #[test]
    fn writes_direct_explosion_sound_holder() {
        init_vanilla_registry();
        let sound_id = Identifier::from_steel("test_explosion");
        let packet = CExplode {
            center: DVec3::ZERO,
            radius: 0.0,
            block_count: 0,
            player_knockback: None,
            explosion_particle: ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            explosion_sound: SoundEventHolder::Direct {
                sound_id: sound_id.clone(),
                fixed_range: Some(24.0),
            },
            block_particles: palette(Vec::new()),
            play_sound: false,
        };

        let mut encoded = Vec::new();
        assert!(packet.write(&mut encoded).is_ok());

        let mut expected = vec![0; 24];
        expected.extend_from_slice(&0.0_f32.to_be_bytes());
        expected.extend_from_slice(&0_i32.to_be_bytes());
        expected.push(0);
        write_registry_id(&mut expected, vanilla_particle_types::EXPLOSION.id());
        assert!(VarInt(0).write(&mut expected).is_ok());
        assert!(sound_id.write(&mut expected).is_ok());
        expected.push(1);
        expected.extend_from_slice(&24.0_f32.to_be_bytes());
        assert!(VarInt(0).write(&mut expected).is_ok());

        expected.push(0);
        assert_eq!(encoded, expected);
    }

    #[test]
    fn rejects_invalid_explosion_particle_weights() {
        let particle = || ExplosionParticleInfo {
            particle: ParticleData::simple(&vanilla_particle_types::POOF),
            scaling: 1.0,
            speed: 1.0,
        };

        assert!(matches!(
            ExplosionParticlePalette::try_new(vec![(particle(), -1)]),
            Err(ExplosionParticleWeightError::Negative(-1))
        ));
        assert!(matches!(
            ExplosionParticlePalette::try_new(vec![(particle(), i32::MAX), (particle(), 1),]),
            Err(ExplosionParticleWeightError::TotalOverflow)
        ));
    }

    fn palette(entries: Vec<(ExplosionParticleInfo, i32)>) -> ExplosionParticlePalette {
        let Ok(palette) = ExplosionParticlePalette::try_new(entries) else {
            panic!("test explosion particle palette should be valid");
        };
        palette
    }

    fn write_registry_id(output: &mut Vec<u8>, id: usize) {
        let Ok(id) = i32::try_from(id) else {
            panic!("test registry id should fit in i32");
        };
        assert!(VarInt(id).write(output).is_ok());
    }

    fn sample_packet() -> CExplode {
        CExplode {
            center: DVec3::new(1.25, -2.5, 3.75),
            radius: 4.0,
            block_count: 17,
            player_knockback: Some(DVec3::new(-0.25, 0.5, 1.0)),
            explosion_particle: ParticleData::simple(&vanilla_particle_types::EXPLOSION),
            explosion_sound: SoundEventHolder::registry(&sound_events::ENTITY_GENERIC_EXPLODE),
            block_particles: palette(vec![(
                ExplosionParticleInfo {
                    particle: ParticleData::simple(&vanilla_particle_types::POOF),
                    scaling: 0.5,
                    speed: 1.0,
                },
                2,
            )]),
            play_sound: true,
        }
    }

    fn encode_legacy(packet: &CExplode, version: MinecraftVersion) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet
            .write_payload_legacy(&mut bytes, version)
            .expect("legacy payload should encode");
        bytes
    }

    fn write_center(output: &mut Vec<u8>, center: DVec3) {
        output.extend_from_slice(&center.x.to_be_bytes());
        output.extend_from_slice(&center.y.to_be_bytes());
        output.extend_from_slice(&center.z.to_be_bytes());
    }

    fn write_knockback_f32(output: &mut Vec<u8>, knockback: Option<DVec3>) {
        let knockback = knockback.unwrap_or(DVec3::ZERO);
        output.extend_from_slice(&(knockback.x as f32).to_be_bytes());
        output.extend_from_slice(&(knockback.y as f32).to_be_bytes());
        output.extend_from_slice(&(knockback.z as f32).to_be_bytes());
    }

    #[test]
    fn legacy_payload_26_3_matches_native() {
        init_vanilla_registry();
        let packet = sample_packet();
        let mut native = Vec::new();
        packet.write(&mut native).expect("native encode");
        assert_eq!(encode_legacy(&packet, MinecraftVersion::V26_3), native);
    }

    #[test]
    fn legacy_payload_26_2_drops_only_play_sound() {
        init_vanilla_registry();
        let packet = sample_packet();

        let mut expected = Vec::new();
        write_center(&mut expected, packet.center);
        expected.extend_from_slice(&packet.radius.to_be_bytes());
        expected.extend_from_slice(&packet.block_count.to_be_bytes());
        expected.push(1);
        write_center(&mut expected, DVec3::new(-0.25, 0.5, 1.0));
        write_registry_id(&mut expected, vanilla_particle_types::EXPLOSION.id());
        assert!(
            VarInt(sound_events::ENTITY_GENERIC_EXPLODE.packet_holder_id())
                .write(&mut expected)
                .is_ok()
        );
        assert!(VarInt(1).write(&mut expected).is_ok());
        write_registry_id(&mut expected, vanilla_particle_types::POOF.id());
        expected.extend_from_slice(&0.5_f32.to_be_bytes());
        expected.extend_from_slice(&1.0_f32.to_be_bytes());
        assert!(VarInt(2).write(&mut expected).is_ok());

        assert_eq!(encode_legacy(&packet, MinecraftVersion::V26_2), expected);
    }

    #[test]
    fn legacy_payload_1_21_9_has_weighted_list_no_play_sound() {
        init_vanilla_registry();
        let packet = sample_packet();

        let mut expected = Vec::new();
        write_center(&mut expected, packet.center);
        expected.extend_from_slice(&packet.radius.to_be_bytes());
        expected.extend_from_slice(&packet.block_count.to_be_bytes());
        expected.push(1);
        write_center(&mut expected, DVec3::new(-0.25, 0.5, 1.0));
        write_registry_id(&mut expected, vanilla_particle_types::EXPLOSION.id());
        assert!(
            VarInt(sound_events::ENTITY_GENERIC_EXPLODE.packet_holder_id())
                .write(&mut expected)
                .is_ok()
        );
        // WeightedList: VarInt(1), POOF id, scaling f32, speed f32, weight VarInt(2)
        assert!(VarInt(1).write(&mut expected).is_ok());
        write_registry_id(&mut expected, vanilla_particle_types::POOF.id());
        expected.extend_from_slice(&0.5_f32.to_be_bytes());
        expected.extend_from_slice(&1.0_f32.to_be_bytes());
        assert!(VarInt(2).write(&mut expected).is_ok());

        assert_eq!(encode_legacy(&packet, MinecraftVersion::V1_21_9), expected);
    }

    #[test]
    fn legacy_payload_1_21_2_keeps_center_knockback_particle_sound() {
        init_vanilla_registry();
        let packet = sample_packet();

        let mut expected = Vec::new();
        write_center(&mut expected, packet.center);
        expected.push(1);
        write_center(&mut expected, DVec3::new(-0.25, 0.5, 1.0));
        write_registry_id(&mut expected, vanilla_particle_types::EXPLOSION.id());
        assert!(
            VarInt(sound_events::ENTITY_GENERIC_EXPLODE.packet_holder_id())
                .write(&mut expected)
                .is_ok()
        );

        assert_eq!(encode_legacy(&packet, MinecraftVersion::V1_21_2), expected);
    }

    #[test]
    fn legacy_payload_1_21_uses_block_list_enum_and_two_particles() {
        init_vanilla_registry();
        let packet = sample_packet();

        let mut expected = Vec::new();
        write_center(&mut expected, packet.center);
        expected.extend_from_slice(&packet.radius.to_be_bytes());
        expected.push(0); // empty block list
        write_knockback_f32(&mut expected, packet.player_knockback);
        expected.push(0); // BlockInteraction.KEEP ordinal
        write_registry_id(&mut expected, vanilla_particle_types::EXPLOSION.id());
        write_registry_id(&mut expected, vanilla_particle_types::EXPLOSION.id());
        assert!(
            VarInt(sound_events::ENTITY_GENERIC_EXPLODE.packet_holder_id())
                .write(&mut expected)
                .is_ok()
        );

        assert_eq!(encode_legacy(&packet, MinecraftVersion::V1_21), expected);
    }

    #[test]
    fn legacy_payload_1_21_writes_zero_knockback_when_absent() {
        init_vanilla_registry();
        let packet = CExplode {
            player_knockback: None,
            ..sample_packet()
        };

        let legacy = encode_legacy(&packet, MinecraftVersion::V1_21);
        // center (24) + radius (4) + empty block list (1) = 29 bytes before knockback.
        let knockback = &legacy[29..41];
        assert_eq!(knockback, &[0_u8; 12]);
    }
}
