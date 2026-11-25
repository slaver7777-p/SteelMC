use std::io::Cursor;

use steel_macros::{ReadFrom, ServerPacket};

use crate::legacy::read_container_id_legacy;
use crate::version::MinecraftVersion;

#[derive(ServerPacket, ReadFrom, Clone, Debug)]
#[legacy_payload]
pub struct SContainerClose {
    #[read(as = VarInt)]
    pub container_id: i32,
}

impl SContainerClose {
    /// Reads the body, accepting `container_id` as a byte from clients before 1.21.2.
    ///
    /// # Errors
    /// - If the packet is malformed.
    pub fn read_payload_legacy(
        data: &mut Cursor<&[u8]>,
        version: MinecraftVersion,
    ) -> std::io::Result<Self> {
        Ok(Self {
            container_id: read_container_id_legacy(data, version)?,
        })
    }
}
