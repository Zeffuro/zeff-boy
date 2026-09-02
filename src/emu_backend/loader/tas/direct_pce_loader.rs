use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;
use zeff_pce_core::hardware::{
    PCEAS_HEADER_LEN, PceArcadeCardMode, PceCartridgeDescriptor, PceCartridgeHardware,
    PceConsoleWiring, PceControllerMode, PceHardwareTopology, PceHuCardBoard, PceMemoryBaseMode,
    normalize_hucard_image,
};

#[cfg(test)]
use super::direct_pce::{
    direct_pce_tas_project_board, direct_pce_tas_sync_config_sha256_for_board,
    zip_pce_tas_sync_config_sha256_for_board,
};
use super::media::read_bounded_direct_rom;
use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, TasDigest, TasEditorExecutionEngine,
    TasEditorExecutionProvider, TasExecutionSession, TasInitialBranch, TasProject,
    direct_pce::{
        PceTasHardwareProfile, direct_pce_tas_identity, direct_pce_tas_project_profile,
        direct_pce_tas_sync_config_sha256_for_profile, zip_pce_tas_identity,
        zip_pce_tas_sync_config_sha256_for_profile,
    },
    has_extension, validate_direct_pce_six_button_tas_runtime, validate_direct_pce_tas_runtime,
    validate_direct_pce_tas_state,
};

pub(crate) const MAX_DIRECT_PCE_HUCARD_BYTES: u64 = 4 * 1024 * 1024 + PCEAS_HEADER_LEN as u64;
const MAX_PCE_ZIP_BYTES: u64 = 128 * 1024 * 1024;
#[cfg(test)]
const TEST_POPULOUS_HUCARD_SHA256: [u8; 32] = [
    0x8E, 0x73, 0x6C, 0x39, 0xFB, 0x20, 0xD8, 0x95, 0x0F, 0x21, 0x2B, 0x45, 0xD1, 0x69, 0x57, 0x0C,
    0x13, 0x05, 0x03, 0x48, 0xAD, 0x48, 0x09, 0x52, 0x2F, 0x05, 0xFC, 0xC6, 0x4C, 0x15, 0x65, 0xC5,
];
#[cfg(test)]
const TEST_SUPERGRAFX_HUCARD_SHA256: [u8; 32] = [
    0x5E, 0xAB, 0x93, 0x29, 0xA2, 0x5A, 0xC6, 0xB0, 0xD5, 0xD9, 0x0A, 0xE9, 0x1B, 0x34, 0xBF, 0x31,
    0xC2, 0x5A, 0x13, 0xA7, 0xFF, 0x3F, 0x82, 0x72, 0xF2, 0x98, 0xC6, 0x59, 0x4B, 0x4D, 0xB7, 0xC4,
];

#[derive(Clone, Debug)]
pub(crate) struct DirectPceTasExecutionLoader {
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
    controller_mode: PceControllerMode,
}

pub(crate) struct PceTasMediaIdentity {
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
}

