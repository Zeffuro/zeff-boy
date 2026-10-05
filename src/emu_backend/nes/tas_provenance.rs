use std::path::{Path, PathBuf};

use super::{NesBackend, NesEmulator};
use crate::emu_backend::paths::BackendPaths;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NesPersistentLoadOutcome {
    Absent,
    Loaded,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NesTasInitialInput {
    pub(crate) buttons: u8,
    pub(crate) dpad: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NesTasLoadProvenance {
    pub(crate) netplay_media: Option<crate::emu_backend::loader::NetplayRomMedia>,
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: u64,
    pub(crate) initial_persistent_sha256: [u8; 32],
    pub(crate) initial_state_sha256: Option<[u8; 32]>,
    pub(crate) direct_nes_file: bool,
    #[cfg(target_arch = "wasm32")]
    pub(crate) direct_browser_nes: bool,
    pub(crate) sync_config_sha256: [u8; 32],
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) persistent_load: NesPersistentLoadOutcome,
    pub(crate) initial_input: NesTasInitialInput,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) initial_sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NesTasLoadProvenanceSeed {
    raw_source_media_sha256: [u8; 32],
    raw_source_media_len: u64,
    direct_nes_file: bool,
    #[cfg(target_arch = "wasm32")]
    direct_browser_nes: bool,
    sync_config_sha256: [u8; 32],
    battery_sync_config_sha256: [u8; 32],
    any_mod_enabled: bool,
    any_mod_applied: bool,
    initial_input: NesTasInitialInput,
    configured_sample_rate: Option<u32>,
    initial_sample_rate: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct NesTasLoadSetup {
    pub(crate) loaded_from_source_path: bool,
    #[cfg(target_arch = "wasm32")]
    pub(crate) loaded_from_browser_bytes: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) tas_source_media_sha256: Option<[u8; 32]>,
    pub(crate) tas_sync_config_sha256: Option<[u8; 32]>,
    pub(crate) tas_battery_sync_config_sha256: Option<[u8; 32]>,
}

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub(crate) struct NesTasLoadProvenanceView<'a> {
    pub(crate) load: &'a NesTasLoadProvenance,
    pub(crate) current_sample_rate: u32,
}

impl NesTasLoadProvenanceSeed {
    pub(crate) fn new(
        raw_source_media_sha256: [u8; 32],
        raw_source_media_len: u64,
        source_path: &Path,
        rom_path: &Path,
        setup: NesTasLoadSetup,
    ) -> Self {
        let initial_sample_rate = setup
            .configured_sample_rate
            .unwrap_or(zeff_nes_core::hardware::constants::NES_DEFAULT_HOST_SAMPLE_RATE_HZ);
        let (buttons, dpad) = setup.initial_input.unwrap_or_default();
        Self {
            raw_source_media_sha256: setup
                .tas_source_media_sha256
                .unwrap_or(raw_source_media_sha256),
            raw_source_media_len,
            direct_nes_file: setup.loaded_from_source_path
                && direct_nes_file(source_path, rom_path),
            #[cfg(target_arch = "wasm32")]
            direct_browser_nes: setup.loaded_from_browser_bytes
                && direct_nes_file(source_path, rom_path),
            sync_config_sha256: setup.tas_sync_config_sha256.unwrap_or([0; 32]),
            battery_sync_config_sha256: setup.tas_battery_sync_config_sha256.unwrap_or([0; 32]),
            any_mod_enabled: setup.any_mod_enabled,
            any_mod_applied: setup.any_mod_applied,
            initial_input: NesTasInitialInput { buttons, dpad },
            configured_sample_rate: setup.configured_sample_rate,
            initial_sample_rate,
        }
    }

    pub(crate) fn finish(
        self,
        persistent_load: NesPersistentLoadOutcome,
        battery_backed: bool,
    ) -> NesTasLoadProvenance {
        NesTasLoadProvenance {
            netplay_media: None,
            raw_source_media_sha256: self.raw_source_media_sha256,
            raw_source_media_len: self.raw_source_media_len,
            initial_persistent_sha256: [0; 32],
            initial_state_sha256: None,
            direct_nes_file: self.direct_nes_file,
            #[cfg(target_arch = "wasm32")]
            direct_browser_nes: self.direct_browser_nes,
            sync_config_sha256: if battery_backed {
                self.battery_sync_config_sha256
            } else {
                self.sync_config_sha256
            },
            any_mod_enabled: self.any_mod_enabled,
            any_mod_applied: self.any_mod_applied,
            persistent_load,
            initial_input: self.initial_input,
            configured_sample_rate: self.configured_sample_rate,
            initial_sample_rate: self.initial_sample_rate,
        }
    }
}

impl NesBackend {
    pub(crate) fn set_netplay_media(
        &mut self,
        media: Option<crate::emu_backend::loader::NetplayRomMedia>,
    ) {
        if let Some(load) = &mut self.tas_load_provenance {
            load.netplay_media = media;
        }
    }

    pub(crate) fn with_load_provenance(
        emu: NesEmulator,
        rom_path: PathBuf,
        source_path: PathBuf,
        mut provenance: NesTasLoadProvenance,
    ) -> Self {
        provenance.initial_persistent_sha256 =
            zeff_firmware::sha256_bytes(&emu.dump_persistent_data().unwrap_or_default());
        provenance.initial_state_sha256 = emu
            .encode_state()
            .ok()
            .map(|bytes| zeff_firmware::sha256_bytes(&bytes));
        let current_sample_rate = provenance.initial_sample_rate;
        let sram_recovery =
            crate::save_paths::battery_sram_session(&rom_path, "nes", emu.rom_hash());
        Self {
            emu,
            paths: BackendPaths::with_source_path(rom_path, source_path),
            sram_recovery,
            tas_load_provenance: Some(provenance),
            current_sample_rate: Some(current_sample_rate),
            host_persistence_enabled: true,
        }
    }

    pub(crate) fn tas_load_provenance(&self) -> Option<NesTasLoadProvenanceView<'_>> {
        Some(NesTasLoadProvenanceView {
            load: self.tas_load_provenance.as_ref()?,
            current_sample_rate: self.current_sample_rate?,
        })
    }
}

pub(crate) fn persistent_load_outcome(
    result: &anyhow::Result<Option<String>>,
) -> NesPersistentLoadOutcome {
    match result {
        Ok(Some(_)) => NesPersistentLoadOutcome::Loaded,
        Ok(None) => NesPersistentLoadOutcome::Absent,
        Err(_) => NesPersistentLoadOutcome::Unknown,
    }
}

fn direct_nes_file(source_path: &Path, rom_path: &Path) -> bool {
    source_path == rom_path
        && rom_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("nes"))
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
