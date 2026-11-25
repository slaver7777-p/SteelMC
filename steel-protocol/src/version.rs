//! # Protocol version model
//!
//! Backward-compatible client versions are identified by their network protocol number
//! (the value carried in the handshake `SClientIntention`). This enum is the single
//! source of truth for the versions Steel speaks to and how they order chronologically,
//! which every lower-level translation layer keys off of.
//!
//! Only *protocol-distinct* releases are listed. A release whose handshake number and wire
//! format match a listed variant (1.21.1 with 1.21, 1.21.3 with 1.21.2, 1.21.8 with 1.21.7,
//! 1.21.10 with 1.21.9) collapses onto that variant, because the handshake reports the same
//! protocol number and the packets are byte-identical. The variant list matches the columns
//! of the packet-id matrix in [`crate::packets`], one column per supported version.

use std::fmt::{self, Display, Formatter};

/// A supported Minecraft Java protocol version, ordered oldest to newest.
///
/// The declaration order is the chronological order and backs [`PartialOrd`]/[`Ord`]:
/// comparing two versions with `<`/`>=` is a release-order comparison, which the payload
/// adapters use to pick the encoding era for a client.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum MinecraftVersion {
    /// Minecraft 1.21 / 1.21.1 (protocol 767).
    V1_21 = 0,
    /// Minecraft 1.21.2 / 1.21.3 (protocol 768).
    V1_21_2,
    /// Minecraft 1.21.4 (protocol 769).
    V1_21_4,
    /// Minecraft 1.21.5 (protocol 770).
    V1_21_5,
    /// Minecraft 1.21.6 (protocol 771).
    V1_21_6,
    /// Minecraft 1.21.7 / 1.21.8 (protocol 772).
    V1_21_7,
    /// Minecraft 1.21.9 / 1.21.10 (protocol 773).
    V1_21_9,
    /// Minecraft 1.21.11 (protocol 774).
    V1_21_11,
    /// Minecraft 26.1 (protocol 775).
    V26_1,
    /// Minecraft 26.2 (protocol 776).
    V26_2,
    /// Minecraft 26.3 (protocol 777), the server's native version.
    V26_3,
}

impl MinecraftVersion {
    /// The server's native protocol version; packets on this version bypass translation.
    pub const CURRENT: Self = Self::V26_3;

    /// Every supported version, oldest first. The last entry is [`Self::CURRENT`].
    ///
    /// The order and count match the columns of the generated packet-id matrix, so a
    /// version's index is its matrix column.
    pub const ALL: &[Self] = &[
        Self::V1_21,
        Self::V1_21_2,
        Self::V1_21_4,
        Self::V1_21_5,
        Self::V1_21_6,
        Self::V1_21_7,
        Self::V1_21_9,
        Self::V1_21_11,
        Self::V26_1,
        Self::V26_2,
        Self::V26_3,
    ];

    /// Number of supported versions, and the width of each packet-id matrix row.
    pub const COUNT: usize = Self::ALL.len();

    /// The oldest client version Steel accepts.
    pub const OLDEST_SUPPORTED: Self = Self::V1_21;

    /// The matrix column (oldest-first index) this version occupies.
    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    /// The network protocol number this version reports in the handshake.
    #[must_use]
    pub const fn protocol(self) -> i32 {
        match self {
            Self::V1_21 => 767,
            Self::V1_21_2 => 768,
            Self::V1_21_4 => 769,
            Self::V1_21_5 => 770,
            Self::V1_21_6 => 771,
            Self::V1_21_7 => 772,
            Self::V1_21_9 => 773,
            Self::V1_21_11 => 774,
            Self::V26_1 => 775,
            Self::V26_2 => 776,
            Self::V26_3 => 777,
        }
    }

    /// Resolves a version from a handshake protocol number.
    ///
    /// Returns [`None`] for numbers outside the supported range. Releases that share a
    /// protocol number with a listed variant resolve to that variant: 1.21.1 to
    /// [`Self::V1_21`], 1.21.3 to [`Self::V1_21_2`], 1.21.8 to [`Self::V1_21_7`], 1.21.10 to
    /// [`Self::V1_21_9`], because each pair is wire-identical.
    #[must_use]
    pub const fn from_protocol(protocol: i32) -> Option<Self> {
        match protocol {
            767 => Some(Self::V1_21),
            768 => Some(Self::V1_21_2),
            769 => Some(Self::V1_21_4),
            770 => Some(Self::V1_21_5),
            771 => Some(Self::V1_21_6),
            772 => Some(Self::V1_21_7),
            773 => Some(Self::V1_21_9),
            774 => Some(Self::V1_21_11),
            775 => Some(Self::V26_1),
            776 => Some(Self::V26_2),
            777 => Some(Self::V26_3),
            _ => None,
        }
    }

