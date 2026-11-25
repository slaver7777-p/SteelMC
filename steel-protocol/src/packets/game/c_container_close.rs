use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_CONTAINER_CLOSE;

use crate::legacy::write_container_id_legacy;
use crate::version::MinecraftVersion;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_CONTAINER_CLOSE)]
#[legacy_payload]
pub struct CContainerClose {
    #[write(as = VarInt)]
    pub container_id: i32,
}

impl CContainerClose {
    /// Writes the body, sending `container_id` as a byte before 1.21.2.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl std::io::Write,
        version: MinecraftVersion,
    ) -> std::io::Result<()> {
        write_container_id_legacy(self.container_id, version, writer)
    }
}
