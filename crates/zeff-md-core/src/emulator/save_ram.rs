use super::Emulator;
use anyhow::{Result, bail};
use zeff_emu_common::save_ram::SaveRamKind;

impl Emulator {
    pub fn save_ram_kind(&self) -> SaveRamKind {
        if let Some(save) = &self.bus.cartridge.save {
            SaveRamKind::known_nonvolatile(save.bytes.len())
        } else {
            SaveRamKind::none()
        }
    }

    pub fn dump_save_ram(&self) -> Option<Vec<u8>> {
        self.bus
            .cartridge
            .save
            .as_ref()
            .map(|save| save.bytes.to_vec())
    }

    pub fn load_save_ram(&mut self, bytes: &[u8]) -> Result<()> {
        let Some(save) = &mut self.bus.cartridge.save else {
            bail!("MD cartridge has no supported save RAM");
        };
        save.load(bytes)?;
        self.invalidate_audio_trace(
            zeff_emu_common::audio_trace::AudioTraceInvalidation::ExternalMutation,
        );
        Ok(())
    }
}
