//! # Steel Protocol Packet Traits
//!
//! This module contains the traits for the packets.
use std::{
    io::{Cursor, Write},
    num::NonZeroU32,
    sync::Arc,
};

use flate2::{Compression, write::ZlibEncoder};
use rustc_hash::FxHashMap;
use serde::Deserialize;
use steel_utils::{
    FrontVec,
    codec::VarInt,
    serial::{ReadFrom, WriteTo},
};

use crate::utils::{ConnectionProtocol, MAX_PACKET_DATA_SIZE, MAX_PACKET_SIZE, PacketError};
use crate::version::MinecraftVersion;

// These are the network read/write traits
/// A trait for packets sent from the server to the client.
pub trait ServerPacket: ReadFrom {
    /// Reads a packet from the given data.
    fn read_packet(data: &mut Cursor<&[u8]>) -> Result<Self, PacketError> {
        Self::read(data).map_err(PacketError::from)
    }

    /// Reads a packet sent by a client on an older protocol version.
    ///
    /// The default is the native layout. A packet with a field whose encoding changed within
    /// the supported range overrides this and writes that field through the matching
    /// [`crate::legacy`] helper instead of the plain read.
    fn read_packet_for(
        data: &mut Cursor<&[u8]>,
        version: MinecraftVersion,
    ) -> Result<Self, PacketError> {
        let _ = version;
        Self::read_packet(data)
    }
}

/// A trait for packets sent from the client to the server.
pub trait ClientPacket: WriteTo {
    /// Writes the packet to the given writer.
    ///
    /// # Errors
    /// - If the packet fails to write.
    /// - If the protocol is invalid.
    fn write_packet(
        &self,
        writer: &mut impl Write,
        protocol: ConnectionProtocol,
    ) -> Result<(), PacketError> {
        let packet_id = self
            .get_id(protocol)
            .ok_or(PacketError::InvalidProtocol(format!(
                "Invalid protocol {protocol:?}"
            )))?;
        VarInt(packet_id).write(writer)?;
        self.write(writer).map_err(PacketError::from)
    }

    /// Writes the packet for a client on an older protocol version.
    ///
    /// The id is translated to that client's numbering, then [`ClientPacket::write_payload`]
    /// writes the body, which a packet with a version-dependent field overrides.
    ///
    /// # Errors
    /// - If the packet fails to write.
    /// - If the protocol is invalid.
    /// - [`PacketError::LegacyPacketUnsupported`] when the client's version has no such
    ///   packet and the packet must be dropped instead.
    fn write_packet_for(
        &self,
        writer: &mut impl Write,
        protocol: ConnectionProtocol,
        version: MinecraftVersion,
    ) -> Result<(), PacketError> {
        let current_id = self
            .get_id(protocol)
            .ok_or(PacketError::InvalidProtocol(format!(
                "Invalid protocol {protocol:?}"
            )))?;
        let client_id = crate::legacy::column_for(version)
            .and_then(|column| crate::legacy::remap_clientbound(column, protocol, current_id))
            .ok_or(PacketError::LegacyPacketUnsupported(current_id))?;
        VarInt(client_id).write(writer)?;
        self.write_payload(writer, version)
    }

    /// Writes the packet body for `version`.
    ///
    /// The default is the native layout, so a packet whose fields never changed needs no
    /// override and stays byte-identical to [`ClientPacket::write_packet`].
    ///
    /// # Errors
    /// - If the packet fails to write.
    fn write_payload(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> Result<(), PacketError> {
        let _ = version;
        self.write(writer).map_err(PacketError::from)
    }

    /// Gets the ID of the packet for the given protocol.
    fn get_id(&self, protocol: ConnectionProtocol) -> Option<i32>;
}

/// Information about compression.
#[derive(Copy, Clone, Debug, Deserialize)]
pub struct CompressionInfo {
    /// The compression threshold used when compression is enabled.
    /// Its an `NonZeroU32` to allow for nullptr optimization in `Option<Self>` cases
    pub threshold: NonZeroU32,
    /// A value between `0..9`.
    /// `1` = Optimize for the best speed of encoding.
    /// `9` = Optimize for the size of data being encoded.
    pub level: i32,
}

impl CompressionInfo {
    /// Vanilla default compression threshold in bytes.
    pub const DEFAULT_THRESHOLD: u32 = 256;
    /// Steel's default zlib compression level.
    pub const DEFAULT_LEVEL: i32 = 4;
}

impl Default for CompressionInfo {
    #[expect(
        clippy::unwrap_used,
        reason = "the default threshold is a known nonzero compression threshold"
    )]
    fn default() -> Self {
        Self {
            threshold: NonZeroU32::new(Self::DEFAULT_THRESHOLD).unwrap(),
            level: Self::DEFAULT_LEVEL,
        }
    }
}

