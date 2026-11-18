use std::path::Path;

use zeff_md_core::Emulator;

use super::MdBackend;
use crate::emu_backend::ActiveSystem;
use crate::save_paths::SRAM_COMPONENT;

impl MdBackend {
    pub(crate) fn host_persistence_enabled(&self) -> bool {
        self.host_persistence_enabled
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub(crate) fn set_host_persistence_enabled(&mut self, enabled: bool) {
        if self.rollback_owned() {
            return;
        }
        self.host_persistence_enabled = enabled;
    }

    pub(crate) fn battery_components(&self) -> Vec<(&'static str, Vec<u8>)> {
        if !self.machine.core().save_ram_kind().is_persistent() {
            return Vec::new();
        }
        self.machine
            .core()
            .dump_save_ram()
            .map(|bytes| vec![(SRAM_COMPONENT, bytes)])
            .unwrap_or_default()
    }

    pub(super) fn flush_save_ram(&mut self) -> anyhow::Result<Option<String>> {
        self.require_normal()?;
        if !self.host_persistence_enabled || !self.machine.core().save_ram_kind().is_persistent() {
            return Ok(None);
        }
        crate::save_paths::flush_battery_sram(
            &mut self.sram_recovery,
            self.paths.rom_path(),
            ActiveSystem::Md.storage_subdir(),
            self.rom_hash,
            self.machine.core().dump_save_ram(),
        )
    }
}

pub(crate) fn try_load_save_ram(
    emu: &mut Emulator,
    rom_path: &Path,
    media_identity: [u8; 32],
) -> anyhow::Result<Option<String>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = media_identity;
        crate::save_paths::try_load_battery_sram(
            rom_path,
            "Mega Drive",
            emu.save_ram_kind().is_persistent(),
            |bytes| emu.load_save_ram(bytes),
        )
    }
    #[cfg(target_arch = "wasm32")]
    crate::save_paths::try_load_browser_battery_sram(
        crate::save_paths::BrowserBatterySramRequest {
            rom_path,
            system_subdir: ActiveSystem::Md.storage_subdir(),
            media_identity,
            component: SRAM_COMPONENT,
            system_label: "Mega Drive",
            has_battery: emu.save_ram_kind().is_persistent(),
        },
        |bytes| emu.load_save_ram(bytes),
    )
}
