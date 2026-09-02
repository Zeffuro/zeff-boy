use std::path::Path;

use super::ColecoBackend;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColecoTasLoadProvenance {
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: usize,
    pub(crate) tas_source_media_sha256: [u8; 32],
    pub(crate) tas_source_media_len: usize,
    pub(crate) tas_sync_config_sha256: [u8; 32],
    pub(crate) direct_col_file: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) initial_sample_rate: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ColecoTasLoadProvenanceSeed {
    provenance: ColecoTasLoadProvenance,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ColecoTasLoadSetup {
    pub(crate) loaded_from_source_path: bool,
    pub(crate) any_mod_enabled: bool,
    pub(crate) any_mod_applied: bool,
    pub(crate) initial_input: Option<(u8, u8)>,
    pub(crate) configured_sample_rate: Option<u32>,
    pub(crate) tas_source_media: Option<([u8; 32], usize, [u8; 32])>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ColecoTasLoadProvenanceView<'a> {
    pub(crate) load: &'a ColecoTasLoadProvenance,
    pub(crate) current_sample_rate: u32,
    pub(crate) current_controllers: [zeff_coleco_core::StandardController; 2],
}

impl ColecoTasLoadProvenanceSeed {
    pub(crate) fn new(
        raw_source_media_sha256: [u8; 32],
        raw_source_media_len: usize,
        source_path: &Path,
        rom_path: &Path,
        setup: ColecoTasLoadSetup,
    ) -> Self {
        let tas_source_media = setup.tas_source_media.unwrap_or((
            raw_source_media_sha256,
            raw_source_media_len,
            [0; 32],
        ));
        Self {
            provenance: ColecoTasLoadProvenance {
                raw_source_media_sha256,
                raw_source_media_len,
                tas_source_media_sha256: tas_source_media.0,
                tas_source_media_len: tas_source_media.1,
                tas_sync_config_sha256: tas_source_media.2,
                direct_col_file: setup.loaded_from_source_path
                    && direct_col_file(source_path, rom_path),
                any_mod_enabled: setup.any_mod_enabled,
                any_mod_applied: setup.any_mod_applied,
                initial_input: setup.initial_input,
                configured_sample_rate: setup.configured_sample_rate,
                initial_sample_rate: setup
                    .configured_sample_rate
                    .unwrap_or(zeff_coleco_core::constants::DEFAULT_SAMPLE_RATE),
            },
        }
    }

    pub(crate) fn finish(self) -> ColecoTasLoadProvenance {
        self.provenance
    }
}

impl ColecoBackend {
    pub(crate) fn with_tas_load_provenance(mut self, provenance: ColecoTasLoadProvenance) -> Self {
        self.tas_load_provenance = Some(provenance);
        self
    }
}

pub(super) fn view(backend: &ColecoBackend) -> Option<ColecoTasLoadProvenanceView<'_>> {
    Some(ColecoTasLoadProvenanceView {
        load: backend.tas_load_provenance.as_ref()?,
        current_sample_rate: backend.emu.sample_rate(),
        current_controllers: [
            backend.emu.controller_ports().player(0)?,
            backend.emu.controller_ports().player(1)?,
        ],
    })
}

fn direct_col_file(source_path: &Path, rom_path: &Path) -> bool {
    source_path == rom_path
        && rom_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("col"))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn seed_records_only_a_direct_col_route() {
        let path = PathBuf::from("game.col");
        let direct = ColecoTasLoadProvenanceSeed::new(
            [7; 32],
            8192,
            &path,
            &path,
            ColecoTasLoadSetup {
                loaded_from_source_path: true,
                ..ColecoTasLoadSetup::default()
            },
        )
        .finish();
        assert!(direct.direct_col_file);

        let archived = ColecoTasLoadProvenanceSeed::new(
            [7; 32],
            8192,
            Path::new("games.zip"),
            &path,
            ColecoTasLoadSetup {
                loaded_from_source_path: true,
                ..ColecoTasLoadSetup::default()
            },
        )
        .finish();
        assert!(!archived.direct_col_file);
    }
}
