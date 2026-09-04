use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;

use super::media::read_bounded_direct_rom;
use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, TasDigest, TasEditorExecutionEngine,
    TasEditorExecutionProvider, TasExecutionSession, TasInitialBranch, TasProject,
    direct_sg1000::direct_sg1000_tas_identity, has_extension, validate_direct_sg1000_tas_runtime,
    validate_direct_sg1000_tas_state,
};

pub(crate) const MAX_DIRECT_SG1000_ROM_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SG1000_ZIP_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct DirectSg1000TasExecutionLoader {
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
}

pub(crate) struct Sg1000TasMediaIdentity {
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
}

impl DirectSg1000TasExecutionLoader {
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
        super::direct_sg1000::validate_direct_sg1000_tas_project_identity(project)?;
        let mut matches = Vec::new();
        for extension in ["sg", "sc"] {
            let inspection = crate::rom_archive::inspect_bounded_zip_members(
                &source_path,
                extension,
                MAX_SG1000_ZIP_BYTES,
                MAX_DIRECT_SG1000_ROM_BYTES,
            )?;
            ensure!(
                TasDigest(inspection.archive_sha256) == project.identity().source_media_sha256,
                "SG-1000 ZIP archive does not match the TAS project"
            );
            matches.extend(inspection.entries.into_iter().filter(|entry| {
                super::direct_sg1000::zip_sg1000_tas_sync_config_sha256(&entry.member_name)
                    == project.identity().sync_config_sha256
            }));
        }
        ensure!(
            matches.len() == 1,
            "SG-1000 ZIP member does not match the TAS project"
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
        super::direct_sg1000::validate_direct_sg1000_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, media) = self.load_fresh_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = self.identity(&backend, media, &start_state)?;
        TasProject::new(
            format!("sg1000-{}", identity.source_media_sha256.to_hex()),
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
        ensure!(
            backend.encode_state_bytes()?.as_slice() == start_state,
            "SG-1000 TAS starting state does not match the fresh direct-ROM baseline"
        );
        let projection = validate_direct_sg1000_tas_state(&mut backend, start_state)?;
        ensure!(
            projection.frame_count == 0 && projection.framebuffer.as_ref() == backend.framebuffer(),
            "SG-1000 TAS starting state does not restore the fresh baseline frame"
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
            super::direct_sg1000::validate_direct_sg1000_tas_branch_scope,
        )
    }

