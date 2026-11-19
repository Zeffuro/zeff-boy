use super::*;

impl Emulator {
    pub fn clear_rom_patches(&mut self) {
        self.install_rom_patches(&[])
            .expect("empty ROM cheat set is always valid");
    }

    pub fn supports_rom_breakpoints(&self) -> bool {
        self.supports_rom_cheats()
            && self.bus.cartridge.download.is_none()
            && self.bus.cartridge.svp.is_none()
    }

    pub fn supports_rom_cheats(&self) -> bool {
        (self.bus.cartridge.svp.is_none() || self.bus.owned.is_none())
            && self.bus.tmss.as_ref().is_none_or(|_| {
                self.bus.owned.is_none() && self.bus.cartridge.tmss_rom_overlay_compatible()
            })
            && self.bus.cartridge.supports_rom_overlay()
    }

    /// Patch offsets name original image bytes; Download uses them as read masks.
    pub fn install_rom_patches(&mut self, patches: &[CheatPatch]) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.supports_rom_cheats() || !patches.iter().any(|patch| patch.is_rom_patch()),
            "ROM cheats require a qualified cartridge read mask and boot owner"
        );
        let overlay = self.bus.cartridge.prepare_rom_overlay(patches)?;
        if self.bus.cartridge.rom_overlay != overlay {
            self.bus.cartridge.rom_overlay = overlay;
            self.invalidate_audio_trace(AudioTraceInvalidation::ExternalMutation);
        }
        Ok(())
    }
}
