use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;

use super::media::read_bounded_direct_rom;
use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, TasDigest, TasEditorExecutionEngine,
    TasEditorExecutionProvider, TasExecutionSession, TasInitialBranch, TasProject,
    direct_coleco_tas_identity, has_extension, publish_new_project,
    validate_direct_coleco_tas_branch_scope, validate_direct_coleco_tas_runtime,
    validate_direct_coleco_tas_state,
};

const MAX_COLECO_ZIP_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct DirectColecoTasExecutionLoader {
    pub(super) source_path: PathBuf,
    pub(super) rom_path: Option<PathBuf>,
    firmware_search_dirs: Vec<PathBuf>,
    #[cfg(test)]
    coleco_bios_override: Option<&'static [u8]>,
}

impl DirectColecoTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf, firmware_search_dirs: Vec<PathBuf>) -> Self {
        Self {
            source_path,
            rom_path: None,
            firmware_search_dirs,
            #[cfg(test)]
            coleco_bios_override: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_bios_override(
        source_path: PathBuf,
        firmware_search_dirs: Vec<PathBuf>,
        coleco_bios_override: &'static [u8],
    ) -> Self {
        Self {
            source_path,
            rom_path: None,
            firmware_search_dirs,
            coleco_bios_override: Some(coleco_bios_override),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_zip_with_bios_override(
        source_path: PathBuf,
        rom_path: Option<PathBuf>,
        coleco_bios_override: &'static [u8],
    ) -> Self {
        Self {
            source_path,
            rom_path,
            firmware_search_dirs: Vec::new(),
            coleco_bios_override: Some(coleco_bios_override),
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
            #[cfg(test)]
            coleco_bios_override: None,
        }
    }

    pub(crate) fn new_zip_for_project(
        source_path: PathBuf,
        firmware_search_dirs: Vec<PathBuf>,
        project: &TasProject,
    ) -> Result<Self> {
        super::direct_coleco::validate_direct_coleco_tas_project_identity(project)?;
        let inspection = crate::rom_archive::inspect_bounded_zip_members(
            &source_path,
            "col",
            MAX_COLECO_ZIP_BYTES,
            zeff_coleco_core::constants::MAX_CARTRIDGE_SIZE as u64,
        )?;
        ensure!(
            TasDigest(inspection.archive_sha256) == project.identity().source_media_sha256,
            "ColecoVision ZIP archive does not match the TAS project"
        );
        let matches = inspection
            .entries
            .into_iter()
            .filter(|entry| {
                super::direct_coleco::zip_coleco_tas_sync_config_sha256(&entry.member_name)
                    == project.identity().sync_config_sha256
            })
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "ColecoVision ZIP member does not match the TAS project"
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
        validate_direct_coleco_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, media) = self.load_fresh_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = self.identity(&backend, media, &start_state)?;
        TasProject::new(
            format!("coleco-{}", identity.source_media_sha256.to_hex()),
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
        ensure!(
            backend.encode_state_bytes()?.as_slice() == start_state,
            "ColecoVision TAS starting state does not match the fresh direct-ROM baseline"
        );
        let projection = validate_direct_coleco_tas_state(&mut backend, start_state)?;
        ensure!(
            projection.frame_count == 0 && projection.framebuffer.as_ref() == backend.framebuffer(),
            "ColecoVision TAS starting state does not restore the fresh baseline frame"
        );
        let identity = self.identity(&backend, media, start_state)?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn load_editor_engine(
        &self,
        project: &TasProject,
    ) -> Result<TasEditorExecutionEngine> {
        for branch in project.branches() {
            validate_direct_coleco_tas_branch_scope(project, branch.id())?;
        }
        let session = self.load_session(project.start_state())?;
        TasEditorExecutionEngine::attach(project, session, validate_direct_coleco_tas_branch_scope)
    }

    pub(crate) fn load_fresh_backend(&self) -> Result<(EmuBackend, ColecoTasMediaIdentity)> {
        let config = BackendLoadConfig {
            sample_rate: None,
            apply_mods: false,
            initial_input: None,
            firmware_search_dirs: self.firmware_search_dirs.clone(),
            #[cfg(test)]
            coleco_bios_override: self.coleco_bios_override,
            ..BackendLoadConfig::default()
        };
        let (backend, media) = if has_extension(&self.source_path, "col") {
            let source_bytes = read_direct_coleco_rom(&self.source_path)?;
            let media = ColecoTasMediaIdentity {
                source_media_sha256: TasDigest::from_bytes(&source_bytes),
                sync_config_sha256: super::direct_coleco_tas_sync_config_sha256(),
            };
            let backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
                ActiveSystem::Coleco,
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
                "col",
                MAX_COLECO_ZIP_BYTES,
                zeff_coleco_core::constants::MAX_CARTRIDGE_SIZE as u64,
            )?;
            ensure!(
                matches!(selected.bytes.get(..2), Some([0xAA, 0x55] | [0x55, 0xAA])),
                "ColecoVision ZIP member has an invalid standard-cartridge header"
            );
            let media = ColecoTasMediaIdentity {
                source_media_sha256: TasDigest(selected.archive_sha256),
                sync_config_sha256: super::direct_coleco::zip_coleco_tas_sync_config_sha256(
                    &selected.member_name,
                ),
            };
            let backend = super::load_backend_from_rom_source(
                ActiveSystem::Coleco,
                &self.source_path,
                &selected.rom_path,
                Some(selected.bytes),
                config,
            )?
            .backend;
            (backend, media)
        } else {
            anyhow::bail!(
                "ColecoVision TAS execution requires a direct .col file or selected ZIP member"
            );
        };
        validate_direct_coleco_tas_runtime(&backend, false)?;
        Ok((backend, media))
    }

    fn identity(
        &self,
        backend: &EmuBackend,
        media: ColecoTasMediaIdentity,
        start_state: &[u8],
    ) -> Result<crate::tas_project::TasProjectIdentity> {
        if has_extension(&self.source_path, "col") {
            let source_bytes = read_direct_coleco_rom(&self.source_path)?;
            ensure!(
                media.source_media_sha256 == TasDigest::from_bytes(&source_bytes),
                "ColecoVision source changed while constructing TAS identity"
            );
            return direct_coleco_tas_identity(backend, &source_bytes, start_state);
        }
        let selected = crate::rom_archive::extract_bounded_zip_member(
            &self.source_path,
            self.rom_path.as_deref(),
            "col",
            MAX_COLECO_ZIP_BYTES,
            zeff_coleco_core::constants::MAX_CARTRIDGE_SIZE as u64,
        )?;
        ensure!(
            media.source_media_sha256 == TasDigest(selected.archive_sha256)
                && media.sync_config_sha256
                    == super::direct_coleco::zip_coleco_tas_sync_config_sha256(
                        &selected.member_name,
                    )
                && TasDigest::from_bytes(&selected.bytes) == TasDigest(backend.rom_hash()),
            "ColecoVision ZIP changed while constructing TAS identity"
        );
        super::direct_coleco::zip_coleco_tas_identity(
            backend,
            selected.archive_sha256,
            &selected.member_name,
            start_state,
        )
    }
}

pub(crate) struct ColecoTasMediaIdentity {
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
}

impl TasEditorExecutionProvider for DirectColecoTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectColecoTasExecutionLoader::load_editor_engine(self, project)
    }
}

fn read_direct_coleco_rom(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect TAS source media {}", path.display()))?;
    ensure!(
        (2..=zeff_coleco_core::constants::MAX_CARTRIDGE_SIZE as u64).contains(&metadata.len()),
        "direct ColecoVision TAS media has an unsupported size"
    );
    let expected_len =
        usize::try_from(metadata.len()).context("ColecoVision media is too large")?;
    let bytes = read_bounded_direct_rom(
        path,
        expected_len,
        "direct ColecoVision TAS media changed while it was read",
    )?;
    ensure!(
        matches!(bytes.get(..2), Some([0xAA, 0x55] | [0x55, 0xAA])),
        "direct ColecoVision TAS media has an invalid standard-cartridge header"
    );
    Ok(bytes)
}
