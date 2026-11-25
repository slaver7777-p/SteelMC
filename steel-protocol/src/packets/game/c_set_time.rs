use std::io::{Result, Write};
use steel_macros::ClientPacket;
use steel_registry::packets::play::C_SET_TIME;
use steel_utils::codec::{VarInt, VarLong};
use steel_utils::serial::WriteTo;

use crate::version::MinecraftVersion;

#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_SET_TIME)]
#[legacy_payload]
pub struct CSetTime {
    pub game_time: i64,
    /// (`clock_registry_id`, `total_ticks`, `partial_tick`, rate)
    pub clock_updates: Vec<(i32, i64, f32, f32)>,
}

impl WriteTo for CSetTime {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        self.game_time.write(writer)?;
        VarInt(self.clock_updates.len() as i32).write(writer)?;
        for &(clock_id, total_ticks, partial_tick, rate) in &self.clock_updates {
            VarInt(clock_id).write(writer)?;
            VarLong(total_ticks).write(writer)?;
            partial_tick.write(writer)?;
            rate.write(writer)?;
        }
        Ok(())
    }
}

impl CSetTime {
    /// Writes the body for a client on an older protocol version.
    ///
    /// 26.1 introduced the clock-updates list; before it the packet sends the game time
    /// followed by a single day time taken from the first clock update. The day time is a
    /// fixed 8-byte value rather than the native `VarLong`, and pre-1.21.2 has no
    /// is-increasing flag: it signals a paused cycle by negating the day time.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> std::io::Result<()> {
        // 26.1+ is the native layout (game time followed by a clock-updates list).
        if version.at_least(MinecraftVersion::V26_1) {
            return self.write(writer);
        }

        self.game_time.write(writer)?;

        let (day_time, rate) = self
            .clock_updates
            .first()
            .map_or((0, 1.0), |&(_, total_ticks, _, rate)| (total_ticks, rate));

        if version.at_least(MinecraftVersion::V1_21_2) {
            // 1.21.2–26.2: the day time is sent as-is, followed by an is-increasing flag.
            day_time.write(writer)?;
            (rate > 0.0).write(writer)?;
        } else {
            // Before 1.21.2 a paused cycle is signalled by a negative day time (forced to
            // -1 at day 0), and there is no flag byte.
            let day_time = if rate == 0.0 {
                match day_time.cmp(&0) {
                    std::cmp::Ordering::Greater => -day_time,
                    std::cmp::Ordering::Equal => -1,
                    std::cmp::Ordering::Less => day_time,
                }
            } else {
                day_time.abs()
            };
            day_time.write(writer)?;
        }

        Ok(())
    }

    #[must_use]
    pub const fn new(game_time: i64, clock_updates: Vec<(i32, i64, f32, f32)>) -> Self {
        Self {
            game_time,
            clock_updates,
        }
    }
}

#[cfg(test)]
mod tests {
    use steel_utils::serial::WriteTo as _;

    use super::CSetTime;
    use crate::version::MinecraftVersion;

    fn clocked(day_time: i64, rate: f32) -> CSetTime {
        CSetTime::new(100, vec![(0, day_time, 0.5, rate)])
    }

    fn legacy_bytes(packet: &CSetTime, version: MinecraftVersion) -> Vec<u8> {
        let mut bytes = Vec::new();
        packet
            .write_payload_legacy(&mut bytes, version)
            .expect("legacy payload should encode");
        bytes
    }

    #[test]
    fn from_26_1_matches_native() {
        for version in [MinecraftVersion::V26_1, MinecraftVersion::V26_3] {
            let packet = clocked(500, 1.0);
            let mut native = Vec::new();
            packet.write(&mut native).expect("native encode");
            assert_eq!(legacy_bytes(&packet, version), native);
        }
    }

    #[test]
    fn between_1_21_2_and_26_1_sends_day_time_and_flag() {
        let packet = clocked(500, 1.0);
        let bytes = legacy_bytes(&packet, MinecraftVersion::V1_21_2);
        // game_time(8) + day_time(8) + flag(1)
        assert_eq!(bytes.len(), 17);
        assert_eq!(&bytes[0..8], &100i64.to_be_bytes(), "game time is kept");
        assert_eq!(&bytes[8..16], &500i64.to_be_bytes());
        assert_eq!(bytes[16], 1, "increasing cycle sets the flag");

        let paused = legacy_bytes(&clocked(500, 0.0), MinecraftVersion::V1_21_2);
        assert_eq!(
            &paused[8..16],
            &500i64.to_be_bytes(),
            "1.21.2 sends the day time as-is"
        );
        assert_eq!(paused[16], 0, "paused cycle clears the flag");
    }

    #[test]
    fn before_1_21_2_negates_a_paused_day_time() {
        // No flag byte; a paused cycle is signalled by a negative day time.
        let running = legacy_bytes(&clocked(500, 1.0), MinecraftVersion::V1_21);
        assert_eq!(running.len(), 16);
        assert_eq!(&running[0..8], &100i64.to_be_bytes());
        assert_eq!(&running[8..16], &500i64.to_be_bytes());

        let paused = legacy_bytes(&clocked(500, 0.0), MinecraftVersion::V1_21);
        assert_eq!(&paused[8..16], &(-500i64).to_be_bytes());

        // Day 0 paused cannot be encoded as +0, so it becomes -1.
        let paused_zero = legacy_bytes(&clocked(0, 0.0), MinecraftVersion::V1_21);
        assert_eq!(&paused_zero[8..16], &(-1i64).to_be_bytes());
    }

    #[test]
    fn empty_clocks_default_to_day_zero_running() {
        let packet = CSetTime::new(100, vec![]);
        let bytes = legacy_bytes(&packet, MinecraftVersion::V1_21_2);
        assert_eq!(&bytes[8..16], &0i64.to_be_bytes());
        assert_eq!(bytes[16], 1, "default rate is positive");
    }
}
