use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;
use zeff_md_core::{Region, TimingModel};

use super::direct_md;
use super::media::read_bounded_direct_rom;
use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, TasExecutionSession, TasInitialBranch, TasProject,
};
use crate::emu_backend::loader::md_tas::{
    MAX_MD_TAS_ROM_BYTES, MAX_MD_TAS_ZIP_BYTES, MdTasProfile, extension,
};
use crate::tas_project::{TasDigest, TasEditorExecutionEngine, TasEditorExecutionProvider};

#[derive(Clone, Debug)]
pub(crate) struct DirectMdTasExecutionLoader {
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
    profile: MdTasProfile,
}

impl DirectMdTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf, profile: MdTasProfile) -> Self {
        Self {
            source_path,
            rom_path: None,
            profile,
        }
    }

    pub(crate) fn new_zip(source_path: PathBuf, rom_path: PathBuf, profile: MdTasProfile) -> Self {
        Self {
            source_path,
            rom_path: Some(rom_path),
            profile,
        }
    }

    pub(crate) fn new_zip_for_project(source_path: PathBuf, project: &TasProject) -> Result<Self> {
        let profile = direct_md::project_profile(project)?;
        let mut matches = Vec::new();
        for extension in ["md", "gen", "bin"] {
            let inspection = crate::rom_archive::inspect_bounded_zip_members(
                &source_path,
                extension,
                MAX_MD_TAS_ZIP_BYTES,
                MAX_MD_TAS_ROM_BYTES,
            )?;
            ensure!(
                TasDigest(inspection.archive_sha256) == project.identity().source_media_sha256,
                "MD ZIP archive differs from the project"
            );
            matches.extend(inspection.entries.into_iter().filter(|entry| {
                TasDigest(profile.sync_hash(Some(&entry.member_name)))
                    == project.identity().sync_config_sha256
            }));
        }
        ensure!(
            matches.len() == 1,
            "MD ZIP selected member differs from the project"
        );
        Ok(Self::new_zip(
            source_path,
            matches.remove(0).rom_path,
            profile,
        ))
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let backend = self.load_fresh_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = direct_md::identity(&backend, &start_state)?;
        TasProject::new(
            format!("md-{}", identity.source_media_sha256.to_hex()),
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
            "MD TAS project requires .ztas"
        );
        let project = self.create_project()?;
        super::publish_new_project(path, &project)?;
        Ok(project)
    }

    pub(crate) fn load_session(&self, start_state: &[u8]) -> Result<TasExecutionSession> {
        let backend = self.load_fresh_backend()?;
        let identity = direct_md::identity(&backend, start_state)?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn validate_project(&self, project: &TasProject) -> Result<()> {
        ensure!(
            direct_md::project_profile(project)? == self.profile,
            "MD TAS pad profile differs"
        );
        for branch in project.branches() {
            direct_md::validate_branch_scope(project, branch.id())?;
        }
        let session = self.load_session(project.start_state())?;
        ensure!(
            session.identity() == project.identity(),
            "MD TAS loaded media or profile differs"
        );
        Ok(())
    }

    pub(crate) fn load_editor_engine(
        &self,
        project: &TasProject,
    ) -> Result<TasEditorExecutionEngine> {
        self.validate_project(project)?;
        TasEditorExecutionEngine::attach_md(
            project,
            self.load_session(project.start_state())?,
            direct_md::validate_branch_scope,
        )
    }

    pub(crate) fn load_fresh_backend(&self) -> Result<EmuBackend> {
        let config = BackendLoadConfig {
            apply_mods: false,
            initial_input: None,
            sample_rate: Some(48_000),
            md_region: Some(Region::NtscUs),
            md_timing: TimingModel::Standard,
            md_controller: self.profile.kind(),
            md_load_save_ram: false,
            md_host_persistence: false,
            md_use_tmss: false,
            md_svp_functional_profile: None,
            ..Default::default()
        };
        let backend = if extension(&self.source_path).is_some() {
            ensure!(
                self.rom_path.is_none(),
                "direct MD source cannot select an archive member"
            );
            let len = std::fs::metadata(&self.source_path)
                .with_context(|| {
                    format!("failed to inspect MD source {}", self.source_path.display())
                })?
                .len();
            ensure!(
                (8..=MAX_MD_TAS_ROM_BYTES).contains(&len),
                "unsupported factory MD source size"
            );
            let bytes = read_bounded_direct_rom(
                &self.source_path,
                usize::try_from(len)?,
                "MD source changed while reading",
            )?;
            super::super::load_backend_from_bounded_direct_source(
                ActiveSystem::Md,
                &self.source_path,
                bytes,
                config,
            )?
            .backend
        } else {
            ensure!(
                super::has_extension(&self.source_path, "zip"),
                "MD TAS requires direct or selected ZIP media"
            );
            let member = self
                .rom_path
                .as_deref()
                .context("MD TAS requires an explicit ZIP member")?;
            let extension =
                extension(member).context("unsupported MD selected member extension")?;
            let selected = crate::rom_archive::extract_bounded_zip_member(
                &self.source_path,
                Some(member),
                extension,
                MAX_MD_TAS_ZIP_BYTES,
                MAX_MD_TAS_ROM_BYTES,
            )?;
            super::load_backend_from_rom_source(
                ActiveSystem::Md,
                &self.source_path,
                &selected.rom_path,
                Some(selected.bytes),
                config,
            )?
            .backend
        };
        let EmuBackend::Md(md) = &backend else {
            anyhow::bail!("MD normal loader returned another system")
        };
        let load = md.tas_load()?;
        ensure!(
            load.profile() == self.profile,
            "MD normal loader changed pad profile"
        );
        load.validate_fresh(md)?;
        Ok(backend)
    }
}

impl TasEditorExecutionProvider for DirectMdTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectMdTasExecutionLoader::load_editor_engine(self, project)
    }
}

#[cfg(test)]
mod editor_tests;
#[cfg(test)]
mod tests;
