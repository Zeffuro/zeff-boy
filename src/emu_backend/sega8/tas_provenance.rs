use std::path::Path;

use crate::emu_backend::capabilities::TasSourceMediaIdentity;

use super::Sega8Backend;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SmsTasLoadProvenance {
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: usize,
    pub(crate) tas_source_media_sha256: [u8; 32],
    pub(crate) tas_source_media_len: usize,
    pub(crate) tas_sync_config_sha256: [u8; 32],
    pub(crate) direct_sms_file: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) initial_sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SmsTasLoadProvenanceSeed {
    provenance: SmsTasLoadProvenance,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SmsTasLoadSetup {
    pub(crate) loaded_from_source_path: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SmsTasLoadProvenanceView<'a> {
    pub(crate) load: &'a SmsTasLoadProvenance,
    pub(crate) current_sample_rate: u32,
    pub(crate) current_controller_raw: [u8; 2],
}

impl SmsTasLoadProvenanceSeed {
    pub(crate) fn new(
        raw_source_media_sha256: [u8; 32],
        raw_source_media_len: usize,
        source_path: &Path,
        rom_path: &Path,
        setup: SmsTasLoadSetup,
    ) -> Self {
        let tas_source_media = setup.tas_source_media.unwrap_or((
            raw_source_media_sha256,
            raw_source_media_len,
            [0; 32],
        ));
        Self {
            provenance: SmsTasLoadProvenance {
                raw_source_media_sha256,
                raw_source_media_len,
                tas_source_media_sha256: tas_source_media.0,
                tas_source_media_len: tas_source_media.1,
                tas_sync_config_sha256: tas_source_media.2,
                direct_sms_file: setup.loaded_from_source_path
                    && direct_sms_file(source_path, rom_path),
                any_mod_enabled: setup.any_mod_enabled,
                any_mod_applied: setup.any_mod_applied,
                initial_input: setup.initial_input,
                configured_sample_rate: setup.configured_sample_rate,
                initial_sample_rate: setup
                    .configured_sample_rate
                    .unwrap_or(zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE),
            },
        }
    }

    pub(crate) fn finish(self) -> SmsTasLoadProvenance {
        self.provenance
    }
}

impl SmsTasLoadProvenance {
    pub(crate) fn source_media_identity(self) -> TasSourceMediaIdentity {
        TasSourceMediaIdentity::new(self.tas_source_media_sha256, self.tas_source_media_len)
    }
}

impl Sega8Backend {
    pub(crate) fn with_sms_tas_load_provenance(mut self, provenance: SmsTasLoadProvenance) -> Self {
        self.sms_tas_load_provenance = Some(provenance);
        self
    }

    pub(crate) fn sms_tas_load_provenance(&self) -> Option<SmsTasLoadProvenanceView<'_>> {
        use zeff_sega8_core::hardware::input::ControllerPort;

        Some(SmsTasLoadProvenanceView {
            load: self.sms_tas_load_provenance.as_ref()?,
            current_sample_rate: self.emu.sample_rate(),
            current_controller_raw: [
                self.emu.bus().input().read_controller(ControllerPort::One),
                self.emu.bus().input().read_controller(ControllerPort::Two),
            ],
        })
    }

    pub(crate) fn sms_tas_source_media_identity(&self) -> Option<TasSourceMediaIdentity> {
        self.sms_tas_load_provenance
            .map(SmsTasLoadProvenance::source_media_identity)
    }
}

fn direct_sms_file(source_path: &Path, rom_path: &Path) -> bool {
    source_path == rom_path
        && rom_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("sms"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn seed_marks_only_direct_sms_file_routes() {
        let path = PathBuf::from("game.sms");
        let direct = SmsTasLoadProvenanceSeed::new(
            [7; 32],
            8192,
            &path,
            &path,
            SmsTasLoadSetup {
                loaded_from_source_path: true,
                ..SmsTasLoadSetup::default()
            },
        )
        .finish();
        assert!(direct.direct_sms_file);

        let archived = SmsTasLoadProvenanceSeed::new(
            [7; 32],
            8192,
            Path::new("games.zip"),
            &path,
            SmsTasLoadSetup {
                loaded_from_source_path: true,
                ..SmsTasLoadSetup::default()
            },
        )
        .finish();
        assert!(!archived.direct_sms_file);
    }
}
