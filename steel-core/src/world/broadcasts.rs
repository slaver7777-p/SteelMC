use super::{
    CPlayerChat, CSystemChat, ChunkPos, ClientPacket, ConnectionProtocol, EncodedPacket, Entity,
    EntityMovementSyncPacket, NetworkConnection, Player, PlayerChunkView, World,
};
use crate::player::chat::OutgoingChatMessage;
use steel_protocol::packet_traits::VersionCache;
use steel_protocol::packets::game::ChatTypeBound;
use steel_protocol::version::MinecraftVersion;

impl World {
    /// Broadcasts a chat message (signed or not) to all players
    pub fn broadcast_chat(&self, outgoing: &OutgoingChatMessage, chat_type: &ChatTypeBound) {
        self.players.iter_players(|_, recipient| {
            outgoing.send_to_player(recipient, chat_type);
            true
        });
    }

    /// Broadcasts a system chat message to all players.
    pub fn broadcast_system_chat(&self, packet: CSystemChat) {
        self.broadcast_to_all(packet);
    }

    /// Broadcasts a packet to all players in the world.
    pub fn broadcast_to_all<P: ClientPacket + Clone>(&self, packet: P) {
        let mut cache = VersionCache::new();
        self.players.iter_players(|_, player| {
            self.send_play_to(player, packet.clone(), &mut cache);
            true
        });
    }

    /// Broadcasts a packet to all players in the world except one (identified by entity ID).
    pub fn broadcast_to_all_except<P: ClientPacket + Clone>(&self, packet: P, exclude: i32) {
        let mut cache = VersionCache::new();
        self.players.iter_players(|_, player| {
            if player.id() == exclude {
                return true;
            }
            self.send_play_to(player, packet.clone(), &mut cache);
            true
        });
    }

    /// Broadcasts a packet to all players in the world.
    ///
    /// This method handles encoding the packets produced from the function passed.
    pub fn broadcast_to_all_with<P: ClientPacket, F: Fn(&Player) -> P>(&self, packet: F) {
        self.players.iter_players(|_, player| {
            player.send_packet(packet(player));
            true
        });
    }

    /// Broadcasts an unsigned player chat message to all players.
    pub fn broadcast_unsigned_chat(&self, packet: CPlayerChat) {
        self.players.iter_players(|_, recipient| {
            let messages_received = recipient.get_and_increment_messages_received();
            recipient.send_packet(packet.for_recipient(
                messages_received,
                &packet.previous_messages,
                &packet.chat_type,
            ));

            true
        });
    }

