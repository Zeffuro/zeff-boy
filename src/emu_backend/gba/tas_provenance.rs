use std::path::Path;

use crate::emu_backend::capabilities::TasSourceMediaIdentity;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GbaTasPersistentLoadOutcome {
    Absent,
    Loaded,
    Skipped,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GbaTasInitialInput {
    pub(crate) buttons: u8,
    pub(crate) dpad: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GbaTasLoadProvenance {
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: usize,
    pub(crate) tas_source_media_sha256: [u8; 32],
    pub(crate) tas_source_media_len: usize,
    pub(crate) tas_sync_config_sha256: [u8; 32],
    pub(crate) direct_gba_file: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) persistent_load: GbaTasPersistentLoadOutcome,
    pub(crate) initial_input: GbaTasInitialInput,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) initial_sample_rate: u32,
    pub(crate) external_bios_selected: bool,
    pub(crate) rtc_seeded_from_host: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GbaTasLoadProvenanceSeed {
    provenance: GbaTasLoadProvenance,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GbaTasLoadSetup {
    pub(crate) loaded_from_source_path: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) external_bios_selected: bool,
    pub(crate) tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) struct GbaTasLoadProvenanceView<'a> {
    pub(crate) load: &'a GbaTasLoadProvenance,
    pub(crate) current_sample_rate: u32,
    pub(crate) external_bios_present: bool,
}

impl GbaTasLoadProvenanceSeed {
    pub(crate) fn new(
        raw_source_media_sha256: [u8; 32],
        raw_source_media_len: usize,
        source_path: &Path,
        rom_path: &Path,
        setup: GbaTasLoadSetup,
    ) -> Self {
        let (buttons, dpad) = setup.initial_input.unwrap_or_default();
        let (tas_source_media_sha256, tas_source_media_len, tas_sync_config_sha256) = setup
            .tas_source_media
            .unwrap_or((raw_source_media_sha256, raw_source_media_len, [0; 32]));
        Self {
            provenance: GbaTasLoadProvenance {
                raw_source_media_sha256,
                raw_source_media_len,
                tas_source_media_sha256,
                tas_source_media_len,
                tas_sync_config_sha256,
                direct_gba_file: (setup.loaded_from_source_path
                    && source_path == rom_path
                    && has_gba_extension(rom_path))
                    || setup.tas_source_media.is_some(),
                any_mod_enabled: setup.any_mod_enabled,
                any_mod_applied: setup.any_mod_applied,
                persistent_load: GbaTasPersistentLoadOutcome::Unknown,
                initial_input: GbaTasInitialInput { buttons, dpad },
                configured_sample_rate: setup.configured_sample_rate,
                initial_sample_rate: setup
                    .configured_sample_rate
                    .unwrap_or(zeff_gba_core::emulator::DEFAULT_SAMPLE_RATE),
                external_bios_selected: setup.external_bios_selected,
                rtc_seeded_from_host: false,
            },
        }
    }

    pub(crate) fn finish(
        mut self,
        persistent_load: GbaTasPersistentLoadOutcome,
        initial_sample_rate: u32,
        rtc_seeded_from_host: bool,
    ) -> GbaTasLoadProvenance {
        self.provenance.persistent_load = persistent_load;
        self.provenance.initial_sample_rate = initial_sample_rate;
        self.provenance.rtc_seeded_from_host = rtc_seeded_from_host;
        self.provenance
    }
}

impl GbaTasLoadProvenance {
    pub(crate) fn source_media_identity(self) -> TasSourceMediaIdentity {
        TasSourceMediaIdentity::new(self.tas_source_media_sha256, self.tas_source_media_len)
    }

    pub(crate) fn set_sync_config_sha256(&mut self, sync_config_sha256: [u8; 32]) {
        self.tas_sync_config_sha256 = sync_config_sha256;
    }
}

pub(crate) fn persistent_load_outcome(
    result: &anyhow::Result<Option<String>>,
) -> GbaTasPersistentLoadOutcome {
    match result {
        Ok(Some(_)) => GbaTasPersistentLoadOutcome::Loaded,
        Ok(None) => GbaTasPersistentLoadOutcome::Absent,
        Err(_) => GbaTasPersistentLoadOutcome::Unknown,
    }
}

fn has_gba_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("gba"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};

    fn rom() -> Vec<u8> {
        let mut rom = vec![0; 0xC0];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xB2] = 0x96;
        rom
    }

    #[test]
    fn seed_requires_a_direct_gba_source_and_preserves_runtime_choices() {
        let path = Path::new("game.gba");
        let provenance = GbaTasLoadProvenanceSeed::new(
            [7; 32],
            0xC0,
            path,
            path,
            GbaTasLoadSetup {
                loaded_from_source_path: true,
                initial_input: Some((0xFF, 0xFF)),
                configured_sample_rate: Some(48_000),
                external_bios_selected: true,
                ..Default::default()
            },
        )
        .finish(GbaTasPersistentLoadOutcome::Skipped, 48_000, false);
        assert!(provenance.direct_gba_file);
        assert_eq!(provenance.initial_input.buttons, 0xFF);
        assert_eq!(provenance.initial_input.dpad, 0xFF);
        assert!(provenance.external_bios_selected);
        assert_eq!(
            provenance.persistent_load,
            GbaTasPersistentLoadOutcome::Skipped
        );

        let archive = Path::new("game.zip");
        let rejected = GbaTasLoadProvenanceSeed::new(
            [7; 32],
            0xC0,
            archive,
            archive,
            GbaTasLoadSetup {
                loaded_from_source_path: true,
                ..Default::default()
            },
        )
        .finish(GbaTasPersistentLoadOutcome::Absent, 48_000, false);
        assert!(!rejected.direct_gba_file);
    }

    #[test]
    fn loader_records_direct_identity_and_skipped_battery_restore() {
        let directory = crate::test_support::test_directory("gba-tas-provenance").unwrap();
        let path = directory.path().join("game.gba");
        let rom = rom();
        std::fs::write(&path, &rom).unwrap();
        let backend = load_backend_from_rom_source(
            ActiveSystem::GameBoyAdvance,
            &path,
            &path,
            None,
            BackendLoadConfig {
                sample_rate: Some(48_000),
                initial_input: Some((0x31, 0x06)),
                gba_load_battery_sram: false,
                ..BackendLoadConfig::default()
            },
        )
        .unwrap()
        .backend;
        let view = backend.gba_tas_load_provenance().unwrap();
        assert!(view.load.direct_gba_file);
        assert_eq!(
            view.load.raw_source_media_sha256,
            zeff_firmware::sha256_bytes(&rom)
        );
        assert_eq!(view.load.raw_source_media_len, rom.len());
        assert_eq!(view.load.initial_input.buttons, 0x31);
        assert_eq!(view.load.initial_input.dpad, 0x06);
        assert_eq!(
            view.load.persistent_load,
            GbaTasPersistentLoadOutcome::Skipped
        );
        assert_eq!(view.current_sample_rate, 48_000);
        assert!(!view.external_bios_present);
        assert_eq!(
            backend.tas_source_media_identity().unwrap().sha256,
            zeff_firmware::sha256_bytes(&rom)
        );
        assert_eq!(
            backend.gba().unwrap().emu.rom_hash(),
            zeff_firmware::sha256_bytes(&rom)
        );
    }
}