/// Represents an encoded clientbound packet, optionally applying compression based on threshold and level.
///
/// # Packet Size Limits
/// - Maximum packet size: 2097151 bytes (2^21 - 1, max 3-byte `VarInt`)
/// - Maximum uncompressed size for compressed packets: 8388608 bytes (2^23)
/// - Length field must not exceed 3 bytes
///
/// # Packet Encoding Format
///
/// **Without Compression:**
/// ```text
/// [Length: VarInt]     Length of (Packet ID + Data)
/// [Packet ID: VarInt]  Protocol ID from packet report
/// [Data: Byte Array]   Packet payload
/// ```
///
/// **With Compression (size >= threshold):**
/// ```text
/// [Length: VarInt]     Length of (Data Length + compressed data)
/// [Data Length: VarInt] Length of uncompressed (Packet ID + Data)
/// [Compressed Data]    zlib compressed (Packet ID + Data)
/// ```
///
/// **With Compression (size < threshold):**
/// ```text
/// [Length: VarInt]     Length of (Data Length + uncompressed data)
/// [Data Length: VarInt] 0 to indicate uncompressed
/// [Packet ID: VarInt]  Protocol ID from packet report
/// [Data: Byte Array]   Uncompressed packet payload
/// ```
///
/// Compression is only applied when:
/// 1. Compression is enabled via Set Compression packet
/// 2. The uncompressed data length meets/exceeds the threshold
/// 3. The threshold is non-negative
#[derive(Clone)]
pub struct EncodedPacket {
    // This is optimized for reduces allocation
    /// The encoded data.
    pub encoded_data: Arc<FrontVec>,
}

impl EncodedPacket {
    fn from_data_uncompressed(mut packet_data: FrontVec) -> Result<Self, PacketError> {
        let data_len = packet_data.len();
        let varint_size = VarInt::written_size(data_len as i32);

        let complete_len = varint_size + data_len;
        if complete_len > MAX_PACKET_SIZE {
            return Err(PacketError::TooLong(complete_len));
        }

        VarInt(data_len as i32).set_in_front(&mut packet_data, varint_size);

        Ok(Self {
            encoded_data: Arc::new(packet_data),
        })
    }

    fn from_packet_data(
        mut packet_data: FrontVec,
        compression: CompressionInfo,
    ) -> Result<Self, PacketError> {
        let data_len = packet_data.len();
        // We dont need any more size check to convert to i32 as MAX_PACKET_DATA_SIZE < i32::MAX
        if data_len + VarInt::MAX_SIZE * 2 > MAX_PACKET_DATA_SIZE {
            return Err(PacketError::TooLong(data_len));
        }

        if data_len >= compression.threshold.get() as _ {
            let mut buf = FrontVec::new(10);
            let mut compressor =
                ZlibEncoder::new(&mut buf, Compression::new(compression.level as u32));

            compressor
                .write_all(&packet_data)
                .map_err(|e| PacketError::CompressionFailed(e.to_string()))?;
            compressor
                .finish()
                .map_err(|e| PacketError::CompressionFailed(e.to_string()))?;

            // compressed data cant be larger so we dont need to check the size again
            let varint_size = VarInt::written_size(data_len as i32);
            let full_len = varint_size + buf.len();
            let full_varint_size = VarInt::written_size(full_len as i32);

            VarInt(data_len as i32).set_in_front(&mut buf, varint_size);
            VarInt(full_len as i32).set_in_front(&mut buf, full_varint_size);
            log::trace!(
                "data length: {data_len}, full length: {full_len}, varint size: {varint_size}, full varint size: {full_varint_size}"
            );

            Ok(Self {
                encoded_data: Arc::new(buf),
            })
        } else {
            // Pushed before data:
            // Length of (Data Length) + length of compressed (Packet ID + Data)
            // 0 to indicate uncompressed

            let data_len_with_header = data_len + 1;
            let varint_size = VarInt::written_size(data_len_with_header as i32);

            VarInt(0).set_in_front(&mut packet_data, 1);
            VarInt(data_len_with_header as i32).set_in_front(&mut packet_data, varint_size);

            Ok(Self {
                encoded_data: Arc::new(packet_data),
            })
        }
    }

