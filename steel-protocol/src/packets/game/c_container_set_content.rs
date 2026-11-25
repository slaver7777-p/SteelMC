use steel_macros::{ClientPacket, WriteTo};
use steel_registry::{item_stack::ItemStack, packets::play::C_CONTAINER_SET_CONTENT};
use steel_utils::{codec::VarInt, serial::WriteTo as _};

use crate::legacy::write_container_id_legacy;
use crate::version::MinecraftVersion;

#[derive(ClientPacket, WriteTo, Clone, Debug)]
#[packet_id(Play = C_CONTAINER_SET_CONTENT)]
#[legacy_payload]
pub struct CContainerSetContent {
    #[write(as = VarInt)]
    pub container_id: i32,
    #[write(as = VarInt)]
    pub state_id: i32,
    pub items: Vec<ItemStack>,
    pub carried_item: ItemStack,
}

impl CContainerSetContent {
    /// Writes the body, sending `container_id` as a byte before 1.21.2.
    ///
    /// The remaining fields keep their native layout.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl std::io::Write,
        version: MinecraftVersion,
    ) -> std::io::Result<()> {
        write_container_id_legacy(self.container_id, version, writer)?;
        VarInt(self.state_id).write(writer)?;
        self.items.write(writer)?;
        self.carried_item.write(writer)
    }
}
