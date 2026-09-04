use std::path::Path;

use zeff_pce_core::hardware::{
    PceArcadeCardMode, PceConsoleWiring, PceControllerMode, PceHardwareTopology, PceHuCardBoard,
    PceMemoryBaseMode,
};

use super::PceBackend;
use crate::emu_backend::capabilities::TasSourceMediaIdentity;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PceTasPersistentLoadOutcome {
    Absent,
    Loaded,
    Skipped,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PceTasLoadProvenance {
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: usize,
    pub(crate) tas_source_media_sha256: [u8; 32],
    pub(crate) tas_source_media_len: usize,
    pub(crate) tas_sync_config_sha256: [u8; 32],
    pub(crate) direct_pce_file: bool,
    pub(crate) direct_pce_cd: bool,
    pub(crate) direct_pce_cd_chd: bool,
    pub(crate) direct_pce_cd_iso: bool,
    pub(crate) direct_pce_cd_ppf: bool,
    pub(crate) direct_pce_cd_archive: bool,
    pub(crate) direct_pce_cd_archive_ppf: bool,
    pub(crate) direct_pce_cd_rar: bool,
    pub(crate) direct_pce_cd_zip: bool,
    pub(crate) archive_cue_member_path_sha256: Option<[u8; 32]>,
    pub(crate) rar_cue_member_path_sha256: Option<[u8; 32]>,
    pub(crate) zip_cue_member_path_sha256: Option<[u8; 32]>,
    pub(crate) archive_cue_explicitly_selected: bool,
    pub(crate) rar_cue_explicitly_selected: bool,
    pub(crate) zip_cue_explicitly_selected: bool,
    pub(crate) archive_ppf_patches: Vec<PceTasArchivePpfPatchIdentity>,
    pub(crate) source_disc_sha256: Option<[u8; 32]>,
    pub(crate) effective_disc_sha256: Option<[u8; 32]>,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) persistent_load: PceTasPersistentLoadOutcome,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) initial_sample_rate: u32,
    pub(crate) selected_wiring: Option<PceConsoleWiring>,
    pub(crate) effective_wiring: PceConsoleWiring,
    pub(crate) selected_board: Option<PceHuCardBoard>,
    pub(crate) effective_board: PceHuCardBoard,
    pub(crate) selected_hardware: Option<zeff_pce_core::hardware::PceCartridgeHardware>,
    pub(crate) selected_controller_mode: PceControllerMode,
    pub(crate) effective_controller_mode: PceControllerMode,
    pub(crate) selected_memory_base_mode: PceMemoryBaseMode,
    pub(crate) effective_memory_base_mode: PceMemoryBaseMode,
    pub(crate) selected_arcade_card_mode: PceArcadeCardMode,
    pub(crate) effective_arcade_card_mode: PceArcadeCardMode,
    pub(crate) effective_topology: PceHardwareTopology,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PceTasLoadProvenanceSeed {
    raw_source_media_sha256: [u8; 32],
    raw_source_media_len: usize,
    direct_pce_file: bool,
    direct_pce_cd: bool,
    direct_pce_cd_chd: bool,
    direct_pce_cd_iso: bool,
    direct_pce_cd_ppf: bool,
    direct_pce_cd_archive: bool,
    direct_pce_cd_archive_ppf: bool,
    direct_pce_cd_rar: bool,
    direct_pce_cd_zip: bool,
    archive_cue_member_path_sha256: Option<[u8; 32]>,
    rar_cue_member_path_sha256: Option<[u8; 32]>,
    zip_cue_member_path_sha256: Option<[u8; 32]>,
    archive_cue_explicitly_selected: bool,
    rar_cue_explicitly_selected: bool,
    zip_cue_explicitly_selected: bool,
    archive_ppf_patches: Vec<PceTasArchivePpfPatchIdentity>,
    source_disc_sha256: Option<[u8; 32]>,
    effective_disc_sha256: Option<[u8; 32]>,
    tas_source_media: ([u8; 32], usize, [u8; 32]),
    setup: PceTasLoadSetup,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PceTasCdLoadMedia {
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: usize,
    pub(crate) source_disc_sha256: [u8; 32],
    pub(crate) effective_disc_sha256: [u8; 32],
    pub(crate) direct: bool,
    pub(crate) chd: bool,
    pub(crate) iso: bool,
    pub(crate) ppf: bool,
    pub(crate) archive: bool,
    pub(crate) archive_ppf: bool,
    pub(crate) rar: bool,
    pub(crate) zip: bool,
    pub(crate) archive_cue_member_path_sha256: Option<[u8; 32]>,
    pub(crate) rar_cue_member_path_sha256: Option<[u8; 32]>,
    pub(crate) zip_cue_member_path_sha256: Option<[u8; 32]>,
    pub(crate) archive_cue_explicitly_selected: bool,
    pub(crate) rar_cue_explicitly_selected: bool,
    pub(crate) zip_cue_explicitly_selected: bool,
    pub(crate) archive_ppf_patches: Vec<PceTasArchivePpfPatchIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PceTasArchivePpfPatchIdentity {
    pub(crate) member_path: String,
    pub(crate) len: usize,
    pub(crate) sha256: [u8; 32],
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PceTasLoadProvenanceView<'a> {
    pub(crate) load: &'a PceTasLoadProvenance,
    pub(crate) current_sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PceTasLoadSetup {
    pub(crate) loaded_from_source_path: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) selected_wiring: Option<PceConsoleWiring>,
    pub(crate) selected_board: Option<PceHuCardBoard>,
    pub(crate) selected_hardware: Option<zeff_pce_core::hardware::PceCartridgeHardware>,
    pub(crate) selected_controller_mode: PceControllerMode,
    pub(crate) selected_memory_base_mode: PceMemoryBaseMode,
    pub(crate) selected_arcade_card_mode: PceArcadeCardMode,
    pub(crate) tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
}

impl PceTasLoadProvenanceSeed {
    pub(crate) fn new(
        raw_source_media_sha256: [u8; 32],
        raw_source_media_len: usize,
        source_path: &Path,
        rom_path: &Path,
        setup: PceTasLoadSetup,
    ) -> Self {
        let tas_source_media = setup.tas_source_media.unwrap_or((
            raw_source_media_sha256,
            raw_source_media_len,
            [0; 32],
        ));
        Self {
            raw_source_media_sha256,
            raw_source_media_len,
            direct_pce_file: (setup.loaded_from_source_path
                && source_path == rom_path
                && rom_path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("pce")))
                || setup.tas_source_media.is_some(),
            direct_pce_cd: false,
            direct_pce_cd_chd: false,
            direct_pce_cd_iso: false,
            direct_pce_cd_ppf: false,
            direct_pce_cd_archive: false,
            direct_pce_cd_archive_ppf: false,
            direct_pce_cd_rar: false,
            direct_pce_cd_zip: false,
            archive_cue_member_path_sha256: None,
            rar_cue_member_path_sha256: None,
            zip_cue_member_path_sha256: None,
            archive_cue_explicitly_selected: false,
            rar_cue_explicitly_selected: false,
            zip_cue_explicitly_selected: false,
            archive_ppf_patches: Vec::new(),
            source_disc_sha256: None,
            effective_disc_sha256: None,
            tas_source_media,
            setup,
        }
    }

    pub(crate) fn new_cd(media: PceTasCdLoadMedia, setup: PceTasLoadSetup) -> Self {
        let tas_source_media = setup.tas_source_media.unwrap_or((
            media.raw_source_media_sha256,
            media.raw_source_media_len,
            [0; 32],
        ));
        Self {
            raw_source_media_sha256: media.raw_source_media_sha256,
            raw_source_media_len: media.raw_source_media_len,
            direct_pce_file: false,
            direct_pce_cd: media.direct,
            direct_pce_cd_chd: media.chd,
            direct_pce_cd_iso: media.iso,
            direct_pce_cd_ppf: media.ppf,
            direct_pce_cd_archive: media.archive,
            direct_pce_cd_archive_ppf: media.archive_ppf,
            direct_pce_cd_rar: media.rar,
            direct_pce_cd_zip: media.zip,
            archive_cue_member_path_sha256: media.archive_cue_member_path_sha256,
            rar_cue_member_path_sha256: media.rar_cue_member_path_sha256,
            zip_cue_member_path_sha256: media.zip_cue_member_path_sha256,
            archive_cue_explicitly_selected: media.archive_cue_explicitly_selected,
            rar_cue_explicitly_selected: media.rar_cue_explicitly_selected,
            zip_cue_explicitly_selected: media.zip_cue_explicitly_selected,
            archive_ppf_patches: media.archive_ppf_patches,
            source_disc_sha256: Some(media.source_disc_sha256),
            effective_disc_sha256: Some(media.effective_disc_sha256),
            tas_source_media,
            setup,
        }
    }

    pub(crate) fn finish(
        self,
        backend: &PceBackend,
        persistent_load: PceTasPersistentLoadOutcome,
    ) -> PceTasLoadProvenance {
        PceTasLoadProvenance {
            raw_source_media_sha256: self.raw_source_media_sha256,
            raw_source_media_len: self.raw_source_media_len,
            tas_source_media_sha256: self.tas_source_media.0,
            tas_source_media_len: self.tas_source_media.1,
            tas_sync_config_sha256: self.tas_source_media.2,
            direct_pce_file: self.direct_pce_file,
            direct_pce_cd: self.direct_pce_cd,
            direct_pce_cd_chd: self.direct_pce_cd_chd,
            direct_pce_cd_iso: self.direct_pce_cd_iso,
            direct_pce_cd_ppf: self.direct_pce_cd_ppf,
            direct_pce_cd_archive: self.direct_pce_cd_archive,
            direct_pce_cd_archive_ppf: self.direct_pce_cd_archive_ppf,
            direct_pce_cd_rar: self.direct_pce_cd_rar,
            direct_pce_cd_zip: self.direct_pce_cd_zip,
            archive_cue_member_path_sha256: self.archive_cue_member_path_sha256,
            rar_cue_member_path_sha256: self.rar_cue_member_path_sha256,
            zip_cue_member_path_sha256: self.zip_cue_member_path_sha256,
            archive_cue_explicitly_selected: self.archive_cue_explicitly_selected,
            rar_cue_explicitly_selected: self.rar_cue_explicitly_selected,
            zip_cue_explicitly_selected: self.zip_cue_explicitly_selected,
            archive_ppf_patches: self.archive_ppf_patches,
            source_disc_sha256: self.source_disc_sha256,
            effective_disc_sha256: self.effective_disc_sha256,
            any_mod_enabled: self.setup.any_mod_enabled,
            any_mod_applied: self.setup.any_mod_applied,
            persistent_load,
            initial_input: self.setup.initial_input,
            configured_sample_rate: self.setup.configured_sample_rate,
            initial_sample_rate: backend.pce_sample_rate(),
            selected_wiring: self.setup.selected_wiring,
            effective_wiring: backend.console_wiring(),
            selected_board: self.setup.selected_board,
            effective_board: backend.hucard_board(),
            selected_hardware: self.setup.selected_hardware,
            selected_controller_mode: self.setup.selected_controller_mode,
            effective_controller_mode: backend.controller_mode(),
            selected_memory_base_mode: self.setup.selected_memory_base_mode,
            effective_memory_base_mode: backend.memory_base_mode(),
            selected_arcade_card_mode: self.setup.selected_arcade_card_mode,
            effective_arcade_card_mode: backend.arcade_card_mode(),
            effective_topology: backend.hardware_topology(),
        }
    }
}

impl PceTasLoadProvenance {
    pub(crate) fn source_media_identity(&self) -> TasSourceMediaIdentity {
        TasSourceMediaIdentity::new(self.tas_source_media_sha256, self.tas_source_media_len)
    }
}

impl PceBackend {
    pub(crate) fn with_tas_load_provenance(mut self, provenance: PceTasLoadProvenance) -> Self {
        self.tas_load_provenance = Some(provenance);
        self
    }

    pub(crate) fn tas_load_provenance(&self) -> Option<PceTasLoadProvenanceView<'_>> {
        Some(PceTasLoadProvenanceView {
            load: self.tas_load_provenance.as_ref()?,
            current_sample_rate: self.pce_sample_rate(),
        })
    }

    pub(crate) fn tas_source_media_identity(&self) -> Option<TasSourceMediaIdentity> {
        self.tas_load_provenance
            .as_ref()
            .map(PceTasLoadProvenance::source_media_identity)
    }

    pub(crate) fn pce_sample_rate(&self) -> u32 {
        self.machine.devices().psg().debug_snapshot().sample_rate
    }
}

