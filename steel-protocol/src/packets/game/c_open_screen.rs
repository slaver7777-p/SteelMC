use std::io::{Result, Write};

use steel_macros::ClientPacket;
use steel_registry::{RegistryEntry, menu_type::MenuTypeRef, packets::play::C_OPEN_SCREEN};
use steel_utils::{codec::VarInt, serial::WriteTo};
use text_components::{TextComponent, resolving::TextResolutor};

use crate::legacy::write_container_id_legacy;
use crate::version::MinecraftVersion;

#[derive(ClientPacket, Clone, Debug)]
#[packet_id(Play = C_OPEN_SCREEN)]
#[legacy_payload]
pub struct COpenScreen {
    pub container_id: i32,
    pub menu_type: MenuTypeRef,
    pub title: TextComponent,
}

impl COpenScreen {
    pub fn new<T: TextResolutor>(
        container_id: i32,
        menu_type: MenuTypeRef,
        title: &TextComponent,
        player: &T,
    ) -> Self {
        Self {
            container_id,
            menu_type,
            title: title.resolve(player),
        }
    }

    /// Writes the body, sending `container_id` as a byte before 1.21.2.
    ///
    /// # Errors
    /// - If the packet fails to write.
    pub fn write_payload_legacy(
        &self,
        writer: &mut impl Write,
        version: MinecraftVersion,
    ) -> Result<()> {
        write_container_id_legacy(self.container_id, version, writer)?;
        VarInt(self.menu_type.id() as i32).write(writer)?;
        self.title.write(writer)
    }
}

impl WriteTo for COpenScreen {
    fn write(&self, writer: &mut impl Write) -> Result<()> {
        VarInt(self.container_id).write(writer)?;
        VarInt(self.menu_type.id() as i32).write(writer)?;
        self.title.write(writer)?;
        Ok(())
    }
}
