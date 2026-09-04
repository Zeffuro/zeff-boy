use std::path::Path;

use crate::emu_backend::capabilities::TasSourceMediaIdentity;

use super::WsBackend;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WsTasPersistentLoadOutcome {
    Absent,
    Loaded,
    Skipped,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WsTasLoadProvenance {
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: usize,
    pub(crate) tas_source_media_sha256: [u8; 32],
    pub(crate) tas_source_media_len: usize,
    pub(crate) tas_sync_config_sha256: [u8; 32],
    pub(crate) direct_ws_file: bool,
    pub(crate) source_system: Option<zeff_ws_core::hardware::cartridge::MinimumSystem>,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) persistent_load: WsTasPersistentLoadOutcome,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) initial_sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct WsTasLoadProvenanceSeed {
    provenance: WsTasLoadProvenance,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct WsTasLoadSetup {
    pub(crate) loaded_from_source_path: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct WsTasLoadProvenanceView<'a> {
    pub(crate) load: &'a WsTasLoadProvenance,
    pub(crate) current_sample_rate: u32,
    pub(crate) current_orientation: zeff_ws_core::hardware::cartridge::RomOrientation,
}

impl WsTasLoadProvenanceSeed {
    pub(crate) fn new(
        raw_source_media_sha256: [u8; 32],
        raw_source_media_len: usize,
        source_path: &Path,
        rom_path: &Path,
        setup: WsTasLoadSetup,
    ) -> Self {
        let tas_source_media = setup.tas_source_media.unwrap_or((
            raw_source_media_sha256,
            raw_source_media_len,
            [0; 32],
        ));
        Self {
            provenance: WsTasLoadProvenance {
                raw_source_media_sha256,
                raw_source_media_len,
                tas_source_media_sha256: tas_source_media.0,
                tas_source_media_len: tas_source_media.1,
                tas_sync_config_sha256: tas_source_media.2,
                direct_ws_file: setup.loaded_from_source_path
                    && direct_ws_file(source_path, rom_path),
                source_system: direct_ws_system(rom_path),
                any_mod_enabled: setup.any_mod_enabled,
                any_mod_applied: setup.any_mod_applied,
                persistent_load: WsTasPersistentLoadOutcome::Unknown,
                initial_input: setup.initial_input,
                configured_sample_rate: setup.configured_sample_rate,
                initial_sample_rate: setup
                    .configured_sample_rate
                    .unwrap_or(zeff_ws_core::emulator::DEFAULT_SAMPLE_RATE),
            },
        }
    }

    pub(crate) fn finish(
        mut self,
        persistent_load: WsTasPersistentLoadOutcome,
    ) -> WsTasLoadProvenance {
        self.provenance.persistent_load = persistent_load;
        self.provenance
    }
}

impl WsTasLoadProvenance {
    pub(crate) fn source_media_identity(self) -> TasSourceMediaIdentity {
        TasSourceMediaIdentity::new(self.tas_source_media_sha256, self.tas_source_media_len)
    }
}

impl WsBackend {
    pub(crate) fn with_tas_load_provenance(mut self, provenance: WsTasLoadProvenance) -> Self {
        self.tas_load_provenance = Some(provenance);
        self
    }

    pub(crate) fn tas_load_provenance(&self) -> Option<WsTasLoadProvenanceView<'_>> {
        Some(WsTasLoadProvenanceView {
            load: self.tas_load_provenance.as_ref()?,
            current_sample_rate: self.emu.sample_rate(),
            current_orientation: self.emu.preferred_orientation(),
        })
    }

    pub(crate) fn tas_source_media_identity(&self) -> Option<TasSourceMediaIdentity> {
        self.tas_load_provenance
            .map(WsTasLoadProvenance::source_media_identity)
    }
}

pub(crate) fn persistent_load_outcome(
    result: &anyhow::Result<Option<String>>,
) -> WsTasPersistentLoadOutcome {
    match result {
        Ok(Some(_)) => WsTasPersistentLoadOutcome::Loaded,
        Ok(None) => WsTasPersistentLoadOutcome::Absent,
        Err(_) => WsTasPersistentLoadOutcome::Unknown,
    }
}

fn direct_ws_file(source_path: &Path, rom_path: &Path) -> bool {
    source_path == rom_path && direct_ws_system(rom_path).is_some()
}

fn direct_ws_system(rom_path: &Path) -> Option<zeff_ws_core::hardware::cartridge::MinimumSystem> {
    let extension = rom_path.extension()?.to_str()?;
    if extension.eq_ignore_ascii_case("ws") {
        Some(zeff_ws_core::hardware::cartridge::MinimumSystem::WonderSwan)
    } else if extension.eq_ignore_ascii_case("wsc") {
        Some(zeff_ws_core::hardware::cartridge::MinimumSystem::WonderSwanColor)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_accepts_only_direct_ws_and_wsc_routes() {
        for (extension, expected) in [
            (
                "ws",
                zeff_ws_core::hardware::cartridge::MinimumSystem::WonderSwan,
            ),
            (
                "wsc",
                zeff_ws_core::hardware::cartridge::MinimumSystem::WonderSwanColor,
            ),
        ] {
            let path = std::path::PathBuf::from(format!("game.{extension}"));
            let provenance = WsTasLoadProvenanceSeed::new(
                [7; 32],
                128 * 1024,
                &path,
                &path,
                WsTasLoadSetup {
                    loaded_from_source_path: true,
                    ..Default::default()
                },
            )
            .finish(WsTasPersistentLoadOutcome::Absent);
            assert!(provenance.direct_ws_file);
            assert_eq!(provenance.source_system, Some(expected));
        }

        let path = Path::new("game.zip");
        let rejected = WsTasLoadProvenanceSeed::new(
            [7; 32],
            128 * 1024,
            path,
            path,
            WsTasLoadSetup::default(),
        )
        .finish(WsTasPersistentLoadOutcome::Unknown);
        assert!(!rejected.direct_ws_file);
        assert_eq!(rejected.source_system, None);
    }
}
