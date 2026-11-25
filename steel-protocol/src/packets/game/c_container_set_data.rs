use steel_macros::{ClientPacket, WriteTo};
use steel_registry::packets::play::C_CONTAINER_SET_DATA;
use steel_utils::serial::WriteTo as _;

use crate::legacy::write_container_id_legacy;
use crate::version::MinecraftVersion;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_CONTAINER_SET_DATA)]
#[legacy_payload]
pub struct CContainerSetData {
    #[write(as = VarInt)]
    pub container_id: i32,
    pub id: i16,
    pub value: i16,
}

impl CContainerSetData {
    /// Writes the body, sending `container_id` as a byte before 1.21.2.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl std::io::Write,
        version: MinecraftVersion,
    ) -> std::io::Result<()> {
        write_container_id_legacy(self.container_id, version, writer)?;
        self.id.write(writer)?;
        self.value.write(writer)
    }
}