    /// Broadcasts a packet to all players tracking the given chunk.
    ///
    /// This method handles encoding the packet internally, avoiding boilerplate at call sites.
    /// A legacy tracker receives the packet translated to its version; if encoding fails for a
    /// version, that recipient is skipped while others still receive it.
    pub fn broadcast_to_nearby<P: ClientPacket + Clone>(
        &self,
        chunk: ChunkPos,
        packet: P,
        exclude: Option<i32>,
    ) {
        let tracking_players = self.get_packet_tracking_players(chunk);
        let mut cache = VersionCache::new();
        for entity_id in tracking_players {
            if Some(entity_id) == exclude {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(entity_id) {
                self.send_play_to(&player, packet.clone(), &mut cache);
            }
        }
    }

    /// Returns players whose view includes the chunk and whose client has the base chunk packet.
    pub fn get_packet_tracking_players(&self, chunk: ChunkPos) -> Vec<i32> {
        self.player_area_map
            .get_tracking_players(chunk)
            .into_iter()
            .filter(|entity_id| {
                self.players
                    .get_by_entity_id(*entity_id)
                    .is_some_and(|player| player.chunk_sender().lock().is_chunk_sent(chunk))
            })
            .collect()
    }

    /// Returns players on the tracked border of a chunk whose client has its base chunk packet.
    pub fn get_light_packet_tracking_players(&self, chunk: ChunkPos) -> Vec<i32> {
        self.player_area_map
            .get_tracking_players(chunk)
            .into_iter()
            .filter(|entity_id| {
                let Some(player) = self.players.get_by_entity_id(*entity_id) else {
                    return false;
                };
                let Some(view) = *player.last_tracking_view.lock() else {
                    return false;
                };
                let chunk_sender = player.chunk_sender().lock();
                let is_chunk_sent = |pos| chunk_sender.is_chunk_sent(pos);
                Self::chunk_is_on_packet_tracked_border(view, chunk, &is_chunk_sent)
            })
            .collect()
    }

    pub(super) fn chunk_is_on_packet_tracked_border(
        view: PlayerChunkView,
        chunk: ChunkPos,
        is_chunk_sent: &impl Fn(ChunkPos) -> bool,
    ) -> bool {
        if !Self::chunk_is_packet_tracked(view, chunk, is_chunk_sent) {
            return false;
        }

        for dx in -1..=1 {
            for dz in -1..=1 {
                if dx == 0 && dz == 0 {
                    continue;
                }

                let neighbor = ChunkPos::new(chunk.0.x + dx, chunk.0.y + dz);
                if !Self::chunk_is_packet_tracked(view, neighbor, is_chunk_sent) {
                    return true;
                }
            }
        }

        false
    }

    pub(super) fn chunk_is_packet_tracked(
        view: PlayerChunkView,
        chunk: ChunkPos,
        is_chunk_sent: &impl Fn(ChunkPos) -> bool,
    ) -> bool {
        view.contains(chunk) && is_chunk_sent(chunk)
    }

    /// Broadcasts a packet to players currently tracking an entity.
    pub fn broadcast_to_entity_trackers<P: ClientPacket + Clone>(
        &self,
        entity_id: i32,
        packet: P,
        exclude: Option<i32>,
    ) {
        let mut cache = VersionCache::new();
        for player_id in self.entity_tracker.tracking_player_ids(entity_id) {
            if Some(player_id) == exclude {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(player_id) {
                self.send_play_to(&player, packet.clone(), &mut cache);
            }
        }
    }

    /// Broadcasts a packet to players tracking an entity, excluding several players.
    pub fn broadcast_to_entity_trackers_except_many<P: ClientPacket + Clone>(
        &self,
        entity_id: i32,
        packet: P,
        excluded_player_ids: &[i32],
    ) {
        let mut cache = VersionCache::new();
        for player_id in self.entity_tracker.tracking_player_ids(entity_id) {
            if excluded_player_ids.contains(&player_id) {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(player_id) {
                self.send_play_to(&player, packet.clone(), &mut cache);
            }
        }
    }

    /// Broadcasts an entity movement sync packet to players currently tracking an entity.
    pub fn broadcast_movement_sync_to_entity_trackers(
        &self,
        entity_id: i32,
        packet: EntityMovementSyncPacket,
        exclude: Option<i32>,
    ) {
        let mut cache = VersionCache::new();
        for player_id in self.entity_tracker.tracking_player_ids(entity_id) {
            if Some(player_id) == exclude {
                continue;
            }
            if let Some(player) = self.players.get_by_entity_id(player_id) {
                let version = player.connection.legacy_version();
                let encoded = cache.get_or_insert_with(version, || {
                    self.encode_movement_sync_for(packet.clone(), version)
                });
                if let Some(encoded) = encoded {
                    encoded.send_to(&player);
                }
            }
        }
    }

    /// Encodes `packet` for the client's version and sends it, caching per version.
    ///
    /// A recipient whose version has no id for this packet is skipped; others still receive it.
    pub(crate) fn send_play_to<P: ClientPacket>(
        &self,
        player: &Player,
        packet: P,
        cache: &mut VersionCache<EncodedPacket>,
    ) {
        let version = player.connection.legacy_version();
        let encoded = cache.get_or_insert_with(version, || self.encode_play_for(packet, version));
        if let Some(encoded) = encoded {
            player.connection.send_encoded(encoded);
        }
    }

    /// Encodes a play packet for `version`, or `None` when that version cannot receive it.
    fn encode_play_for<P: ClientPacket>(
        &self,
        packet: P,
        version: Option<MinecraftVersion>,
    ) -> Option<EncodedPacket> {
        EncodedPacket::from_bare_optional(
            packet,
            self.compression,
            ConnectionProtocol::Play,
            version,
        )
    }

    /// Encodes a movement sync packet for one client version.
    pub(super) fn encode_movement_sync_for(
        &self,
        packet: EntityMovementSyncPacket,
        version: Option<MinecraftVersion>,
    ) -> Option<EncodedMovementSync> {
        let encoded = match packet {
            EntityMovementSyncPacket::Position(packet) => self.encode_play_for(packet, version),
            EntityMovementSyncPacket::PositionRotation(packet) => {
                self.encode_play_for(packet, version)
            }
            EntityMovementSyncPacket::Rotation(packet) => self.encode_play_for(packet, version),
            EntityMovementSyncPacket::HeadRotation(packet) => self.encode_play_for(packet, version),
            EntityMovementSyncPacket::PositionSync(packet) => self.encode_play_for(packet, version),
            EntityMovementSyncPacket::Velocity(packet) => self.encode_play_for(packet, version),
            EntityMovementSyncPacket::ProjectileVelocity(motion, power) => {
                return Some(EncodedMovementSync::Bundle(vec![
                    self.encode_play_for(motion, version)?,
                    self.encode_play_for(power, version)?,
                ]));
            }
        };
        encoded.map(EncodedMovementSync::Single)
    }
}

/// An encoded movement sync, sent alone or as a client bundle.
#[derive(Clone)]
pub(super) enum EncodedMovementSync {
    Single(EncodedPacket),
    Bundle(Vec<EncodedPacket>),
}

impl EncodedMovementSync {
    pub(super) fn send_to(&self, player: &Player) {
        match self {
            Self::Single(packet) => player.connection.send_encoded(packet.clone()),
            Self::Bundle(packets) => player.connection.send_encoded_bundle(packets.clone()),
        }
    }
}
