use std::path::{Path, PathBuf};

use zeff_gb_core::hardware::GameBoySerialDevice;
use zeff_gb_core::hardware::ppu::DmgPalettePreset;
use zeff_gb_core::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
use zeff_gb_core::hardware::types::{CartridgeType, RamSize, RomSize};

use super::{GbBackend, GbEmulator};
use crate::emu_backend::paths::BackendPaths;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GbPersistentLoadOutcome {
    Absent,
    Loaded,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GbTasInitialInput {
    pub(crate) buttons: u8,
    pub(crate) dpad: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GbTasLoadProvenance {
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: usize,
    pub(crate) direct_gb_file: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) requested_hardware_mode: HardwareModePreference,
    pub(crate) resolved_hardware_mode: HardwareMode,
    pub(crate) external_boot_rom_used: bool,
    pub(crate) persistent_load: GbPersistentLoadOutcome,
    pub(crate) initial_input: GbTasInitialInput,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) initial_sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GbTasLoadProvenanceSeed {
    raw_source_media_sha256: [u8; 32],
    raw_source_media_len: usize,
    direct_gb_file: bool,
    any_mod_enabled: bool,
    any_mod_applied: bool,
    requested_hardware_mode: HardwareModePreference,
    initial_input: GbTasInitialInput,
    configured_sample_rate: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GbTasLoadSetup {
    pub(crate) loaded_from_source_path: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) requested_hardware_mode: HardwareModePreference,
}

impl Default for GbTasLoadSetup {
    fn default() -> Self {
        Self {
            loaded_from_source_path: false,
            any_mod_enabled: false,
            any_mod_applied: false,
            initial_input: None,
            configured_sample_rate: None,
            requested_hardware_mode: HardwareModePreference::Auto,
        }
    }
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub(crate) struct GbTasLoadProvenanceView<'a> {
    pub(crate) load: &'a GbTasLoadProvenance,
    pub(crate) current_sample_rate: u32,
    pub(crate) current_hardware_mode: HardwareMode,
    pub(crate) current_hardware_mode_preference: HardwareModePreference,
    pub(crate) current_serial_device: GameBoySerialDevice,
    pub(crate) cartridge_type: CartridgeType,
    pub(crate) rom_size: RomSize,
    pub(crate) ram_size: RamSize,
    pub(crate) is_cgb_exclusive: bool,
    pub(crate) has_external_boot_rom: bool,
    pub(crate) dmg_palette_preset: DmgPalettePreset,
}

impl GbTasLoadProvenanceSeed {
    pub(crate) fn new(
        raw_source_media_sha256: [u8; 32],
        raw_source_media_len: usize,
        source_path: &Path,
        rom_path: &Path,
        setup: GbTasLoadSetup,
    ) -> Self {
        let (buttons, dpad) = setup.initial_input.unwrap_or_default();
        Self {
            raw_source_media_sha256,
            raw_source_media_len,
            direct_gb_file: setup.loaded_from_source_path && direct_gb_file(source_path, rom_path),
            any_mod_enabled: setup.any_mod_enabled,
            any_mod_applied: setup.any_mod_applied,
            requested_hardware_mode: setup.requested_hardware_mode,
            initial_input: GbTasInitialInput {
                buttons: buttons & 0x0F,
                dpad: dpad & 0x0F,
            },
            configured_sample_rate: setup.configured_sample_rate,
        }
    }

    pub(crate) fn finish(
        self,
        persistent_load: GbPersistentLoadOutcome,
        resolved_hardware_mode: HardwareMode,
        initial_sample_rate: u32,
        external_boot_rom_used: bool,
    ) -> GbTasLoadProvenance {
        GbTasLoadProvenance {
            raw_source_media_sha256: self.raw_source_media_sha256,
            raw_source_media_len: self.raw_source_media_len,
            direct_gb_file: self.direct_gb_file,
            any_mod_enabled: self.any_mod_enabled,
            any_mod_applied: self.any_mod_applied,
            requested_hardware_mode: self.requested_hardware_mode,
            resolved_hardware_mode,
            external_boot_rom_used,
            persistent_load,
            initial_input: self.initial_input,
            configured_sample_rate: self.configured_sample_rate,
            initial_sample_rate,
        }
    }
}

impl GbBackend {
    pub(crate) fn with_load_provenance(
        emu: GbEmulator,
        rom_path: PathBuf,
        source_path: PathBuf,
        provenance: GbTasLoadProvenance,
    ) -> Self {
        let sram_recovery = crate::save_paths::battery_sram_session(
            &rom_path,
            crate::emu_backend::ActiveSystem::Gb.storage_subdir(),
            emu.rom_hash(),
        );
        Self {
            emu,
            paths: BackendPaths::with_source_path(rom_path, source_path),
            sram_recovery,
            tas_load_provenance: Some(provenance),
        }
    }

    pub(crate) fn tas_load_provenance(&self) -> Option<GbTasLoadProvenanceView<'_>> {
        Some(GbTasLoadProvenanceView {
            load: self.tas_load_provenance.as_ref()?,
            current_sample_rate: self.emu.sample_rate(),
            current_hardware_mode: self.emu.hardware_mode(),
            current_hardware_mode_preference: self.emu.hardware_mode_preference(),
            current_serial_device: self.emu.game_boy_serial_device(),
            cartridge_type: self.emu.header().cartridge_type,
            rom_size: self.emu.header().rom_size,
            ram_size: self.emu.header().ram_size,
            is_cgb_exclusive: self.emu.header().is_cgb_exclusive,
            has_external_boot_rom: self.emu.has_boot_rom(),
            dmg_palette_preset: self.emu.dmg_palette_preset(),
        })
    }
}

pub(crate) fn persistent_load_outcome(
    result: &anyhow::Result<Option<String>>,
) -> GbPersistentLoadOutcome {
    match result {
        Ok(Some(_)) => GbPersistentLoadOutcome::Loaded,
        Ok(None) => GbPersistentLoadOutcome::Absent,
        Err(_) => GbPersistentLoadOutcome::Unknown,
    }
}

fn direct_gb_file(source_path: &Path, rom_path: &Path) -> bool {
    source_path == rom_path
        && rom_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gb"))
}

#[cfg(test)]
mod tests;
