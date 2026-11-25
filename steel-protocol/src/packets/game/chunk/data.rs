use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_LEVEL_CHUNK_WITH_LIGHT;
use steel_utils::serial::{OptionalNbt, WriteTo as _};
use steel_utils::{PackedChunkLocalXZ, codec::BitSet};

use crate::legacy::write_bit_set_legacy;
use crate::version::MinecraftVersion;

#[derive(WriteTo, Copy, Clone, Debug)]
#[write(as = VarInt)]
pub enum HeightmapType {
    WorldSurface = 1,
    MotionBlocking = 4,
    MotionBlockingNoLeaves = 5,
}

#[derive(Debug, Clone, WriteTo)]
pub struct Heightmaps {
    pub heightmaps: Vec<(HeightmapType, Vec<i64>)>,
}

#[derive(Debug, Clone, WriteTo)]
pub struct BlockEntityInfo {
    pub packed_xz: PackedChunkLocalXZ,
    pub y: i16,
    #[write(as = VarInt)]
    pub type_id: i32,
    pub data: OptionalNbt,
}

#[derive(Debug, Clone, WriteTo)]
pub struct ChunkPacketData {
    pub heightmaps: Heightmaps,
    #[write(as = Prefixed(VarInt), bound = 2097152)]
    pub data: Vec<u8>,
    pub block_entities: Vec<BlockEntityInfo>,
}

#[derive(Debug, Clone, WriteTo)]
pub struct LightUpdatePacketData {
    pub sky_y_mask: BitSet,
    pub block_y_mask: BitSet,
    pub empty_sky_y_mask: BitSet,
    pub empty_block_y_mask: BitSet,
    pub sky_updates: Vec<Vec<u8>>,
    pub block_updates: Vec<Vec<u8>>,
}

impl LightUpdatePacketData {
    /// Writes the light body, sending the four Y masks in the pre-26.3 word form.
    ///
    /// Only the masks changed within the supported range; the light arrays keep their native
    /// layout.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_legacy(
        &self,
        writer: &mut impl std::io::Write,
        version: MinecraftVersion,
    ) -> std::io::Result<()> {
        for mask in [
            &self.sky_y_mask,
            &self.block_y_mask,
            &self.empty_sky_y_mask,
            &self.empty_block_y_mask,
        ] {
            write_bit_set_legacy(mask, version, writer)?;
        }
        self.sky_updates.write(writer)?;
        self.block_updates.write(writer)
    }
}

#[derive(ClientPacket, Debug, Clone, WriteTo)]
#[packet_id(Play = C_LEVEL_CHUNK_WITH_LIGHT)]
#[legacy_payload]
pub struct CLevelChunkWithLight {
    pub x: i32,
    pub z: i32,
    pub chunk_data: ChunkPacketData,
    pub light_data: LightUpdatePacketData,
}

impl CLevelChunkWithLight {
    /// Writes the body, keeping the chunk data native and translating only the light masks.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl std::io::Write,
        version: MinecraftVersion,
    ) -> std::io::Result<()> {
        self.x.write(writer)?;
        self.z.write(writer)?;
        self.chunk_data.write(writer)?;
        self.light_data.write_legacy(writer, version)
    }
}
