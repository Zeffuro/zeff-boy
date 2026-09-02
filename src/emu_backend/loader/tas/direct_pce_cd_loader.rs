use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;
use zeff_pce_core::hardware::{
    PceArcadeCardMode, PceCartridgeHardware, PceConsoleWiring, PceControllerMode, PceMemoryBaseMode,
};

use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, TasEditorExecutionEngine,
    TasEditorExecutionProvider, TasExecutionSession, TasInitialBranch, TasProject,
    direct_pce_cd::{
        direct_pce_cd_arcade_eligible, direct_pce_cd_arcade_tas_sync_config_sha256,
        direct_pce_cd_chd_arcade_tas_sync_config_sha256,
        direct_pce_cd_chd_memory_base_tas_sync_config_sha256, direct_pce_cd_chd_source_identity,
        direct_pce_cd_chd_tas_sync_config_sha256, direct_pce_cd_iso_arcade_tas_sync_config_sha256,
        direct_pce_cd_iso_memory_base_tas_sync_config_sha256, direct_pce_cd_iso_source_identity,
        direct_pce_cd_iso_tas_sync_config_sha256, direct_pce_cd_memory_base_eligible,
        direct_pce_cd_memory_base_tas_sync_config_sha256,
        direct_pce_cd_ppf_memory_base_tas_sync_config_sha256,
        direct_pce_cd_ppf_tas_sync_config_sha256, direct_pce_cd_tas_identity,
        direct_pce_cd_tas_sync_config_sha256, validate_direct_pce_cd_tas_branch_scope,
        validate_direct_pce_cd_tas_project_identity, validate_direct_pce_cd_tas_runtime,
        validate_direct_pce_cd_tas_state,
    },
    has_extension,
};

#[derive(Clone, Debug)]
pub(crate) struct DirectPceCdTasExecutionLoader {
    source_path: PathBuf,
    firmware_search_dirs: Vec<PathBuf>,
    #[cfg(test)]
    system_card_override: Option<&'static [u8]>,
    #[cfg(test)]
    system_card_sha256_override: Option<[u8; 32]>,
    #[cfg(test)]
    ppf_stack_override: Option<crate::emu_backend::pce_cd::PceCdTasPpfStack>,
}