pub(crate) fn pce_persistent_load_outcome(
    result: &anyhow::Result<Option<String>>,
) -> PceTasPersistentLoadOutcome {
    match result {
        Ok(Some(_)) => PceTasPersistentLoadOutcome::Loaded,
        Ok(None) => PceTasPersistentLoadOutcome::Absent,
        Err(_) => PceTasPersistentLoadOutcome::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};

    fn setup() -> PceTasLoadSetup {
        PceTasLoadSetup {
            loaded_from_source_path: true,
            any_mod_enabled: false,
            any_mod_applied: false,
            initial_input: None,
            configured_sample_rate: None,
            selected_wiring: None,
            selected_board: None,
            selected_hardware: None,
            selected_controller_mode: PceControllerMode::Automatic,
            selected_memory_base_mode: PceMemoryBaseMode::Automatic,
            selected_arcade_card_mode: PceArcadeCardMode::Automatic,
            tas_source_media: None,
        }
    }

    #[test]
    fn seed_accepts_only_a_direct_pce_file() {
        let path = Path::new("game.pce");
        let backend = PceBackend::new(vec![0; 0x2000], path.to_path_buf()).unwrap();
        let direct = PceTasLoadProvenanceSeed::new([3; 32], 0x2000, path, path, setup())
            .finish(&backend, PceTasPersistentLoadOutcome::Absent);
        assert!(direct.direct_pce_file);

        let archive = Path::new("game.zip");
        let nested = Path::new("game.pce");
        let rejected = PceTasLoadProvenanceSeed::new([3; 32], 0x2000, archive, nested, setup())
            .finish(&backend, PceTasPersistentLoadOutcome::Unknown);
        assert!(!rejected.direct_pce_file);
    }

    #[test]
    fn shared_loader_retains_raw_and_effective_hucard_facts() {
        let dir = crate::test_support::test_directory("pce-tas-provenance").unwrap();
        let path = dir.path().join("synthetic.pce");
        let mut raw = vec![0; 512];
        raw[0] = 1;
        raw.extend(vec![0xEA; 0x2000]);
        fs::write(&path, &raw).unwrap();

        let loaded = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &path,
            &path,
            None,
            BackendLoadConfig {
                sample_rate: Some(48_000),
                initial_input: Some((0x01, 0x01)),
                pce_console_wiring: Some(PceConsoleWiring::PcEngine),
                pce_hucard_board: Some(PceHuCardBoard::Plain),
                pce_cartridge_hardware: Some(zeff_pce_core::hardware::PceCartridgeHardware::Base),
                pce_arcade_card_mode: PceArcadeCardMode::Disabled,
                pce_load_battery_bram: false,
                ..BackendLoadConfig::default()
            },
        )
        .unwrap();
        let crate::emu_backend::EmuBackend::Pce(backend) = loaded.backend else {
            panic!("PC Engine loader returned a different backend");
        };
        let view = backend.tas_load_provenance().unwrap();
        let provenance = view.load;

        assert!(provenance.direct_pce_file);
        assert_eq!(
            provenance.raw_source_media_sha256,
            zeff_firmware::sha256_bytes(&raw)
        );
        assert_eq!(provenance.raw_source_media_len, raw.len());
        assert_eq!(
            provenance.persistent_load,
            PceTasPersistentLoadOutcome::Skipped
        );
        assert_eq!(provenance.initial_input, Some((0x01, 0x01)));
        assert_eq!(provenance.configured_sample_rate, Some(48_000));
        assert_eq!(provenance.initial_sample_rate, 48_000);
        assert_eq!(view.current_sample_rate, 48_000);
        assert_eq!(backend.pce_sample_rate(), 48_000);
        assert_eq!(provenance.selected_wiring, Some(PceConsoleWiring::PcEngine));
        assert_eq!(provenance.effective_wiring, PceConsoleWiring::PcEngine);
        assert_eq!(provenance.selected_board, Some(PceHuCardBoard::Plain));
        assert_eq!(provenance.effective_board, PceHuCardBoard::Plain);
        assert_eq!(provenance.effective_topology, PceHardwareTopology::Base);
        assert_eq!(
            provenance.effective_controller_mode,
            PceControllerMode::TwoButton
        );
        assert_eq!(
            provenance.effective_memory_base_mode,
            PceMemoryBaseMode::Disabled
        );
        assert_eq!(
            provenance.effective_arcade_card_mode,
            PceArcadeCardMode::Disabled
        );
        assert_eq!(
            backend.tas_source_media_identity(),
            Some(TasSourceMediaIdentity::new(
                zeff_firmware::sha256_bytes(&raw),
                raw.len(),
            ))
        );
    }

    #[test]
    fn persistence_outcomes_fail_closed() {
        assert_eq!(
            pce_persistent_load_outcome(&Ok(Some("memory-base.bin".to_owned()))),
            PceTasPersistentLoadOutcome::Loaded
        );
        assert_eq!(
            pce_persistent_load_outcome(&Ok(None)),
            PceTasPersistentLoadOutcome::Absent
        );
        assert_eq!(
            pce_persistent_load_outcome(&Err(anyhow::anyhow!("load failed"))),
            PceTasPersistentLoadOutcome::Unknown
        );
    }
}