    /// Creates a new `EncodedPacket` from a bare packet.
    ///
    /// # Errors
    /// - If the packet fails to write.
    /// - If the packet fails to compress.
    pub fn from_bare<P: ClientPacket>(
        packet: P,
        compression: Option<CompressionInfo>,
        protocol: ConnectionProtocol,
    ) -> Result<Self, PacketError> {
        let buf = Self::write_vec(packet, protocol)?;
        Self::from_data(buf, compression)
    }

    /// Creates a new `EncodedPacket` whose id is translated for a legacy client.
    ///
    /// The id is remapped before framing and compression, which is the only point where
    /// that is still possible: in a compressed packet the id lives inside the compressed
    /// payload.
    ///
    /// # Errors
    /// - [`PacketError::LegacyPacketUnsupported`] when the client's version lacks the packet.
    /// - If the packet fails to write or compress.
    pub fn from_bare_for<P: ClientPacket>(
        packet: P,
        compression: Option<CompressionInfo>,
        protocol: ConnectionProtocol,
        version: MinecraftVersion,
    ) -> Result<Self, PacketError> {
        let buf = Self::write_vec_for(packet, protocol, version)?;
        Self::from_data(buf, compression)
    }

    /// Encodes a packet for one client, choosing the native or translated form by `version`.
    ///
    /// `version` is `None` for a native client. Returns `None` when the client's version has
    /// no id for this packet, meaning it must be skipped for that recipient rather than sent
    /// with bytes the client cannot interpret.
    pub fn from_bare_optional<P: ClientPacket>(
        packet: P,
        compression: Option<CompressionInfo>,
        protocol: ConnectionProtocol,
        version: Option<MinecraftVersion>,
    ) -> Option<Self> {
        match version {
            None => Self::from_bare(packet, compression, protocol).ok(),
            Some(version) => match Self::from_bare_for(packet, compression, protocol, version) {
                Err(PacketError::LegacyPacketUnsupported(_)) => None,
                result => result.ok(),
            },
        }
    }

    fn write_vec<P: ClientPacket>(
        packet: P,
        protocol: ConnectionProtocol,
    ) -> Result<FrontVec, PacketError> {
        let mut buf = FrontVec::new(6);
        packet.write_packet(&mut buf, protocol)?;
        Ok(buf)
    }

    fn write_vec_for<P: ClientPacket>(
        packet: P,
        protocol: ConnectionProtocol,
        version: MinecraftVersion,
    ) -> Result<FrontVec, PacketError> {
        let mut buf = FrontVec::new(6);
        packet.write_packet_for(&mut buf, protocol, version)?;
        Ok(buf)
    }

    fn from_data(buf: FrontVec, compression: Option<CompressionInfo>) -> Result<Self, PacketError> {
        if let Some(compression) = compression {
            Self::from_packet_data(buf, compression)
        } else {
            Self::from_data_uncompressed(buf)
        }
    }
}

/// Кеширует encoded-представление одного пакета по уникальной версии клиента в рассылке.
///
/// Игроки на нативной версии (`None`) и каждая legacy-колонка получают собственный encoded;
/// остальные игроки той же версии получают его клон. Так пакет кодируется по разу на версию,
/// а не один раз в нативном формате для всех (что сломало бы legacy-клиента).
pub struct VersionCache<T> {
    entries: FxHashMap<Option<MinecraftVersion>, Option<T>>,
}

impl<T: Clone> VersionCache<T> {
    /// Creates an empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: FxHashMap::default(),
        }
    }

    /// Возвращает encoded для `version`, закодировав его при первой встрече этой версии.
    ///
    /// `encode` вызывается не более одного раза на каждую уникальную версию. `None`
    /// кешируется как «этой версии пакет не положен» (например, у неё нет такого id).
    pub fn get_or_insert_with<F: FnOnce() -> Option<T>>(
        &mut self,
        version: Option<MinecraftVersion>,
        encode: F,
    ) -> Option<T> {
        self.entries.entry(version).or_insert_with(encode).clone()
    }
}