impl DirectPceTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf) -> Self {
        Self {
            source_path,
            rom_path: None,
            controller_mode: PceControllerMode::TwoButton,
        }
    }

    pub(crate) fn new_zip(source_path: PathBuf, rom_path: Option<PathBuf>) -> Self {
        Self {
            source_path,
            rom_path,
            controller_mode: PceControllerMode::TwoButton,
        }
    }

    pub(crate) fn new_six_button(source_path: PathBuf) -> Self {
        Self {
            source_path,
            rom_path: None,
            controller_mode: PceControllerMode::SixButton,
        }
    }

    pub(crate) fn new_zip_six_button(source_path: PathBuf, rom_path: Option<PathBuf>) -> Self {
        Self {
            source_path,
            rom_path,
            controller_mode: PceControllerMode::SixButton,
        }
    }

    pub(crate) fn new_zip_for_project(source_path: PathBuf, project: &TasProject) -> Result<Self> {
        super::direct_pce::validate_direct_pce_tas_project_identity(project)?;
        let profile = direct_pce_tas_project_profile(project)?;
        let inspection = crate::rom_archive::inspect_bounded_zip_members(
            &source_path,
            "pce",
            MAX_PCE_ZIP_BYTES,
            MAX_DIRECT_PCE_HUCARD_BYTES,
        )?;
        ensure!(
            TasDigest(inspection.archive_sha256) == project.identity().source_media_sha256,
            "PC Engine ZIP archive does not match the TAS project"
        );
        let matches = inspection
            .entries
            .into_iter()
            .filter(|entry| {
                zip_pce_tas_sync_config_sha256_for_profile(profile, &entry.member_name)
                    == project.identity().sync_config_sha256
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "PC Engine ZIP member does not match the TAS project"
        );
        Ok(Self {
            source_path,
            rom_path: Some(matches[0].rom_path.clone()),
            controller_mode: profile.controller_mode,
        })
    }

    pub(crate) fn validate_project_branch_scope(
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        super::direct_pce::validate_direct_pce_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, media) = self.load_fresh_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = self.identity(&backend, media, &start_state)?;
        TasProject::new(
            format!("pce-{}", identity.source_media_sha256.to_hex()),
            identity,
            start_state,
            ReplayStartMetadata::default(),
            TasInitialBranch {
                id: "main".to_owned(),
                name: "Main".to_owned(),
                frame_count: 1,
                input_spans: Vec::new(),
                events: Vec::new(),
            },
            BTreeMap::new(),
        )
    }

    pub(crate) fn create_project_file(&self, path: &Path) -> Result<TasProject> {
        ensure!(
            TasProject::is_project_path(path),
            "TAS projects must use the .ztas extension"
        );
        let project = self.create_project()?;
        super::publish_new_project(path, &project)?;
        Ok(project)
    }

    pub(crate) fn replace_project_file(&self, path: &Path) -> Result<TasProject> {
        ensure!(
            TasProject::is_project_path(path),
            "TAS projects must use the .ztas extension"
        );
        ensure!(path.exists(), "TAS project destination does not exist");
        let project = self.create_project()?;
        project.save_atomic(path).with_context(|| {
            format!(
                "failed to atomically replace TAS project {}",
                path.display()
            )
        })?;
        Ok(project)
    }

    pub(crate) fn load_session(&self, start_state: &[u8]) -> Result<TasExecutionSession> {
        let (mut backend, media) = self.load_fresh_backend()?;
        let projection = validate_direct_pce_tas_state(&mut backend, start_state)?;
        let pce = backend
            .pce()
            .context("PC Engine backend became unavailable")?;
        ensure!(
            projection.frame_count == backend.frame_count()
                && projection.framebuffer.as_ref() == pce.tas_core_framebuffer()
                && pce.tas_presented_frame_is_current(),
            "PC Engine TAS starting state did not restore exact frame output"
        );
        let identity = self.identity(&backend, media, start_state)?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn load_editor_engine(
        &self,
        project: &TasProject,
    ) -> Result<TasEditorExecutionEngine> {
        for branch in project.branches() {
            Self::validate_project_branch_scope(project, branch.id())?;
        }
        let session = self.load_session(project.start_state())?;
        TasEditorExecutionEngine::attach(
            project,
            session,
            super::direct_pce::validate_direct_pce_tas_branch_scope,
        )
    }

    pub(crate) fn load_fresh_backend(&self) -> Result<(EmuBackend, PceTasMediaIdentity)> {
        let (backend, media) = if has_extension(&self.source_path, "pce") {
            let source_bytes = read_direct_pce_hucard(&self.source_path)?;
            let profile = self.classify_hardware(&source_bytes)?;
            let media = PceTasMediaIdentity {
                source_media_sha256: TasDigest::from_bytes(&source_bytes),
                sync_config_sha256: direct_pce_tas_sync_config_sha256_for_profile(profile),
            };
            let backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
                ActiveSystem::Pce,
                &self.source_path,
                source_bytes,
                pce_tas_load_config(profile),
            )?
            .backend;
            (backend, media)
        } else if has_extension(&self.source_path, "zip") {
            let selected = crate::rom_archive::extract_bounded_zip_member(
                &self.source_path,
                self.rom_path.as_deref(),
                "pce",
                MAX_PCE_ZIP_BYTES,
                MAX_DIRECT_PCE_HUCARD_BYTES,
            )?;
            let profile = self.classify_hardware(&selected.bytes)?;
            let media = PceTasMediaIdentity {
                source_media_sha256: TasDigest(selected.archive_sha256),
                sync_config_sha256: zip_pce_tas_sync_config_sha256_for_profile(
                    profile,
                    &selected.member_name,
                ),
            };
            let backend = super::load_backend_from_rom_source(
                ActiveSystem::Pce,
                &self.source_path,
                &selected.rom_path,
                Some(selected.bytes),
                pce_tas_load_config(profile),
            )?
            .backend;
            (backend, media)
        } else {
            anyhow::bail!(
                "PC Engine TAS execution requires a direct .pce file or selected ZIP member"
            );
        };
        match self.controller_mode {
            PceControllerMode::TwoButton => {
                validate_direct_pce_tas_runtime(&backend, false)?;
            }
            PceControllerMode::SixButton => {
                validate_direct_pce_six_button_tas_runtime(&backend, false)?;
            }
            _ => anyhow::bail!("PC Engine TAS execution requires a supported controller"),
        }
        Ok((backend, media))
    }

    fn identity(
        &self,
        backend: &EmuBackend,
        media: PceTasMediaIdentity,
        start_state: &[u8],
    ) -> Result<crate::tas_project::TasProjectIdentity> {
        if has_extension(&self.source_path, "pce") {
            let source_bytes = read_direct_pce_hucard(&self.source_path)?;
            ensure!(
                media.source_media_sha256 == TasDigest::from_bytes(&source_bytes),
                "PC Engine source changed while constructing TAS identity"
            );
            return direct_pce_tas_identity(backend, &source_bytes, start_state);
        }
        let selected = crate::rom_archive::extract_bounded_zip_member(
            &self.source_path,
            self.rom_path.as_deref(),
            "pce",
            MAX_PCE_ZIP_BYTES,
            MAX_DIRECT_PCE_HUCARD_BYTES,
        )?;
        let profile = self.classify_hardware(&selected.bytes)?;
        ensure!(
            media.source_media_sha256 == TasDigest(selected.archive_sha256)
                && media.sync_config_sha256
                    == zip_pce_tas_sync_config_sha256_for_profile(profile, &selected.member_name)
                && TasDigest::from_bytes(&selected.bytes)
                    == TasDigest(
                        backend
                            .pce()
                            .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
                            .context("PC Engine backend omitted load provenance")?
                            .load
                            .raw_source_media_sha256,
                    ),
            "PC Engine ZIP changed while constructing TAS identity"
        );
        zip_pce_tas_identity(
            backend,
            selected.archive_sha256,
            &selected.member_name,
            start_state,
        )
    }

    fn classify_hardware(&self, source_bytes: &[u8]) -> Result<PceTasHardwareProfile> {
        let mut profile = classify_direct_pce_tas_hardware(source_bytes)?;
        profile.controller_mode = self.controller_mode;
        if profile.controller_mode == PceControllerMode::SixButton {
            ensure!(
                profile.board == PceHuCardBoard::Plain
                    && profile.topology == PceHardwareTopology::Base,
                "six-button PC Engine TAS requires a Base plain HuCard"
            );
        }
        Ok(profile)
    }
}