impl DirectPceCdTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf, firmware_search_dirs: Vec<PathBuf>) -> Self {
        Self {
            source_path,
            firmware_search_dirs,
            #[cfg(test)]
            system_card_override: None,
            #[cfg(test)]
            system_card_sha256_override: None,
            #[cfg(test)]
            ppf_stack_override: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_system_card_override(
        source_path: PathBuf,
        system_card: &'static [u8],
        sha256: [u8; 32],
    ) -> Self {
        Self {
            source_path,
            firmware_search_dirs: Vec::new(),
            system_card_override: Some(system_card),
            system_card_sha256_override: Some(sha256),
            ppf_stack_override: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_with_system_card_and_ppf_stack(
        source_path: PathBuf,
        system_card: &'static [u8],
        sha256: [u8; 32],
        ppf_stack: crate::emu_backend::pce_cd::PceCdTasPpfStack,
    ) -> Self {
        Self {
            source_path,
            firmware_search_dirs: Vec::new(),
            system_card_override: Some(system_card),
            system_card_sha256_override: Some(sha256),
            ppf_stack_override: Some(ppf_stack),
        }
    }

    pub(crate) fn validate_project_branch_scope(
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        validate_direct_pce_cd_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let mut backend = self.load_fresh_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = direct_pce_cd_tas_identity(&backend, &start_state)?;
        validate_direct_pce_cd_tas_state(&mut backend, &start_state)?;
        TasProject::new(
            format!("pce-cd-{}", identity.source_media_sha256.to_hex()),
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
        let mut backend = self.load_fresh_backend()?;
        let projection = validate_direct_pce_cd_tas_state(&mut backend, start_state)?;
        let pce = backend
            .pce()
            .context("PC Engine backend became unavailable")?;
        ensure!(
            projection.frame_count == backend.frame_count()
                && projection.framebuffer.as_ref() == pce.tas_core_framebuffer()
                && pce.tas_presented_frame_is_current(),
            "PC Engine CD TAS starting state did not restore exact frame output"
        );
        let identity = direct_pce_cd_tas_identity(&backend, start_state)?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn load_editor_engine(
        &self,
        project: &TasProject,
    ) -> Result<TasEditorExecutionEngine> {
        validate_direct_pce_cd_tas_project_identity(project)?;
        for branch in project.branches() {
            Self::validate_project_branch_scope(project, branch.id())?;
        }
        let session = self.load_session(project.start_state())?;
        TasEditorExecutionEngine::attach(project, session, validate_direct_pce_cd_tas_branch_scope)
    }

    pub(crate) fn load_fresh_backend(&self) -> Result<EmuBackend> {
        ensure!(
            has_extension(&self.source_path, "cue")
                || has_extension(&self.source_path, "chd")
                || has_extension(&self.source_path, "iso"),
            "PC Engine CD TAS execution requires a direct .cue, .chd, or .iso file"
        );
        let chd = has_extension(&self.source_path, "chd");
        let iso = has_extension(&self.source_path, "iso");
        #[cfg(test)]
        let ppf_stack = if let Some(stack) = self.ppf_stack_override.clone() {
            Some(stack)
        } else if has_extension(&self.source_path, "cue") {
            crate::emu_backend::pce_cd::PceCdTasPpfStack::discover(&self.source_path)?
        } else {
            None
        };
        #[cfg(not(test))]
        let ppf_stack = if has_extension(&self.source_path, "cue") {
            crate::emu_backend::pce_cd::PceCdTasPpfStack::discover(&self.source_path)?
        } else {
            None
        };
        let ppf = ppf_stack.is_some();
        let iso_raw_source = if iso {
            Some(crate::emu_backend::pce_cd_file::direct_file_sha256(
                &self.source_path,
            )?)
        } else {
            None
        };
        let disc = if let Some(stack) = &ppf_stack {
            stack.load(&self.source_path)?
        } else if chd {
            crate::emu_backend::pce_cd::load_direct_chd_with_mods(&self.source_path, false)?
        } else if iso {
            let cue = crate::emu_backend::pce_cd::cue_path_for_iso(&self.source_path)?;
            crate::emu_backend::pce_cd::load_direct_cue_with_mods(&cue, false)?
        } else {
            crate::emu_backend::pce_cd::load_direct_cue_with_mods(&self.source_path, false)?
        };
        let arcade_card = direct_pce_cd_arcade_eligible(ppf, disc.source_disc_sha256);
        let memory_base =
            direct_pce_cd_memory_base_eligible(chd, iso, ppf, disc.source_disc_sha256);
        let source_identity = (
            if let Some(stack) = &ppf_stack {
                stack.source_media_identity().0
            } else if chd {
                direct_pce_cd_chd_source_identity(
                    disc.raw_source_media_sha256,
                    disc.raw_source_media_len,
                )
                .0
            } else if let Some((sha256, bytes)) = iso_raw_source {
                direct_pce_cd_iso_source_identity(sha256, bytes).0
            } else {
                disc.raw_source_media_sha256
            },
            if let Some(stack) = &ppf_stack {
                stack.source_media_identity().1
            } else {
                iso_raw_source.map_or(disc.raw_source_media_len, |(_, bytes)| bytes)
            },
            if chd && arcade_card {
                direct_pce_cd_chd_arcade_tas_sync_config_sha256().0
            } else if chd && memory_base {
                direct_pce_cd_chd_memory_base_tas_sync_config_sha256().0
            } else if chd {
                direct_pce_cd_chd_tas_sync_config_sha256().0
            } else if iso && arcade_card {
                direct_pce_cd_iso_arcade_tas_sync_config_sha256().0
            } else if iso && memory_base {
                direct_pce_cd_iso_memory_base_tas_sync_config_sha256().0
            } else if iso {
                direct_pce_cd_iso_tas_sync_config_sha256().0
            } else if ppf && memory_base {
                direct_pce_cd_ppf_memory_base_tas_sync_config_sha256().0
            } else if ppf {
                direct_pce_cd_ppf_tas_sync_config_sha256().0
            } else if arcade_card {
                direct_pce_cd_arcade_tas_sync_config_sha256().0
            } else if memory_base {
                direct_pce_cd_memory_base_tas_sync_config_sha256().0
            } else {
                direct_pce_cd_tas_sync_config_sha256().0
            },
        );
        let config = BackendLoadConfig {
            sample_rate: Some(48_000),
            apply_mods: ppf,
            initial_input: None,
            pce_console_wiring: Some(PceConsoleWiring::PcEngine),
            pce_cartridge_hardware: Some(PceCartridgeHardware::Base),
            pce_cd_tas_source_media: Some(source_identity),
            pce_cd_tas_ppf_stack: ppf_stack,
            pce_controller_mode: PceControllerMode::TwoButton,
            pce_memory_base_mode: if memory_base {
                PceMemoryBaseMode::Enabled
            } else {
                PceMemoryBaseMode::Disabled
            },
            pce_arcade_card_mode: if arcade_card {
                PceArcadeCardMode::Enabled
            } else {
                PceArcadeCardMode::Disabled
            },
            pce_load_battery_bram: false,
            firmware_search_dirs: self.firmware_search_dirs.clone(),
            #[cfg(test)]
            pce_cd_system_card_override: self.system_card_override,
            #[cfg(test)]
            pce_cd_system_card_sha256_override: self.system_card_sha256_override,
            ..BackendLoadConfig::default()
        };
        let backend = crate::emu_backend::loader::load_backend_from_rom_source(
            ActiveSystem::Pce,
            &self.source_path,
            &self.source_path,
            None,
            config,
        )?
        .backend;
        validate_direct_pce_cd_tas_runtime(&backend, false)?;
        Ok(backend)
    }
}

impl TasEditorExecutionProvider for DirectPceCdTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectPceCdTasExecutionLoader::load_editor_engine(self, project)
    }
}

#[cfg(test)]
mod tests;
