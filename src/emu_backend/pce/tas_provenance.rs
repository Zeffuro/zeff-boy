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
    pub(crate) cdda_source: Option<PceTasCdSource>,
    pub(crate) cdda_selected_member_path_sha256: Option<[u8; 32]>,
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
    cdda_source: Option<PceTasCdSource>,
    cdda_selected_member_path_sha256: Option<[u8; 32]>,
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
    pub(crate) cdda_source: Option<PceTasCdSource>,
    pub(crate) cdda_selected_member_path_sha256: Option<[u8; 32]>,
    pub(crate) cdda_selected_member_explicitly_selected: bool,
    pub(crate) archive_ppf_patches: Vec<PceTasArchivePpfPatchIdentity>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PceTasCdSource {
    DirectCue,
    DirectCuePpf,
    DirectChd,
    DirectIsoCue,
    ArchiveCue(PceTasCdArchiveCarrier),
    ArchiveCuePpf(PceTasCdArchiveCarrier),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PceTasCdArchiveCarrier {
    SevenZip,
    Rar,
    Zip,
}

impl PceTasCdSource {
    pub(crate) const fn cdda_label(self) -> &'static str {
        match self {
            Self::DirectCue => "cue",
            Self::DirectCuePpf => "cue_ppf",
            Self::DirectChd => "chd",
            Self::DirectIsoCue => "iso_cue",
            Self::ArchiveCue(PceTasCdArchiveCarrier::SevenZip) => "archive_cue",
            Self::ArchiveCue(PceTasCdArchiveCarrier::Rar) => "rar_cue",
            Self::ArchiveCue(PceTasCdArchiveCarrier::Zip) => "zip_cue",
            Self::ArchiveCuePpf(_) => "archive_cue_ppf",
        }
    }

    const fn tas_witnesses(self) -> PceTasCdWitnesses {
        match self {
            Self::DirectCue => PceTasCdWitnesses {
                direct: true,
                ..PceTasCdWitnesses::EMPTY
            },
            Self::DirectCuePpf => PceTasCdWitnesses {
                direct: true,
                ppf: true,
                ..PceTasCdWitnesses::EMPTY
            },
            Self::DirectChd => PceTasCdWitnesses {
                direct: true,
                chd: true,
                ..PceTasCdWitnesses::EMPTY
            },
            Self::DirectIsoCue => PceTasCdWitnesses {
                direct: true,
                iso: true,
                ..PceTasCdWitnesses::EMPTY
            },
            Self::ArchiveCue(carrier) => PceTasCdWitnesses {
                direct: true,
                archive: matches!(carrier, PceTasCdArchiveCarrier::SevenZip),
                rar: matches!(carrier, PceTasCdArchiveCarrier::Rar),
                zip: matches!(carrier, PceTasCdArchiveCarrier::Zip),
                ..PceTasCdWitnesses::EMPTY
            },
            Self::ArchiveCuePpf(carrier) => PceTasCdWitnesses {
                direct: true,
                archive: matches!(carrier, PceTasCdArchiveCarrier::SevenZip),
                archive_ppf: true,
                rar: matches!(carrier, PceTasCdArchiveCarrier::Rar),
                zip: matches!(carrier, PceTasCdArchiveCarrier::Zip),
                ..PceTasCdWitnesses::EMPTY
            },
        }
    }

    const fn archive_carrier(self) -> Option<PceTasCdArchiveCarrier> {
        match self {
            Self::ArchiveCue(carrier) | Self::ArchiveCuePpf(carrier) => Some(carrier),
            Self::DirectCue | Self::DirectCuePpf | Self::DirectChd | Self::DirectIsoCue => None,
        }
    }
}

#[derive(Clone, Copy)]
struct PceTasCdWitnesses {
    direct: bool,
    chd: bool,
    iso: bool,
    ppf: bool,
    archive: bool,
    archive_ppf: bool,
    rar: bool,
    zip: bool,
}

impl PceTasCdWitnesses {
    const EMPTY: Self = Self {
        direct: false,
        chd: false,
        iso: false,
        ppf: false,
        archive: false,
        archive_ppf: false,
        rar: false,
        zip: false,
    };
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
            cdda_source: None,
            cdda_selected_member_path_sha256: None,
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
        let witnesses = media
            .cdda_source
            .map(PceTasCdSource::tas_witnesses)
            .unwrap_or(PceTasCdWitnesses::EMPTY);
        let carrier = media.cdda_source.and_then(PceTasCdSource::archive_carrier);
        let member = carrier.and(media.cdda_selected_member_path_sha256);
        let explicit = media.cdda_selected_member_explicitly_selected;
        Self {
            raw_source_media_sha256: media.raw_source_media_sha256,
            raw_source_media_len: media.raw_source_media_len,
            cdda_source: media.cdda_source,
            cdda_selected_member_path_sha256: member,
            direct_pce_file: false,
            direct_pce_cd: witnesses.direct,
            direct_pce_cd_chd: witnesses.chd,
            direct_pce_cd_iso: witnesses.iso,
            direct_pce_cd_ppf: witnesses.ppf,
            direct_pce_cd_archive: witnesses.archive,
            direct_pce_cd_archive_ppf: witnesses.archive_ppf,
            direct_pce_cd_rar: witnesses.rar,
            direct_pce_cd_zip: witnesses.zip,
            archive_cue_member_path_sha256: (carrier == Some(PceTasCdArchiveCarrier::SevenZip))
                .then_some(member)
                .flatten(),
            rar_cue_member_path_sha256: (carrier == Some(PceTasCdArchiveCarrier::Rar))
                .then_some(member)
                .flatten(),
            zip_cue_member_path_sha256: (carrier == Some(PceTasCdArchiveCarrier::Zip))
                .then_some(member)
                .flatten(),
            archive_cue_explicitly_selected: carrier == Some(PceTasCdArchiveCarrier::SevenZip)
                && explicit,
            rar_cue_explicitly_selected: carrier == Some(PceTasCdArchiveCarrier::Rar) && explicit,
            zip_cue_explicitly_selected: carrier == Some(PceTasCdArchiveCarrier::Zip) && explicit,
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
            cdda_source: self.cdda_source,
            cdda_selected_member_path_sha256: self.cdda_selected_member_path_sha256,
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
#[path = "tas_provenance/tests.rs"]
mod tests;