impl TasEditorExecutionProvider for DirectPceTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectPceTasExecutionLoader::load_editor_engine(self, project)
    }
}

fn pce_tas_load_config(profile: PceTasHardwareProfile) -> BackendLoadConfig {
    BackendLoadConfig {
        sample_rate: Some(48_000),
        apply_mods: false,
        initial_input: None,
        pce_console_wiring: Some(PceConsoleWiring::PcEngine),
        pce_hucard_board: Some(profile.board),
        pce_cartridge_hardware: Some(match profile.topology {
            PceHardwareTopology::Base => PceCartridgeHardware::Base,
            PceHardwareTopology::SuperGrafx => PceCartridgeHardware::SuperGrafx,
        }),
        pce_controller_mode: profile.controller_mode,
        pce_memory_base_mode: PceMemoryBaseMode::Disabled,
        pce_arcade_card_mode: PceArcadeCardMode::Disabled,
        pce_load_battery_bram: false,
        ..BackendLoadConfig::default()
    }
}

#[cfg(test)]
pub(crate) fn classify_direct_pce_tas_board(source_bytes: &[u8]) -> Result<PceHuCardBoard> {
    Ok(classify_direct_pce_tas_hardware(source_bytes)?.board)
}

pub(crate) fn classify_direct_pce_tas_hardware(
    source_bytes: &[u8],
) -> Result<PceTasHardwareProfile> {
    let normalized = normalize_hucard_image(source_bytes.to_vec())?;
    let normalized_sha256 = zeff_firmware::sha256_bytes(&normalized);
    #[cfg(test)]
    if normalized_sha256 == TEST_POPULOUS_HUCARD_SHA256 {
        return Ok(PceTasHardwareProfile {
            board: PceHuCardBoard::Populous,
            topology: PceHardwareTopology::Base,
            controller_mode: PceControllerMode::TwoButton,
        });
    }
    let descriptor = PceCartridgeDescriptor::from_sha256(normalized_sha256);
    let board = descriptor.hucard_board(normalized.len());
    #[cfg(test)]
    let hardware = if normalized_sha256 == TEST_SUPERGRAFX_HUCARD_SHA256 {
        PceCartridgeHardware::SuperGrafx
    } else {
        descriptor.required_hardware()
    };
    #[cfg(not(test))]
    let hardware = descriptor.required_hardware();
    let topology = match hardware {
        PceCartridgeHardware::Base => PceHardwareTopology::Base,
        PceCartridgeHardware::SuperGrafx => PceHardwareTopology::SuperGrafx,
    };
    ensure!(
        descriptor.console_wiring() == PceConsoleWiring::PcEngine
            && matches!(
                board,
                PceHuCardBoard::Plain | PceHuCardBoard::Sf2Ce | PceHuCardBoard::Populous
            )
            && (topology == PceHardwareTopology::Base
                || (topology == PceHardwareTopology::SuperGrafx && board == PceHuCardBoard::Plain)),
        "direct PC Engine TAS requires a supported PC Engine HuCard profile"
    );
    Ok(PceTasHardwareProfile {
        board,
        topology,
        controller_mode: PceControllerMode::TwoButton,
    })
}

fn read_direct_pce_hucard(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect TAS source media {}", path.display()))?;
    ensure!(
        (1..=MAX_DIRECT_PCE_HUCARD_BYTES).contains(&metadata.len()),
        "direct PC Engine TAS media has an unsupported size"
    );
    let expected_len = usize::try_from(metadata.len()).context("PC Engine media is too large")?;
    read_bounded_direct_rom(
        path,
        expected_len,
        "direct PC Engine TAS media changed while it was read",
    )
}

#[cfg(test)]
mod tests;
