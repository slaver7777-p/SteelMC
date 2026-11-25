use std::io::Cursor;

use steel_macros::{ReadFrom, ServerPacket};
use steel_utils::codec::VarInt;
use steel_utils::serial::ReadFrom as _;

use crate::legacy::read_container_id_legacy;
use crate::version::MinecraftVersion;

#[derive(ServerPacket, ReadFrom, Clone, Debug)]
#[legacy_payload]
pub struct SContainerSlotStateChanged {
    #[read(as = VarInt)]
    pub slot_id: i32,
    #[read(as = VarInt)]
    pub container_id: i32,
    pub new_state: bool,
}

impl SContainerSlotStateChanged {
    /// Reads the body, accepting `container_id` as a byte from clients before 1.21.2.
    ///
    /// # Errors
    /// - If the packet is malformed.
    pub fn read_payload_legacy(
        data: &mut Cursor<&[u8]>,
        version: MinecraftVersion,
    ) -> std::io::Result<Self> {
        Ok(Self {
            slot_id: VarInt::read(data)?.0,
            container_id: read_container_id_legacy(data, version)?,
            new_state: bool::read(data)?,
        })
    }
}