    pub(crate) fn load_fresh_backend(&self) -> Result<(EmuBackend, Sg1000TasMediaIdentity)> {
        let config = BackendLoadConfig {
            sample_rate: None,
            apply_mods: false,
            initial_input: None,
            sega8_video_standard: Some(zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc),
            sega8_console_region: Some(zeff_sega8_core::hardware::region::Sega8Region::Japanese),
            sega8_use_external_boot_rom: false,
            ..BackendLoadConfig::default()
        };
        let (backend, media) =
            if has_extension(&self.source_path, "sg") || has_extension(&self.source_path, "sc") {
                let bytes = read_direct_sg1000_rom(&self.source_path)?;
                let media = Sg1000TasMediaIdentity {
                    source_media_sha256: TasDigest::from_bytes(&bytes),
                    sync_config_sha256: super::direct_sg1000_tas_sync_config_sha256(),
                };
                let backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
                    ActiveSystem::Sg1000,
                    &self.source_path,
                    bytes,
                    config,
                )?
                .backend;
                (backend, media)
            } else if has_extension(&self.source_path, "zip") {
                let selected = self.select_zip_member()?;
                let media = Sg1000TasMediaIdentity {
                    source_media_sha256: TasDigest(selected.archive_sha256),
                    sync_config_sha256: super::direct_sg1000::zip_sg1000_tas_sync_config_sha256(
                        &selected.member_name,
                    ),
                };
                let backend = super::load_backend_from_rom_source(
                    ActiveSystem::Sg1000,
                    &self.source_path,
                    &selected.rom_path,
                    Some(selected.bytes),
                    config,
                )?
                .backend;
                (backend, media)
            } else {
                anyhow::bail!(
                    "SG-1000 TAS execution requires a direct .sg/.sc file or selected ZIP member"
                );
            };
        validate_direct_sg1000_tas_runtime(&backend, false)?;
        Ok((backend, media))
    }

    fn select_zip_member(&self) -> Result<crate::rom_archive::BoundedZipMember> {
        if let Some(path) = self.rom_path.as_deref() {
            let extension = path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            ensure!(
                extension.eq_ignore_ascii_case("sg") || extension.eq_ignore_ascii_case("sc"),
                "selected SG-1000 ZIP member must use .sg or .sc"
            );
            return crate::rom_archive::extract_bounded_zip_member(
                &self.source_path,
                Some(path),
                extension,
                MAX_SG1000_ZIP_BYTES,
                MAX_DIRECT_SG1000_ROM_BYTES,
            );
        }
        let mut entries = Vec::new();
        for extension in ["sg", "sc"] {
            entries.extend(
                crate::rom_archive::inspect_bounded_zip_members(
                    &self.source_path,
                    extension,
                    MAX_SG1000_ZIP_BYTES,
                    MAX_DIRECT_SG1000_ROM_BYTES,
                )?
                .entries,
            );
        }
        ensure!(
            entries.len() == 1,
            "SG-1000 ZIP must contain exactly one .sg or .sc member unless one is selected explicitly"
        );
        let entry = &entries[0];
        let extension = entry
            .rom_path
            .extension()
            .and_then(|value| value.to_str())
            .context("SG-1000 ZIP member omitted its extension")?;
        crate::rom_archive::extract_bounded_zip_member(
            &self.source_path,
            Some(&entry.rom_path),
            extension,
            MAX_SG1000_ZIP_BYTES,
            MAX_DIRECT_SG1000_ROM_BYTES,
        )
    }

    fn identity(
        &self,
        backend: &EmuBackend,
        media: Sg1000TasMediaIdentity,
        start_state: &[u8],
    ) -> Result<crate::tas_project::TasProjectIdentity> {
        if !has_extension(&self.source_path, "zip") {
            let bytes = read_direct_sg1000_rom(&self.source_path)?;
            ensure!(
                media.source_media_sha256 == TasDigest::from_bytes(&bytes),
                "SG-1000 source changed while constructing TAS identity"
            );
            return direct_sg1000_tas_identity(backend, &bytes, start_state);
        }
        let selected = self.select_zip_member()?;
        ensure!(
            media.source_media_sha256 == TasDigest(selected.archive_sha256)
                && media.sync_config_sha256
                    == super::direct_sg1000::zip_sg1000_tas_sync_config_sha256(
                        &selected.member_name
                    )
                && TasDigest::from_bytes(&selected.bytes) == TasDigest(backend.rom_hash()),
            "SG-1000 ZIP changed while constructing TAS identity"
        );
        super::direct_sg1000::zip_sg1000_tas_identity(
            backend,
            selected.archive_sha256,
            &selected.member_name,
            start_state,
        )
    }
}

impl TasEditorExecutionProvider for DirectSg1000TasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectSg1000TasExecutionLoader::load_editor_engine(self, project)
    }
}

fn read_direct_sg1000_rom(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect TAS source media {}", path.display()))?;
    ensure!(
        (1..=MAX_DIRECT_SG1000_ROM_BYTES).contains(&metadata.len()),
        "direct SG-1000 TAS media has an unsupported size"
    );
    let expected_len = usize::try_from(metadata.len()).context("SG-1000 media is too large")?;
    read_bounded_direct_rom(
        path,
        expected_len,
        "direct SG-1000 TAS media changed while it was read",
    )
}

#[cfg(test)]
mod tests;
