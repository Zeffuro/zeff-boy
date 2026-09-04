use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;

use super::media::read_bounded_direct_rom;
use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, TasDigest, TasEditorExecutionEngine,
    TasEditorExecutionProvider, TasExecutionSession, TasInitialBranch, TasProject, has_extension,
    publish_new_project,
};
use crate::emu_backend::gba::{
    DIRECT_GBA_SAMPLE_RATE, MAX_DIRECT_GBA_ROM_BYTES, direct_gba_tas_identity,
    is_gba_tilt_tas_identity, validate_direct_gba_tas_branch_scope,
    validate_direct_gba_tas_private_runtime, validate_direct_gba_tas_state,
    zip_gba_battery_tas_sync_config_sha256, zip_gba_tas_identity, zip_gba_tas_sync_config_sha256,
    zip_gba_tilt_tas_sync_config_sha256,
};

const MAX_GBA_ZIP_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct DirectGbaTasExecutionLoader {
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
}

pub(crate) struct GbaTasMediaIdentity {
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    zip_member_name: Option<String>,
}

impl DirectGbaTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf) -> Self {
        Self {
            source_path,
            rom_path: None,
        }
    }

    pub(crate) fn new_zip(source_path: PathBuf, rom_path: Option<PathBuf>) -> Self {
        Self {
            source_path,
            rom_path,
        }
    }

    pub(crate) fn new_zip_for_project(source_path: PathBuf, project: &TasProject) -> Result<Self> {
        crate::emu_backend::gba::validate_direct_gba_tas_project_identity(project)?;
        let inspection = crate::rom_archive::inspect_bounded_zip_members(
            &source_path,
            "gba",
            MAX_GBA_ZIP_BYTES,
            MAX_DIRECT_GBA_ROM_BYTES,
        )?;
        ensure!(
            TasDigest(inspection.archive_sha256) == project.identity().source_media_sha256,
            "GBA ZIP archive does not match the TAS project"
        );
        let expected_sync = match project.identity().persistent_state {
            crate::tas_project::TasExternalIdentity::Absent => zip_gba_tas_sync_config_sha256,
            crate::tas_project::TasExternalIdentity::ExternalSha256(_) => {
                zip_gba_battery_tas_sync_config_sha256
            }
        };
        let tilt = is_gba_tilt_tas_identity(project.identity());
        let rtc = project.identity().rtc_state != crate::tas_project::TasExternalIdentity::Absent;
        let matches = inspection
            .entries
            .into_iter()
            .filter(|entry| {
                if tilt {
                    zip_gba_tilt_tas_sync_config_sha256(&entry.member_name)
                        == project.identity().sync_config_sha256
                } else if rtc {
                    crate::emu_backend::gba::supported_gba_rtc_backup_kinds()
                        .into_iter()
                        .filter(|kind| {
                            (*kind == zeff_gba_core::hardware::cartridge::BackupKind::None)
                                == (project.identity().persistent_state
                                    == crate::tas_project::TasExternalIdentity::Absent)
                        })
                        .any(|kind| {
                            crate::emu_backend::gba::zip_gba_rtc_tas_sync_config_sha256(
                                &entry.member_name,
                                kind,
                            ) == project.identity().sync_config_sha256
                        })
                } else {
                    expected_sync(&entry.member_name) == project.identity().sync_config_sha256
                }
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "GBA ZIP member does not match the TAS project"
        );
        Ok(Self::new_zip(
            source_path,
            Some(matches[0].rom_path.clone()),
        ))
    }

    pub(crate) fn validate_project_branch_scope(
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        validate_direct_gba_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, media) = self.load_creation_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = self.identity(&backend, media, &start_state)?;
        TasProject::new(
            format!("gba-{}", identity.source_media_sha256.to_hex()),
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

    pub(crate) fn load_session(&self, start_state: &[u8]) -> Result<TasExecutionSession> {
        let (mut backend, media) = self.load_fresh_backend()?;
        if backend.save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None {
            ensure!(
                backend.encode_state_bytes()?.as_slice() == start_state,
                "GBA TAS starting state does not match the fresh direct-ROM baseline"
            );
        }
        let projection = validate_direct_gba_tas_state(&mut backend, start_state)?;
        ensure!(
            projection.frame_count == 0 && projection.framebuffer.as_ref() == backend.framebuffer(),
            "GBA TAS starting state does not restore the fresh baseline frame"
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
        TasEditorExecutionEngine::attach(project, session, validate_direct_gba_tas_branch_scope)
    }

    fn load_creation_backend(&self) -> Result<(EmuBackend, GbaTasMediaIdentity)> {
        self.load_backend(true)
    }

    pub(crate) fn load_fresh_backend(&self) -> Result<(EmuBackend, GbaTasMediaIdentity)> {
        self.load_backend(false)
    }

    fn load_backend(&self, load_battery_sram: bool) -> Result<(EmuBackend, GbaTasMediaIdentity)> {
        let config = BackendLoadConfig {
            sample_rate: Some(DIRECT_GBA_SAMPLE_RATE),
            apply_mods: false,
            initial_input: Some((0, 0)),
            gba_load_battery_sram: load_battery_sram,
            gba_seed_rtc_from_host: false,
            gba_use_external_bios: false,
            ..BackendLoadConfig::default()
        };
        let (mut backend, mut media) = if has_extension(&self.source_path, "gba") {
            let source_bytes = read_direct_gba_rom(&self.source_path)?;
            let media = GbaTasMediaIdentity {
                source_media_sha256: TasDigest::from_bytes(&source_bytes),
                sync_config_sha256: crate::emu_backend::gba::direct_gba_tas_sync_config_sha256(),
                zip_member_name: None,
            };
            let backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
                ActiveSystem::GameBoyAdvance,
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
                "gba",
                MAX_GBA_ZIP_BYTES,
                MAX_DIRECT_GBA_ROM_BYTES,
            )?;
            ensure!(selected.bytes.len() >= 0xC0, "GBA ZIP member is too small");
            let media = GbaTasMediaIdentity {
                source_media_sha256: TasDigest(selected.archive_sha256),
                sync_config_sha256: zip_gba_tas_sync_config_sha256(&selected.member_name),
                zip_member_name: Some(selected.member_name),
            };
            let backend = super::load_backend_from_rom_source(
                ActiveSystem::GameBoyAdvance,
                &self.source_path,
                &selected.rom_path,
                Some(selected.bytes),
                config,
            )?
            .backend;
            (backend, media)
        } else {
            anyhow::bail!("GBA TAS execution requires a direct .gba file or selected ZIP member");
        };
        validate_direct_gba_tas_private_runtime(&backend, false)?;
        media.sync_config_sha256 = crate::emu_backend::gba::gba_tas_sync_config(
            &backend,
            media.zip_member_name.as_deref(),
        )?;
        let EmuBackend::Gba(gba) = &mut backend else {
            unreachable!("direct GBA loader must produce a GBA backend");
        };
        gba.set_tas_sync_config_sha256(media.sync_config_sha256.0);
        Ok((backend, media))
    }

    fn identity(
        &self,
        backend: &EmuBackend,
        media: GbaTasMediaIdentity,
        start_state: &[u8],
    ) -> Result<crate::tas_project::TasProjectIdentity> {
        if has_extension(&self.source_path, "gba") {
            let source_bytes = read_direct_gba_rom(&self.source_path)?;
            ensure!(
                media.source_media_sha256 == TasDigest::from_bytes(&source_bytes),
                "GBA source changed while constructing TAS identity"
            );
            return direct_gba_tas_identity(backend, &source_bytes, start_state);
        }
        let selected = crate::rom_archive::extract_bounded_zip_member(
            &self.source_path,
            self.rom_path.as_deref(),
            "gba",
            MAX_GBA_ZIP_BYTES,
            MAX_DIRECT_GBA_ROM_BYTES,
        )?;
        ensure!(
            media.source_media_sha256 == TasDigest(selected.archive_sha256)
                && media.sync_config_sha256
                    == crate::emu_backend::gba::gba_tas_sync_config(
                        backend,
                        Some(&selected.member_name),
                    )?
                && TasDigest::from_bytes(&selected.bytes) == TasDigest(backend.rom_hash()),
            "GBA ZIP changed while constructing TAS identity"
        );
        zip_gba_tas_identity(
            backend,
            selected.archive_sha256,
            &selected.member_name,
            start_state,
        )
    }
}

impl TasEditorExecutionProvider for DirectGbaTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectGbaTasExecutionLoader::load_editor_engine(self, project)
    }
}

#[cfg(test)]
mod tests;

fn read_direct_gba_rom(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect TAS source media {}", path.display()))?;
    ensure!(
        (0xC0..=MAX_DIRECT_GBA_ROM_BYTES).contains(&metadata.len()),
        "direct GBA TAS media has an unsupported size"
    );
    let expected_len = usize::try_from(metadata.len()).context("GBA media is too large")?;
    read_bounded_direct_rom(
        path,
        expected_len,
        "direct GBA TAS media changed while it was read",
    )
}