impl<T: Clone> Default for VersionCache<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::packets::common::CKeepAlive;

    use super::*;

    fn keep_alive() -> CKeepAlive {
        CKeepAlive::new(42)
    }

    #[test]
    fn native_version_encodes_byte_identically_to_unversioned() {
        for compression in [None, Some(CompressionInfo::default())] {
            let plain =
                EncodedPacket::from_bare(keep_alive(), compression, ConnectionProtocol::Play)
                    .expect("unversioned encode");
            let versioned = EncodedPacket::from_bare_for(
                keep_alive(),
                compression,
                ConnectionProtocol::Play,
                MinecraftVersion::CURRENT,
            )
            .expect("native encode");
            assert_eq!(
                plain.encoded_data.as_slice(),
                versioned.encoded_data.as_slice(),
                "the native version must not change a single byte (compression: {compression:?})"
            );
        }
    }

    #[test]
    fn remapped_id_is_written_before_framing() {
        // The id must be the first value in the pre-framing buffer, so compression and the
        // length prefix both cover the translated id.
        let mut buf = FrontVec::new(6);
        keep_alive()
            .write_packet_for(
                &mut buf,
                ConnectionProtocol::Play,
                MinecraftVersion::CURRENT,
            )
            .expect("write");
        let written = buf.as_slice();
        let mut cursor = Cursor::new(written);
        let id = <VarInt as ReadFrom>::read(&mut cursor)
            .expect("id varint")
            .0;
        assert_eq!(id, steel_registry::packets::play::C_KEEP_ALIVE);
        assert!(
            cursor.position() < written.len() as u64,
            "the payload follows the id: {written:?}"
        );
    }

    #[test]
    fn unextracted_version_is_reported_not_panicked() {
        // 1.21 has no extracted packet table yet, so no id can be translated for it. The
        // packet must be reported unsupported rather than sent with a native id.
        let mut buf = FrontVec::new(6);
        let err = keep_alive()
            .write_packet_for(&mut buf, ConnectionProtocol::Play, MinecraftVersion::V1_21)
            .expect_err("unextracted version must fail");
        assert!(
            matches!(err, PacketError::LegacyPacketUnsupported(_)),
            "got {err:?}"
        );
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct Marker(u32);

    /// The cache must encode once per distinct client version, not once per recipient and
    /// not a single native form reused for a legacy client.
    #[test]
    fn version_cache_encodes_once_per_version() {
        let mut cache: VersionCache<Marker> = VersionCache::new();
        let mut encodes = Vec::new();

        let native = [None, None, None];
        let legacy = [
            Some(MinecraftVersion::V1_21),
            Some(MinecraftVersion::V1_21),
            Some(MinecraftVersion::V26_1),
        ];

        for version in native.iter().chain(legacy.iter()) {
            let marker = cache.get_or_insert_with(*version, || {
                encodes.push(*version);
                Some(Marker(encodes.len() as u32))
            });
            assert!(marker.is_some());
        }

        assert_eq!(
            encodes,
            vec![
                None,
                Some(MinecraftVersion::V1_21),
                Some(MinecraftVersion::V26_1)
            ],
            "each distinct version must trigger exactly one encode"
        );

        // Repeated recipients of the same version reuse the identical encoded form.
        let first = cache.get_or_insert_with(Some(MinecraftVersion::V1_21), || unreachable!());
        let second = cache.get_or_insert_with(Some(MinecraftVersion::V1_21), || unreachable!());
        assert_eq!(first, Some(Marker(2)));
        assert_eq!(second, first);

        // Native and legacy versions are kept apart, never sharing one encoding.
        let native_marker = cache.get_or_insert_with(None, || unreachable!());
        let legacy_marker =
            cache.get_or_insert_with(Some(MinecraftVersion::V1_21), || unreachable!());
        assert_ne!(native_marker, legacy_marker);
    }

    /// A version that cannot receive the packet (absent id) must be cached as "no packet",
    /// so the closure is not retried on every recipient.
    #[test]
    fn version_cache_persists_a_failed_encode_as_absent() {
        let mut cache: VersionCache<()> = VersionCache::new();
        let mut attempts = 0;
        for _ in 0..3 {
            let result = cache.get_or_insert_with(Some(MinecraftVersion::V1_21), || {
                attempts += 1;
                None
            });
            assert_eq!(result, None);
        }
        assert_eq!(attempts, 1, "an absent packet must be encoded at most once");
    }
}
