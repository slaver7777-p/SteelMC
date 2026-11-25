use std::io::{self, Write};

use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{packets::play::C_LEVEL_PARTICLES, particle_type::ParticleData};
use steel_utils::serial::WriteTo;

use crate::version::MinecraftVersion;

/// Vanilla's `ClientboundLevelParticlesPacket.RandomizationType`: controls how
/// the client jitters particle position/velocity around the given distribution.
#[derive(WriteTo, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[write(as = i8)]
pub enum RandomizationType {
    #[default]
    Default = 0,
    Alternative = 1,
    AlternativeWithSpeed = 2,
}

/// Sent to create particles on the client.
///
/// The client samples particle positions and velocities from the supplied
/// distribution. A count of zero has Vanilla's special single-particle
/// velocity behavior.
#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_LEVEL_PARTICLES)]
#[legacy_payload]
pub struct CLevelParticles {
    pub particle: ParticleData,
    pub override_limiter: bool,
    pub always_show: bool,
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub x_dist: f32,
    pub y_dist: f32,
    pub z_dist: f32,
    pub x_max_speed: f32,
    pub y_max_speed: f32,
    pub z_max_speed: f32,
    #[write(as = VarInt)]
    pub count: i32,
    pub randomization_type: RandomizationType,
}

impl CLevelParticles {
    /// Writes the body for a client on an older protocol version.
    ///
    /// The particle id and its options payload were one block only from 26.3. Before that
    /// the id sits right before the options payload at the end of the packet. The per-axis
    /// speed fields and the randomization type are 26.3 additions; older clients take a
    /// single max speed and an `i32` count. The always-show flag only exists from 1.21.4 on.
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

        self.override_limiter.write(writer)?;
        if version.at_least(MinecraftVersion::V1_21_4) {
            self.always_show.write(writer)?;
        }
        self.x.write(writer)?;
        self.y.write(writer)?;
        self.z.write(writer)?;
        self.x_dist.write(writer)?;
        self.y_dist.write(writer)?;
        self.z_dist.write(writer)?;
        self.x_max_speed.write(writer)?;
        self.count.write(writer)?;
        self.particle.write_network_id(writer)?;
        self.particle.write_network_options(writer)
    }
}

#[cfg(test)]
mod tests {

    use steel_registry::init_vanilla_registry;
    use steel_registry::{RegistryEntry, vanilla_particle_types};
    use steel_utils::{codec::VarInt, serial::WriteTo};

    use super::{CLevelParticles, RandomizationType};
    use crate::version::MinecraftVersion;

    #[test]
    fn writes_fields_in_vanilla_wire_order() {
        init_vanilla_registry();

        let packet = CLevelParticles {
            particle: steel_registry::particle_type::ParticleData::simple(
                &vanilla_particle_types::FLAME,
            ),
            override_limiter: true,
            always_show: false,
            x: 1.25,
            y: -2.5,
            z: 3.75,
            x_dist: 0.5,
            y_dist: -1.0,
            z_dist: 2.0,
            x_max_speed: 0.125,
            y_max_speed: 0.25,
            z_max_speed: 0.5,
            count: -7,
            randomization_type: RandomizationType::Alternative,
        };

        let mut encoded = Vec::new();
        let Ok(()) = packet.write(&mut encoded) else {
            panic!("level particles packet should encode");
        };

        let mut expected = Vec::new();
        let Ok(flame_id) = i32::try_from(vanilla_particle_types::FLAME.id()) else {
            panic!("flame particle id should fit in i32");
        };
        let Ok(()) = VarInt(flame_id).write(&mut expected) else {
            panic!("flame particle id should encode");
        };
        expected.extend_from_slice(&[1, 0]);
        expected.extend_from_slice(&1.25_f64.to_be_bytes());
        expected.extend_from_slice(&(-2.5_f64).to_be_bytes());
        expected.extend_from_slice(&3.75_f64.to_be_bytes());
        expected.extend_from_slice(&0.5_f32.to_be_bytes());
        expected.extend_from_slice(&(-1.0_f32).to_be_bytes());
        expected.extend_from_slice(&2.0_f32.to_be_bytes());
        expected.extend_from_slice(&0.125_f32.to_be_bytes());
        expected.extend_from_slice(&0.25_f32.to_be_bytes());
        expected.extend_from_slice(&0.5_f32.to_be_bytes());
        let Ok(()) = VarInt(-7).write(&mut expected) else {
            panic!("count should encode");
        };
        expected.push(1);

        assert_eq!(encoded, expected);
    }

    fn sample_packet() -> CLevelParticles {
        CLevelParticles {
            particle: steel_registry::particle_type::ParticleData::simple(
                &vanilla_particle_types::FLAME,
            ),
            override_limiter: true,
            always_show: true,
            x: 1.25,
            y: -2.5,
            z: 3.75,
            x_dist: 0.5,
            y_dist: -1.0,
            z_dist: 2.0,
            x_max_speed: 0.125,
            y_max_speed: 0.25,
            z_max_speed: 0.5,
            count: 7,
            randomization_type: RandomizationType::Default,
        }
    }

    fn encode_legacy(packet: &CLevelParticles, version: MinecraftVersion) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet
            .write_payload_legacy(&mut bytes, version)
            .expect("legacy payload should encode");
        bytes
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
    fn legacy_payload_moves_particle_to_the_tail_before_26_3() {
        init_vanilla_registry();
        let packet = sample_packet();
        let bytes = encode_legacy(&packet, MinecraftVersion::V26_2);

        let mut expected = Vec::new();
        expected.push(1); // override limiter
        expected.push(1); // always show (1.21.4+)
        expected.extend_from_slice(&1.25_f64.to_be_bytes());
        expected.extend_from_slice(&(-2.5_f64).to_be_bytes());
        expected.extend_from_slice(&3.75_f64.to_be_bytes());
        expected.extend_from_slice(&0.5_f32.to_be_bytes());
        expected.extend_from_slice(&(-1.0_f32).to_be_bytes());
        expected.extend_from_slice(&2.0_f32.to_be_bytes());
        expected.extend_from_slice(&0.125_f32.to_be_bytes());
        expected.extend_from_slice(&7_i32.to_be_bytes());
        let flame_id = i32::try_from(vanilla_particle_types::FLAME.id()).expect("fits");
        VarInt(flame_id).write(&mut expected).expect("id");

        assert_eq!(bytes, expected);
    }

    #[test]
    fn legacy_payload_drops_always_show_before_1_21_4() {
        init_vanilla_registry();
        let packet = sample_packet();
        let with = encode_legacy(&packet, MinecraftVersion::V1_21_4);
        let without = encode_legacy(&packet, MinecraftVersion::V1_21);
        assert_eq!(with.len(), without.len() + 1, "always-show is one byte");
    }

    #[test]
    fn legacy_payload_before_1_21_4_has_no_flag_after_limiter() {
        init_vanilla_registry();
        let packet = sample_packet();
        let bytes = encode_legacy(&packet, MinecraftVersion::V1_21);
        assert_eq!(bytes[0], 1, "override limiter comes first");
        assert_eq!(
            &bytes[1..9],
            &1.25_f64.to_be_bytes(),
            "position follows immediately without an always-show byte"
        );
    }
}
