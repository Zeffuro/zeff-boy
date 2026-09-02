use std::path::Path;

use crate::emu_backend::capabilities::TasSourceMediaIdentity;

use super::Sega8Backend;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sg1000TasPersistentLoadOutcome {
    Absent,
    Loaded,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sg1000TasControllerModel {
    TwoStandardPads,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sg1000TasLoadProvenance {
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: usize,
    pub(crate) tas_source_media_sha256: [u8; 32],
    pub(crate) tas_source_media_len: usize,
    pub(crate) tas_sync_config_sha256: [u8; 32],
    pub(crate) direct_sg_file: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) persistent_load: Sg1000TasPersistentLoadOutcome,
    pub(crate) controller_model: Sg1000TasControllerModel,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) initial_sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sg1000TasLoadProvenanceSeed {
    provenance: Sg1000TasLoadProvenance,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sg1000TasLoadSetup {
    pub(crate) loaded_from_source_path: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Sg1000TasLoadProvenanceView<'a> {
    pub(crate) load: &'a Sg1000TasLoadProvenance,
    pub(crate) current_sample_rate: u32,
    pub(crate) current_controller_raw: [u8; 2],
}

impl Sg1000TasLoadProvenanceSeed {
    pub(crate) fn new(
        raw_source_media_sha256: [u8; 32],
        raw_source_media_len: usize,
        source_path: &Path,
        rom_path: &Path,
        setup: Sg1000TasLoadSetup,
    ) -> Self {
        let tas_source_media = setup.tas_source_media.unwrap_or((
            raw_source_media_sha256,
            raw_source_media_len,
            [0; 32],
        ));
        Self {
            provenance: Sg1000TasLoadProvenance {
                raw_source_media_sha256,
                raw_source_media_len,
                tas_source_media_sha256: tas_source_media.0,
                tas_source_media_len: tas_source_media.1,
                tas_sync_config_sha256: tas_source_media.2,
                direct_sg_file: setup.loaded_from_source_path
                    && direct_sg1000_file(source_path, rom_path),
                any_mod_enabled: setup.any_mod_enabled,
                any_mod_applied: setup.any_mod_applied,
                persistent_load: Sg1000TasPersistentLoadOutcome::Unknown,
                controller_model: Sg1000TasControllerModel::TwoStandardPads,
                initial_input: setup.initial_input,
                configured_sample_rate: setup.configured_sample_rate,
                initial_sample_rate: setup
                    .configured_sample_rate
                    .unwrap_or(zeff_sega8_core::emulator::DEFAULT_SAMPLE_RATE),
            },
        }
    }

    pub(crate) fn finish(
        mut self,
        persistent_load: Sg1000TasPersistentLoadOutcome,
    ) -> Sg1000TasLoadProvenance {
        self.provenance.persistent_load = persistent_load;
        self.provenance
    }
}

impl Sg1000TasLoadProvenance {
    pub(crate) fn source_media_identity(self) -> TasSourceMediaIdentity {
        TasSourceMediaIdentity::new(self.tas_source_media_sha256, self.tas_source_media_len)
    }
}

impl Sega8Backend {
    pub(crate) fn with_sg1000_tas_load_provenance(
        mut self,
        provenance: Sg1000TasLoadProvenance,
    ) -> Self {
        self.sg1000_tas_load_provenance = Some(provenance);
        self
    }

    pub(crate) fn sg1000_tas_load_provenance(&self) -> Option<Sg1000TasLoadProvenanceView<'_>> {
        use zeff_sega8_core::hardware::input::ControllerPort;

        Some(Sg1000TasLoadProvenanceView {
            load: self.sg1000_tas_load_provenance.as_ref()?,
            current_sample_rate: self.emu.sample_rate(),
            current_controller_raw: [
                self.emu.bus().input().read_controller(ControllerPort::One),
                self.emu.bus().input().read_controller(ControllerPort::Two),
            ],
        })
    }

    pub(crate) fn sg1000_tas_source_media_identity(&self) -> Option<TasSourceMediaIdentity> {
        self.sg1000_tas_load_provenance
            .map(Sg1000TasLoadProvenance::source_media_identity)
    }
}

pub(crate) fn sg1000_persistent_load_outcome(
    result: &anyhow::Result<Option<String>>,
) -> Sg1000TasPersistentLoadOutcome {
    match result {
        Ok(Some(_)) => Sg1000TasPersistentLoadOutcome::Loaded,
        Ok(None) => Sg1000TasPersistentLoadOutcome::Absent,
        Err(_) => Sg1000TasPersistentLoadOutcome::Unknown,
    }
}

fn direct_sg1000_file(source_path: &Path, rom_path: &Path) -> bool {
    source_path == rom_path
        && rom_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("sg") || extension.eq_ignore_ascii_case("sc")
            })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_accepts_only_direct_sg_and_sc_routes() {
        for extension in ["sg", "sc"] {
            let path = std::path::PathBuf::from(format!("game.{extension}"));
            let provenance = Sg1000TasLoadProvenanceSeed::new(
                [9; 32],
                8192,
                &path,
                &path,
                Sg1000TasLoadSetup {
                    loaded_from_source_path: true,
                    ..Default::default()
                },
            )
            .finish(Sg1000TasPersistentLoadOutcome::Absent);
            assert!(provenance.direct_sg_file);
        }
        let path = Path::new("game.sms");
        let rejected = Sg1000TasLoadProvenanceSeed::new(
            [9; 32],
            8192,
            path,
            path,
            Sg1000TasLoadSetup::default(),
        )
        .finish(Sg1000TasPersistentLoadOutcome::Unknown);
        assert!(!rejected.direct_sg_file);
        assert_eq!(
            rejected.persistent_load,
            Sg1000TasPersistentLoadOutcome::Unknown
        );
    }
}