    /// Whether this version is the server's native one, i.e. needs no translation.
    #[must_use]
    pub const fn is_current(self) -> bool {
        matches!(self, Self::CURRENT)
    }

    /// Whether Steel can fully translate pre-play and play traffic for this version.
    ///
    /// A version is admitted at the handshake only when both its packet-id matrix has been
    /// extracted (a `packets-<protocol>.json` build asset) and the payload adapters in
    /// [`crate::legacy`] cover its structural differences. Refusing an unsupported version
    /// keeps the server from serving a legacy client mis-encoded packets.
    #[must_use]
    pub const fn is_translatable(self) -> bool {
        if self.is_current() {
            return true;
        }
        let matrix_ready = steel_registry::packets::legacy::column_of(self.protocol()).is_some();
        matrix_ready && crate::legacy::payload_adapters_ready(self)
    }

    /// Whether `self` is at least `other` in release order.
    #[must_use]
    pub const fn at_least(self, other: Self) -> bool {
        (self as u8) >= (other as u8)
    }

    /// Whether `self` is older than `other` in release order.
    #[must_use]
    pub const fn before(self, other: Self) -> bool {
        (self as u8) < (other as u8)
    }
}

impl Display for MinecraftVersion {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        let label = match self {
            Self::V1_21 => "1.21",
            Self::V1_21_2 => "1.21.2",
            Self::V1_21_4 => "1.21.4",
            Self::V1_21_5 => "1.21.5",
            Self::V1_21_6 => "1.21.6",
            Self::V1_21_7 => "1.21.7",
            Self::V1_21_9 => "1.21.9",
            Self::V1_21_11 => "1.21.11",
            Self::V26_1 => "26.1",
            Self::V26_2 => "26.2",
            Self::V26_3 => "26.3",
        };
        formatter.write_str(label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_round_trips_through_from_protocol() {
        for &version in MinecraftVersion::ALL {
            assert_eq!(
                MinecraftVersion::from_protocol(version.protocol()),
                Some(version),
                "{version} should resolve from its own protocol number"
            );
        }
    }

    #[test]
    fn ordering_is_release_order() {
        assert!(MinecraftVersion::V1_21 < MinecraftVersion::V1_21_2);
        assert!(MinecraftVersion::V1_21_6 < MinecraftVersion::V1_21_7);
        assert!(MinecraftVersion::V1_21_11 < MinecraftVersion::V26_1);
        assert!(MinecraftVersion::V26_2 < MinecraftVersion::CURRENT);
        assert!(MinecraftVersion::CURRENT.at_least(MinecraftVersion::V1_21));
        assert!(MinecraftVersion::V1_21_2.before(MinecraftVersion::V1_21_4));
    }

    #[test]
    fn all_oldest_first_and_last_is_current() {
        assert_eq!(
            MinecraftVersion::ALL.first(),
            Some(&MinecraftVersion::OLDEST_SUPPORTED)
        );
        assert_eq!(
            MinecraftVersion::ALL.last(),
            Some(&MinecraftVersion::CURRENT)
        );
        assert_eq!(MinecraftVersion::ALL.len(), MinecraftVersion::COUNT);
        for pair in MinecraftVersion::ALL.windows(2) {
            assert!(pair[0] < pair[1], "ALL must be strictly ascending");
        }
    }

    #[test]
    fn index_is_oldest_first_position() {
        assert_eq!(MinecraftVersion::V1_21.index(), 0);
        assert_eq!(
            MinecraftVersion::CURRENT.index(),
            MinecraftVersion::COUNT - 1
        );
        for (position, &version) in MinecraftVersion::ALL.iter().enumerate() {
            assert_eq!(version.index(), position);
        }
    }

    #[test]
    fn current_version_is_native() {
        assert!(MinecraftVersion::CURRENT.is_current());
        assert!(!MinecraftVersion::V1_21.is_current());
    }

    #[test]
    fn unsupported_protocols_are_rejected() {
        assert_eq!(MinecraftVersion::from_protocol(766), None);
        assert_eq!(MinecraftVersion::from_protocol(778), None);
        assert_eq!(MinecraftVersion::from_protocol(47), None);
    }

    #[test]
    fn native_version_is_always_translatable() {
        assert!(MinecraftVersion::CURRENT.is_translatable());
    }

    #[test]
    fn legacy_version_requires_extracted_data_and_adapters() {
        for &version in MinecraftVersion::ALL {
            if version.is_current() {
                continue;
            }
            // The matrix column follows the extracted `packets-<protocol>.json` assets, and
            // the payload adapters are not enabled yet, so no legacy version is admitted.
            // Enabling a version must flip this by landing its adapters, not by editing it.
            assert!(
                !version.is_translatable(),
                "{version} must not be advertised before its adapters are implemented"
            );
        }
    }
}
