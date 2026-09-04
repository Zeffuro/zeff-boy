use std::path::Path;

use crate::emu_backend::capabilities::TasSourceMediaIdentity;

use super::Sega8Backend;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GameGearTasPersistentLoadOutcome {
    Absent,
    Loaded,
    Skipped,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GameGearTasControllerModel {
    BuiltInPadAndStart,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GameGearTasLoadProvenance {
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: usize,
    pub(crate) tas_source_media_sha256: [u8; 32],
    pub(crate) tas_source_media_len: usize,
    pub(crate) tas_sync_config_sha256: [u8; 32],
    pub(crate) direct_gg_file: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) persistent_load: GameGearTasPersistentLoadOutcome,
    pub(crate) controller_model: GameGearTasControllerModel,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) initial_sample_rate: u32,
    pub(crate) standard_mapper_ram_identity:
        Option<zeff_sega8_core::hardware::cartridge::GameGearStandardMapperRamIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GameGearTasLoadProvenanceSeed {
    provenance: GameGearTasLoadProvenance,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GameGearTasLoadSetup {
    pub(crate) loaded_from_source_path: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) standard_mapper_ram_identity:
        Option<zeff_sega8_core::hardware::cartridge::GameGearStandardMapperRamIdentity>,
    pub(crate) tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
}

impl GameGearTasLoadProvenanceSeed {
    pub(crate) fn new(
        raw_source_media_sha256: [u8; 32],
        raw_source_media_len: usize,
        source_path: &Path,
        rom_path: &Path,
        setup: GameGearTasLoadSetup,
    ) -> Self {
        let tas_source_media = setup.tas_source_media.unwrap_or((
            raw_source_media_sha256,
            raw_source_media_len,
            [0; 32],
        ));
        Self {
            provenance: GameGearTasLoadProvenance {
                raw_source_media_sha256,
                raw_source_media_len,
                tas_source_media_sha256: tas_source_media.0,
                tas_source_media_len: tas_source_media.1,
                tas_sync_config_sha256: tas_source_media.2,
                direct_gg_file: setup.loaded_from_source_path
                    && direct_game_gear_file(source_path, rom_path),
                any_mod_enabled: setup.any_mod_enabled,
                any_mod_applied: setup.any_mod_applied,
                persistent_load: GameGearTasPersistentLoadOutcome::Unknown,
                controller_model: GameGearTasControllerModel::BuiltInPadAndStart,
                initial_input: setup.initial_input,
                configured_sample_rate: setup.configured_sample_rate,
                initial_sample_rate: setup
                    .configured_sample_rate
                    .unwrap_or(zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE),
                standard_mapper_ram_identity: setup.standard_mapper_ram_identity,
            },
        }
    }

    pub(crate) fn finish(
        mut self,
        persistent_load: GameGearTasPersistentLoadOutcome,
    ) -> GameGearTasLoadProvenance {
        self.provenance.persistent_load = persistent_load;
        self.provenance
    }
}

impl GameGearTasLoadProvenance {
    pub(crate) fn source_media_identity(self) -> TasSourceMediaIdentity {
        TasSourceMediaIdentity::new(self.tas_source_media_sha256, self.tas_source_media_len)
    }

    pub(crate) fn set_sync_config_sha256(&mut self, sync_config_sha256: [u8; 32]) {
        self.tas_sync_config_sha256 = sync_config_sha256;
    }
}

impl Sega8Backend {
    pub(crate) fn with_game_gear_tas_load_provenance(
        mut self,
        provenance: GameGearTasLoadProvenance,
    ) -> Self {
        self.game_gear_tas_load_provenance = Some(provenance);
        self
    }

    pub(crate) fn game_gear_tas_load_provenance(&self) -> Option<&GameGearTasLoadProvenance> {
        self.game_gear_tas_load_provenance.as_ref()
    }

    pub(crate) fn game_gear_tas_source_media_identity(&self) -> Option<TasSourceMediaIdentity> {
        self.game_gear_tas_load_provenance()
            .copied()
            .map(GameGearTasLoadProvenance::source_media_identity)
    }

    pub(crate) fn set_game_gear_tas_sync_config_sha256(&mut self, sync_config_sha256: [u8; 32]) {
        if let Some(provenance) = &mut self.game_gear_tas_load_provenance {
            provenance.set_sync_config_sha256(sync_config_sha256);
        }
    }
}

pub(crate) fn game_gear_persistent_load_outcome(
    result: &anyhow::Result<Option<String>>,
) -> GameGearTasPersistentLoadOutcome {
    match result {
        Ok(Some(_)) => GameGearTasPersistentLoadOutcome::Loaded,
        Ok(None) => GameGearTasPersistentLoadOutcome::Absent,
        Err(_) => GameGearTasPersistentLoadOutcome::Unknown,
    }
}

fn direct_game_gear_file(source_path: &Path, rom_path: &Path) -> bool {
    source_path == rom_path
        && rom_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("gg"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};

    #[test]
    fn shared_loader_retains_direct_media_persistence_and_controller_provenance() {
        let dir = crate::test_support::test_directory("game-gear-tas-provenance").unwrap();
        let path = dir.path().join("synthetic.gg");
        let rom = vec![0x76; 64 * 1024];
        fs::write(&path, &rom).unwrap();

        let loaded = load_backend_from_rom_source(
            ActiveSystem::GameGear,
            &path,
            &path,
            None,
            BackendLoadConfig {
                initial_input: Some((0x09, 0x02)),
                ..BackendLoadConfig::default()
            },
        )
        .unwrap();
        let backend = loaded.backend.sega8().unwrap();
        let provenance = backend.game_gear_tas_load_provenance().unwrap();

        assert!(provenance.direct_gg_file);
        assert_eq!(
            provenance.raw_source_media_sha256,
            zeff_firmware::sha256_bytes(&rom)
        );
        assert_eq!(provenance.raw_source_media_len, rom.len());
        assert!(!provenance.any_mod_enabled);
        assert!(!provenance.any_mod_applied);
        assert_eq!(
            provenance.persistent_load,
            GameGearTasPersistentLoadOutcome::Absent
        );
        assert_eq!(
            provenance.controller_model,
            GameGearTasControllerModel::BuiltInPadAndStart
        );
        assert_eq!(provenance.initial_input, Some((0x09, 0x02)));
        assert_eq!(provenance.configured_sample_rate, None);
        assert_eq!(
            provenance.initial_sample_rate,
            zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE
        );
        assert_eq!(
            backend.emu.sample_rate(),
            zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE
        );
        use zeff_sega8_core::hardware::input::ControllerPort;
        assert_eq!(
            [
                backend
                    .emu
                    .bus()
                    .input()
                    .read_controller(ControllerPort::One),
                backend
                    .emu
                    .bus()
                    .input()
                    .read_controller(ControllerPort::Two),
            ],
            [0xEB, 0xFF]
        );
        assert!(backend.emu.bus().input().game_gear_start_pressed());
        assert_eq!(
            backend.emu.bus().game_gear_serial().debug_snapshot(),
            zeff_sega8_core::hardware::serial::GameGearSerial::new().debug_snapshot()
        );
        assert_eq!(
            backend.game_gear_tas_source_media_identity().unwrap(),
            TasSourceMediaIdentity::new(zeff_firmware::sha256_bytes(&rom), rom.len())
        );
    }

    #[test]
    fn seed_and_persistence_outcomes_fail_closed() {
        let path = Path::new("game.gg");
        let seed = GameGearTasLoadProvenanceSeed::new(
            [3; 32],
            8192,
            path,
            path,
            GameGearTasLoadSetup::default(),
        )
        .finish(GameGearTasPersistentLoadOutcome::Unknown);
        assert!(!seed.direct_gg_file);
        assert_eq!(
            seed.persistent_load,
            GameGearTasPersistentLoadOutcome::Unknown
        );

        assert_eq!(
            game_gear_persistent_load_outcome(&Ok(Some("save.sav".to_owned()))),
            GameGearTasPersistentLoadOutcome::Loaded
        );
        assert_eq!(
            game_gear_persistent_load_outcome(&Ok(None)),
            GameGearTasPersistentLoadOutcome::Absent
        );
        assert_eq!(
            game_gear_persistent_load_outcome(&Err(anyhow::anyhow!("load failed"))),
            GameGearTasPersistentLoadOutcome::Unknown
        );
    }
}
