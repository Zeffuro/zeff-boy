use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;

use super::media::reject_embedded_zip_sram;
use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, MAX_NES_CARTRIDGE_BYTES, MAX_NES_ZIP_BYTES,
    TasDigest, TasEditorExecutionEngine, TasEditorExecutionProvider, TasExecutionSession,
    TasExternalIdentity, TasInitialBranch, TasProject, direct_nes_battery_tas_sync_config_sha256,
    direct_nes_tas_identity, direct_nes_tas_sync_config_sha256, publish_new_project,
    read_nes_cartridge_bounded, validate_current_nes_start_state,
    validate_direct_nes_tas_branch_scope, zip_nes_battery_tas_sync_config_sha256,
    zip_nes_tas_identity, zip_nes_tas_sync_config_sha256,
};

#[derive(Clone, Debug)]
pub(crate) struct DirectNesTasExecutionLoader {
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
    firmware_search_dirs: Vec<PathBuf>,
}

pub(crate) struct NesTasMediaIdentity {
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
}

impl DirectNesTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf, firmware_search_dirs: Vec<PathBuf>) -> Self {
        Self {
            source_path,
            rom_path: None,
            firmware_search_dirs,
        }
    }

    pub(crate) fn new_zip(
        source_path: PathBuf,
        rom_path: Option<PathBuf>,
        firmware_search_dirs: Vec<PathBuf>,
    ) -> Self {
        Self {
            source_path,
            rom_path,
            firmware_search_dirs,
        }
    }

    pub(crate) fn new_zip_for_project(
        source_path: PathBuf,
        firmware_search_dirs: Vec<PathBuf>,
        project: &TasProject,
    ) -> Result<Self> {
        super::validate_direct_nes_tas_project_identity(project)?;
        let inspection = crate::rom_archive::inspect_bounded_zip_members(
            &source_path,
            "nes",
            MAX_NES_ZIP_BYTES,
            MAX_NES_CARTRIDGE_BYTES,
        )?;
        ensure!(
            TasDigest(inspection.archive_sha256) == project.identity().source_media_sha256,
            "ZIP archive does not match the TAS project"
        );
        let expected_sync = match project.identity().persistent_state {
            TasExternalIdentity::Absent => zip_nes_tas_sync_config_sha256,
            TasExternalIdentity::ExternalSha256(_) => zip_nes_battery_tas_sync_config_sha256,
        };
        let matches = inspection
            .entries
            .into_iter()
            .filter(|entry| {
                expected_sync(&entry.member_name) == project.identity().sync_config_sha256
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "ZIP member does not match the TAS project"
        );
        Ok(Self::new_zip(
            source_path,
            Some(matches[0].rom_path.clone()),
            firmware_search_dirs,
        ))
    }

    pub(crate) fn validate_project_branch_scope(
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        validate_direct_nes_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn load_session(&self, start_state: &[u8]) -> Result<TasExecutionSession> {
        validate_current_nes_start_state(start_state)?;
        let (mut backend, media) = self.load_fresh_backend()?;
        backend
            .load_state_from_bytes(start_state.to_vec())
            .context("failed to restore TAS starting state for device-profile validation")?;
        let identity = self.identity(&backend, media, start_state)?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, media) = self.load_creation_backend()?;
        let start_state = backend.encode_state_bytes()?;
        validate_current_nes_start_state(&start_state)?;
        let identity = self.identity(&backend, media, &start_state)?;
        TasProject::new(
            format!("nes-{}", identity.source_media_sha256.to_hex()),
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
        publish_new_project(path, &project)?;
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

    pub(crate) fn load_editor_engine(
        &self,
        project: &TasProject,
    ) -> Result<TasEditorExecutionEngine> {
        for branch in project.branches() {
            validate_direct_nes_tas_branch_scope(project, branch.id()).with_context(|| {
                format!(
                    "TAS branch {:?} is outside the direct NES editor execution profile",
                    branch.id()
                )
            })?;
        }
        let session = self.load_session(project.start_state())?;
        TasEditorExecutionEngine::attach(project, session, validate_direct_nes_tas_branch_scope)
    }

    pub(crate) fn load_fresh_backend(&self) -> Result<(EmuBackend, NesTasMediaIdentity)> {
        self.load_backend(false)
    }

    fn load_creation_backend(&self) -> Result<(EmuBackend, NesTasMediaIdentity)> {
        self.load_backend(true)
    }

    fn load_backend(&self, load_battery_sram: bool) -> Result<(EmuBackend, NesTasMediaIdentity)> {
        let config = BackendLoadConfig {
            firmware_search_dirs: self.firmware_search_dirs.clone(),
            sample_rate: None,
            apply_mods: false,
            initial_input: None,
            nes_load_battery_sram: load_battery_sram,
            ..BackendLoadConfig::default()
        };
        let (backend, mut media) = if has_extension(&self.source_path, "nes") {
            let source_bytes = read_nes_cartridge_bounded(&self.source_path)?;
            let media = NesTasMediaIdentity {
                source_media_sha256: TasDigest::from_bytes(&source_bytes),
                sync_config_sha256: direct_nes_tas_sync_config_sha256(),
            };
            let backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
                ActiveSystem::Nes,
                &self.source_path,
                source_bytes,
                config,
            )?
            .backend;
            (backend, media)
        } else if has_extension(&self.source_path, "zip") {
            let selected = crate::rom_archive::extract_bounded_zip_member(
                &self.source_path,
                self.rom_path.as_deref(),
                "nes",
                MAX_NES_ZIP_BYTES,
                MAX_NES_CARTRIDGE_BYTES,
            )?;
            let media = NesTasMediaIdentity {
                source_media_sha256: TasDigest(selected.archive_sha256),
                sync_config_sha256: zip_nes_tas_sync_config_sha256(&selected.member_name),
            };
            let backend = super::load_backend_from_rom_source(
                ActiveSystem::Nes,
                &self.source_path,
                &selected.rom_path,
                Some(selected.bytes),
                config,
            )?
            .backend;
            (backend, media)
        } else {
            anyhow::bail!("NES TAS execution requires a direct .nes file or selected ZIP member");
        };
        ensure!(
            backend.nes_has_standard_controller_topology() == Some(true),
            "TAS creation requires the standard NES controller topology"
        );
        if backend.save_ram_kind().is_battery_backed() {
            if has_extension(&self.source_path, "zip") {
                reject_embedded_zip_sram(
                    &self.source_path,
                    MAX_NES_ZIP_BYTES,
                    MAX_NES_CARTRIDGE_BYTES,
                    "NES ZIP TAS execution does not import embedded SRAM; use an adjacent .sav file",
                )?;
                let selected = crate::rom_archive::extract_bounded_zip_member(
                    &self.source_path,
                    self.rom_path.as_deref(),
                    "nes",
                    MAX_NES_ZIP_BYTES,
                    MAX_NES_CARTRIDGE_BYTES,
                )?;
                media.sync_config_sha256 =
                    zip_nes_battery_tas_sync_config_sha256(&selected.member_name);
            } else {
                media.sync_config_sha256 = direct_nes_battery_tas_sync_config_sha256();
            }
        }
        Ok((backend, media))
    }

    fn identity(
        &self,
        backend: &EmuBackend,
        media: NesTasMediaIdentity,
        start_state: &[u8],
    ) -> Result<super::TasProjectIdentity> {
        if has_extension(&self.source_path, "nes") {
            let source_bytes = read_nes_cartridge_bounded(&self.source_path)?;
            ensure!(
                media.source_media_sha256 == TasDigest::from_bytes(&source_bytes),
                "NES source changed while constructing TAS identity"
            );
            return direct_nes_tas_identity(backend, &source_bytes, start_state);
        }
        let selected = crate::rom_archive::extract_bounded_zip_member(
            &self.source_path,
            self.rom_path.as_deref(),
            "nes",
            MAX_NES_ZIP_BYTES,
            MAX_NES_CARTRIDGE_BYTES,
        )?;
        ensure!(
            media.source_media_sha256 == TasDigest(selected.archive_sha256)
                && media.sync_config_sha256
                    == if backend.save_ram_kind().is_battery_backed() {
                        zip_nes_battery_tas_sync_config_sha256(&selected.member_name)
                    } else {
                        zip_nes_tas_sync_config_sha256(&selected.member_name)
                    },
            "ZIP changed while constructing TAS identity"
        );
        ensure!(
            TasDigest::from_bytes(&selected.bytes) == TasDigest(backend.rom_hash()),
            "selected ZIP member changed while constructing TAS identity"
        );
        zip_nes_tas_identity(
            backend,
            selected.archive_sha256,
            &selected.member_name,
            start_state,
        )
    }
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|candidate| candidate.to_str())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
}

impl TasEditorExecutionProvider for DirectNesTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectNesTasExecutionLoader::load_editor_engine(self, project)
    }
}

#[cfg(test)]
mod tests;
