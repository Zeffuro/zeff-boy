use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;
use zeff_pce_core::hardware::{
    PceArcadeCardMode, PceCartridgeHardware, PceConsoleWiring, PceControllerMode, PceMemoryBaseMode,
};

#[path = "direct_pce_cd_loader/archive_ppf.rs"]
mod archive_ppf;
#[cfg(test)]
#[path = "direct_pce_cd_loader/test_registry.rs"]
mod test_registry;
#[cfg(test)]
pub(crate) use test_registry::{register_test_pce_cd_ppf_stack, register_test_pce_cd_system_card};

#[cfg(test)]
use super::direct_pce_cd::{
    direct_pce_cd_archive_tas_sync_config_sha256, direct_pce_cd_rar_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_tas_sync_config_sha256, direct_pce_cd_zip_tas_sync_config_sha256,
};
use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, TasEditorExecutionEngine,
    TasEditorExecutionProvider, TasExecutionSession, TasInitialBranch, TasProject,
    direct_pce_cd::{
        PceCdArchiveFormat, PceCdArchiveSelection, PceCdTasProfile, direct_pce_cd_arcade_eligible,
        direct_pce_cd_chd_source_identity, direct_pce_cd_iso_source_identity,
        direct_pce_cd_memory_base_eligible, direct_pce_cd_memory_base_multitap_eligible,
        direct_pce_cd_tas_identity, direct_pce_multitap_cd_sync_config,
        direct_pce_multitap_cd_tas_identity, validate_direct_pce_cd_tas_branch_scope,
        validate_direct_pce_cd_tas_project_identity, validate_direct_pce_cd_tas_runtime,
        validate_direct_pce_cd_tas_state, validate_direct_pce_multitap_cd_tas_branch_scope,
        validate_direct_pce_multitap_cd_tas_project_identity,
        validate_direct_pce_multitap_cd_tas_runtime, validate_direct_pce_multitap_cd_tas_state,
    },
    has_extension,
};

#[derive(Clone, Debug)]
pub(crate) struct DirectPceCdTasExecutionLoader {
    source_path: PathBuf,
    archive_cue_member: Option<String>,
    firmware_search_dirs: Vec<PathBuf>,
    controller_mode: PceControllerMode,
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
            archive_cue_member: None,
            firmware_search_dirs,
            controller_mode: PceControllerMode::TwoButton,
            #[cfg(test)]
            system_card_override: None,
            #[cfg(test)]
            system_card_sha256_override: None,
            #[cfg(test)]
            ppf_stack_override: None,
        }
    }

    pub(crate) fn new_multitap(source_path: PathBuf, firmware_search_dirs: Vec<PathBuf>) -> Self {
        let mut loader = Self::new(source_path, firmware_search_dirs);
        loader.controller_mode = PceControllerMode::Multitap;
        loader
    }

    pub(crate) fn new_with_rom_path(
        source_path: PathBuf,
        rom_path: Option<PathBuf>,
        firmware_search_dirs: Vec<PathBuf>,
    ) -> Result<Self> {
        let loader = Self::new(source_path, firmware_search_dirs);
        if !(has_extension(&loader.source_path, "7z")
            || has_extension(&loader.source_path, "rar")
            || has_extension(&loader.source_path, "zip"))
        {
            return Ok(loader);
        }
        let cue_members = loader.inspect_archive_cue_members()?;
        if cue_members.len() <= 1 {
            if let (Some(member), Some(rom_path)) = (cue_members.first(), rom_path.as_deref()) {
                ensure!(
                    archive_member_path(&loader.source_path, member) == rom_path,
                    "active PC Engine CD archive member changed"
                );
            }
            return Ok(loader);
        }
        let rom_path = rom_path.context("multi-CUE archive requires an explicit CUE member")?;
        let cue_member = cue_members
            .into_iter()
            .find(|member| archive_member_path(&loader.source_path, member) == rom_path)
            .context("selected PC Engine CD archive member is unavailable")?;
        loader.with_archive_cue_member(cue_member)
    }

    pub(crate) fn new_for_loaded_rom_path(
        source_path: PathBuf,
        rom_path: &Path,
        firmware_search_dirs: Vec<PathBuf>,
    ) -> Result<Self> {
        let loader = Self::new(source_path, firmware_search_dirs);
        if !(has_extension(&loader.source_path, "7z")
            || has_extension(&loader.source_path, "rar")
            || has_extension(&loader.source_path, "zip"))
        {
            return Ok(loader);
        }
        let member = rom_path
            .strip_prefix(&loader.source_path)
            .context("active PC Engine CD archive member is outside its source")?
            .to_str()
            .context("active PC Engine CD archive member is not UTF-8")?
            .replace('\\', "/");
        loader.with_archive_cue_member(member)
    }

    pub(crate) fn new_multitap_with_rom_path(
        source_path: PathBuf,
        rom_path: Option<PathBuf>,
        firmware_search_dirs: Vec<PathBuf>,
    ) -> Result<Self> {
        let mut loader = Self::new_with_rom_path(source_path, rom_path, firmware_search_dirs)?;
        loader.controller_mode = PceControllerMode::Multitap;
        Ok(loader)
    }

    pub(crate) fn new_for_project(
        source_path: PathBuf,
        firmware_search_dirs: Vec<PathBuf>,
        project: &TasProject,
    ) -> Result<Self> {
        let profile = PceCdTasProfile::from_sync(project.identity().sync_config_sha256)
            .context("PC Engine CD project has an unknown sync configuration")?;
        let multitap = profile.controller() == PceControllerMode::Multitap;
        if multitap {
            validate_direct_pce_multitap_cd_tas_project_identity(project)?;
        } else {
            validate_direct_pce_cd_tas_project_identity(project)?;
        }
        let loader = if multitap {
            Self::new_multitap(source_path, firmware_search_dirs)
        } else {
            Self::new(source_path, firmware_search_dirs)
        };
        #[cfg(test)]
        let loader = {
            let mut loader = loader;
            if let Some(sha256) =
                project
                    .identity()
                    .firmware
                    .iter()
                    .find_map(|firmware| match firmware {
                        crate::tas_project::TasFirmwareIdentity::External { sha256, .. } => {
                            Some(sha256.0)
                        }
                        _ => None,
                    })
                && let Some(bytes) = test_registry::system_card(sha256)
            {
                loader.system_card_override = Some(bytes);
                loader.system_card_sha256_override = Some(sha256);
            }
            loader
        };
        let Some((archive_format, PceCdArchiveSelection::Selected)) = profile.archive() else {
            return Ok(loader);
        };
        ensure!(
            match archive_format {
                PceCdArchiveFormat::SevenZip => has_extension(&loader.source_path, "7z"),
                PceCdArchiveFormat::Rar => has_extension(&loader.source_path, "rar"),
                PceCdArchiveFormat::Zip => has_extension(&loader.source_path, "zip"),
            },
            "PC Engine CD project archive format differs from its source"
        );
        if profile.archive_ppf() {
            let matches = loader
                .inspect_archive_ppf_candidates()?
                .into_iter()
                .filter(|candidate| {
                    let patches = archive_ppf::identity_parts(&candidate.patches);
                    profile
                        .archive_ppf_source_identity(
                            candidate.identity.source_sha256,
                            candidate.identity.source_len,
                            candidate.identity.cue_member_path_sha256,
                            &patches,
                        )
                        .is_some_and(|source| source == project.identity().source_media_sha256)
                })
                .map(|candidate| candidate.cue_member)
                .collect::<Vec<_>>();
            ensure!(
                matches.len() == 1,
                "PC Engine CD project selected archive PPF identity is unavailable or ambiguous"
            );
            return loader
                .with_archive_cue_member(matches.into_iter().next().expect("one selected CUE"));
        }
        let candidates = loader.inspect_archive_cue_candidates()?;
        let matches = candidates
            .into_iter()
            .filter(|candidate| {
                let source = profile
                    .archive_source_identity(
                        candidate.identity.source_sha256,
                        candidate.identity.source_len,
                        candidate.identity.cue_member_path_sha256,
                    )
                    .expect("selected archive profile");
                source == project.identity().source_media_sha256
            })
            .map(|candidate| candidate.cue_member)
            .collect::<Vec<_>>();
        ensure!(
            matches.len() == 1,
            "PC Engine CD project selected CUE identity is unavailable or ambiguous"
        );
        loader.with_archive_cue_member(matches.into_iter().next().expect("one selected CUE"))
    }

    fn inspect_archive_cue_members(&self) -> Result<Vec<String>> {
        if has_extension(&self.source_path, "7z") {
            Ok(crate::emu_backend::pce_cd_archive::inspect_7z_cue_members(
                &self.source_path,
                512,
            )?)
        } else if has_extension(&self.source_path, "rar") {
            Ok(crate::emu_backend::pce_cd_rar::inspect_rar_cue_members(
                &self.source_path,
            )?)
        } else {
            Ok(crate::emu_backend::pce_cd_zip::inspect_zip_cue_members(
                &self.source_path,
            )?)
        }
    }

    fn inspect_archive_cue_candidates(
        &self,
    ) -> Result<Vec<crate::emu_backend::pce_cd_archive::PceCdArchiveCueCandidate>> {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        if has_extension(&self.source_path, "7z") {
            let progress = crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default();
            Ok(crate::emu_backend::pce_cd_archive::inspect_7z_cue_candidates_with_archive_identity(
                &self.source_path,
                &cancel,
                &progress,
                512,
            )?)
        } else if has_extension(&self.source_path, "rar") {
            Ok(
                crate::emu_backend::pce_cd_rar::inspect_rar_cue_candidates_with_archive_identity(
                    &self.source_path,
                    &cancel,
                )?,
            )
        } else {
            Ok(
                crate::emu_backend::pce_cd_zip::inspect_zip_cue_candidates_with_archive_identity(
                    &self.source_path,
                    &cancel,
                )?,
            )
        }
    }

    fn with_archive_cue_member(mut self, cue_member: String) -> Result<Self> {
        ensure!(
            has_extension(&self.source_path, "7z")
                || has_extension(&self.source_path, "rar")
                || has_extension(&self.source_path, "zip"),
            "explicit CUE selection requires a 7z, RAR, or ZIP source"
        );
        let normalized = crate::emu_backend::pce_cd::normalize_portable_path(&cue_member)
            .map_err(|_| anyhow::anyhow!("archive CUE selection is not a safe portable path"))?;
        ensure!(
            has_extension(Path::new(&normalized), "cue"),
            "archive CUE selection must identify a .cue member"
        );
        self.archive_cue_member = Some(normalized);
        Ok(self)
    }

    #[cfg(test)]
    pub(crate) fn new_with_system_card_override(
        source_path: PathBuf,
        system_card: &'static [u8],
        sha256: [u8; 32],
    ) -> Self {
        Self {
            source_path,
            archive_cue_member: None,
            firmware_search_dirs: Vec::new(),
            controller_mode: PceControllerMode::TwoButton,
            system_card_override: Some(system_card),
            system_card_sha256_override: Some(sha256),
            ppf_stack_override: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_multitap_with_system_card_override(
        source_path: PathBuf,
        system_card: &'static [u8],
        sha256: [u8; 32],
    ) -> Self {
        let mut loader = Self::new_multitap(source_path, Vec::new());
        loader.system_card_override = Some(system_card);
        loader.system_card_sha256_override = Some(sha256);
        loader
    }

    #[cfg(test)]
    pub(crate) fn new_with_rom_path_and_system_card_override(
        source_path: PathBuf,
        rom_path: PathBuf,
        system_card: &'static [u8],
        sha256: [u8; 32],
    ) -> Result<Self> {
        let mut loader = Self::new_with_rom_path(source_path, Some(rom_path), Vec::new())?;
        loader.system_card_override = Some(system_card);
        loader.system_card_sha256_override = Some(sha256);
        Ok(loader)
    }

    #[cfg(test)]
    pub(crate) fn new_multitap_with_rom_path_and_system_card_override(
        source_path: PathBuf,
        rom_path: PathBuf,
        system_card: &'static [u8],
        sha256: [u8; 32],
    ) -> Result<Self> {
        let mut loader = Self::new_multitap_with_rom_path(source_path, Some(rom_path), Vec::new())?;
        loader.system_card_override = Some(system_card);
        loader.system_card_sha256_override = Some(sha256);
        Ok(loader)
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
            archive_cue_member: None,
            firmware_search_dirs: Vec::new(),
            controller_mode: PceControllerMode::TwoButton,
            system_card_override: Some(system_card),
            system_card_sha256_override: Some(sha256),
            ppf_stack_override: Some(ppf_stack),
        }
    }

    #[cfg(test)]
    pub(crate) fn new_multitap_with_system_card_and_ppf_stack(
        source_path: PathBuf,
        system_card: &'static [u8],
        sha256: [u8; 32],
        ppf_stack: crate::emu_backend::pce_cd::PceCdTasPpfStack,
    ) -> Self {
        let mut loader =
            Self::new_with_system_card_and_ppf_stack(source_path, system_card, sha256, ppf_stack);
        loader.controller_mode = PceControllerMode::Multitap;
        loader
    }

    pub(crate) fn validate_project_branch_scope(
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        if direct_pce_multitap_cd_sync_config(project.identity().sync_config_sha256) {
            validate_direct_pce_multitap_cd_tas_branch_scope(project, branch_id)
        } else {
            validate_direct_pce_cd_tas_branch_scope(project, branch_id)
        }
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let mut backend = self.load_fresh_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = self.identity(&backend, &start_state)?;
        self.validate_state(&mut backend, &start_state)?;
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
        let projection = self.validate_state(&mut backend, start_state)?;
        let pce = backend
            .pce()
            .context("PC Engine backend became unavailable")?;
        ensure!(
            projection.frame_count == backend.frame_count()
                && projection.framebuffer.as_ref() == pce.tas_core_framebuffer()
                && pce.tas_presented_frame_is_current(),
            "PC Engine CD TAS starting state did not restore exact frame output"
        );
        let identity = self.identity(&backend, start_state)?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn load_editor_engine(
        &self,
        project: &TasProject,
    ) -> Result<TasEditorExecutionEngine> {
        self.validate_project_identity(project)?;
        for branch in project.branches() {
            Self::validate_project_branch_scope(project, branch.id())?;
        }
        let session = self.load_session(project.start_state())?;
        let validate = if self.controller_mode == PceControllerMode::Multitap {
            validate_direct_pce_multitap_cd_tas_branch_scope
        } else {
            validate_direct_pce_cd_tas_branch_scope
        };
        TasEditorExecutionEngine::attach(project, session, validate)
    }

    pub(crate) fn load_fresh_backend(&self) -> Result<EmuBackend> {
        let multitap = self.controller_mode == PceControllerMode::Multitap;
        ensure!(
            (has_extension(&self.source_path, "cue")
                || has_extension(&self.source_path, "chd")
                || has_extension(&self.source_path, "iso")
                || has_extension(&self.source_path, "7z")
                || has_extension(&self.source_path, "rar")
                || has_extension(&self.source_path, "zip")),
            "PC Engine CD TAS execution requires a direct .cue, .chd, .iso, or exact archive package"
        );
        let chd = has_extension(&self.source_path, "chd");
        let iso = has_extension(&self.source_path, "iso");
        let archive = has_extension(&self.source_path, "7z");
        let rar = has_extension(&self.source_path, "rar");
        let zip = has_extension(&self.source_path, "zip");
        if !multitap
            && (archive || rar || zip)
            && let Some(backend) = self.try_load_archive_ppf_backend()?
        {
            validate_direct_pce_cd_tas_runtime(&backend, false)?;
            return Ok(backend);
        }
        #[cfg(test)]
        let ppf_stack = if let Some(stack) = self.ppf_stack_override.clone() {
            Some(stack)
        } else if let Some(stack) = test_registry::ppf_stack(&self.source_path) {
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
        let (disc, archive_cue_path, archive_identity, rar_identity, zip_identity) = if let Some(
            stack,
        ) =
            &ppf_stack
        {
            (stack.load(&self.source_path)?, None, None, None, None)
        } else if chd {
            (
                crate::emu_backend::pce_cd::load_direct_chd_with_mods(&self.source_path, false)?,
                None,
                None,
                None,
                None,
            )
        } else if iso {
            let cue = crate::emu_backend::pce_cd::cue_path_for_iso(&self.source_path)?;
            (
                crate::emu_backend::pce_cd::load_direct_cue_with_mods(&cue, false)?,
                None,
                None,
                None,
                None,
            )
        } else if archive {
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let progress = crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default();
            let (cue_path, disc, identity) = if let Some(selected) = &self.archive_cue_member {
                crate::emu_backend::pce_cd_archive::load_7z_selected_cue_with_control_and_archive_identity(
                    &self.source_path,
                    selected,
                    &cancel,
                    &progress,
                    512,
                    false,
                )?
            } else {
                crate::emu_backend::pce_cd_archive::load_7z_cue_with_control_and_archive_identity(
                    &self.source_path,
                    &cancel,
                    &progress,
                    512,
                    false,
                )?
            };
            (disc, Some(cue_path), Some(identity), None, None)
        } else if rar {
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let progress = std::sync::Arc::new(
                crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default(),
            );
            let (cue_path, disc, identity) = if let Some(selected) = &self.archive_cue_member {
                crate::emu_backend::pce_cd_rar::load_rar_selected_cue_with_control_and_archive_identity(
                    &self.source_path,
                    selected,
                    cancel,
                    progress,
                    false,
                )?
            } else {
                crate::emu_backend::pce_cd_rar::load_rar_cue_with_control_and_archive_identity(
                    &self.source_path,
                    cancel,
                    progress,
                    false,
                )?
            };
            (disc, Some(cue_path), None, Some(identity), None)
        } else if zip {
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let progress = std::sync::Arc::new(
                crate::emu_backend::pce_cd_archive::PceCdPackageProgress::default(),
            );
            let (cue_path, disc, identity) = if let Some(selected) = &self.archive_cue_member {
                crate::emu_backend::pce_cd_zip::load_zip_selected_cue_with_control_and_archive_identity(
                    &self.source_path,
                    selected,
                    cancel,
                    progress,
                    false,
                )?
            } else {
                crate::emu_backend::pce_cd_zip::load_zip_cue_with_control_and_archive_identity(
                    &self.source_path,
                    cancel,
                    progress,
                    false,
                )?
            };
            (disc, Some(cue_path), None, None, Some(identity))
        } else {
            (
                crate::emu_backend::pce_cd::load_direct_cue_with_mods(&self.source_path, false)?,
                None,
                None,
                None,
                None,
            )
        };
        let arcade_card = direct_pce_cd_arcade_eligible(ppf, disc.source_disc_sha256);
        let memory_base_catalog = crate::emu_backend::pce_profiles::automatic_memory_base_enabled(
            Some(disc.source_disc_sha256),
        );
        let memory_base = if multitap {
            ensure!(
                !memory_base_catalog || !(chd || iso || ppf || archive || rar || zip),
                "PC Engine CD Memory Base 128 plus Multitap TAS is limited to direct CUE media"
            );
            memory_base_catalog
                && direct_pce_cd_memory_base_multitap_eligible(disc.source_disc_sha256)
        } else {
            direct_pce_cd_memory_base_eligible(chd, iso, ppf, disc.source_disc_sha256)
        };
        ensure!(
            !multitap
                || crate::emu_backend::pce_profiles::automatic_controller_mode(
                    disc.source_disc_sha256,
                ) == PceControllerMode::Multitap,
            "PC Engine CD Multitap TAS requires an exact controller catalog witness"
        );
        let selected_archive = self.archive_cue_member.is_some();
        let profile = PceCdTasProfile::from_runtime_flags(
            (chd, iso, ppf, archive, rar, zip),
            false,
            (
                archive && selected_archive,
                rar && selected_archive,
                zip && selected_archive,
            ),
            (arcade_card, memory_base),
            self.controller_mode,
        )
        .context("PC Engine CD TAS source describes an invalid execution profile")?;
        let package_identity = archive_identity.or(rar_identity).or(zip_identity);
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
            } else if let Some(identity) = package_identity {
                profile
                    .archive_source_identity(
                        identity.source_sha256,
                        identity.source_len,
                        identity.cue_member_path_sha256,
                    )
                    .context("PC Engine CD package omitted its archive profile")?
                    .0
            } else {
                disc.raw_source_media_sha256
            },
            if let Some(stack) = &ppf_stack {
                stack.source_media_identity().1
            } else if let Some(identity) = package_identity {
                identity.source_len
            } else {
                iso_raw_source.map_or(disc.raw_source_media_len, |(_, bytes)| bytes)
            },
            profile.sync_config().0,
        );
        #[cfg(test)]
        let (system_card_override, system_card_sha256_override) =
            if self.system_card_override.is_some() {
                (self.system_card_override, self.system_card_sha256_override)
            } else {
                test_registry::sole_system_card()
            };
        let config = BackendLoadConfig {
            sample_rate: Some(48_000),
            apply_mods: ppf,
            initial_input: None,
            pce_console_wiring: Some(PceConsoleWiring::PcEngine),
            pce_cartridge_hardware: Some(PceCartridgeHardware::Base),
            pce_cd_tas_source_media: Some(source_identity),
            pce_cd_tas_archive_cue: archive_identity,
            pce_cd_tas_rar_cue: rar_identity,
            pce_cd_tas_zip_cue: zip_identity,
            pce_cd_tas_ppf_stack: ppf_stack,
            pce_controller_mode: self.controller_mode,
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
            pce_cd_system_card_override: system_card_override,
            #[cfg(test)]
            pce_cd_system_card_sha256_override: system_card_sha256_override,
            ..BackendLoadConfig::default()
        };
        let backend = crate::emu_backend::loader::load_backend_from_rom_source(
            ActiveSystem::Pce,
            &self.source_path,
            archive_cue_path.as_deref().unwrap_or(&self.source_path),
            None,
            config,
        )?
        .backend;
        if multitap {
            validate_direct_pce_multitap_cd_tas_runtime(&backend, false)?;
        } else {
            validate_direct_pce_cd_tas_runtime(&backend, false)?;
        }
        Ok(backend)
    }

    fn identity(
        &self,
        backend: &EmuBackend,
        state: &[u8],
    ) -> Result<crate::tas_project::TasProjectIdentity> {
        if self.controller_mode == PceControllerMode::Multitap {
            direct_pce_multitap_cd_tas_identity(backend, state)
        } else {
            direct_pce_cd_tas_identity(backend, state)
        }
    }

    fn validate_state(
        &self,
        backend: &mut EmuBackend,
        state: &[u8],
    ) -> Result<crate::emu_backend::pce::PceTasStateProjection> {
        if self.controller_mode == PceControllerMode::Multitap {
            validate_direct_pce_multitap_cd_tas_state(backend, state)
        } else {
            validate_direct_pce_cd_tas_state(backend, state)
        }
    }

    fn validate_project_identity(&self, project: &TasProject) -> Result<()> {
        if self.controller_mode == PceControllerMode::Multitap {
            validate_direct_pce_multitap_cd_tas_project_identity(project)
        } else {
            validate_direct_pce_cd_tas_project_identity(project)
        }
    }
}

fn archive_member_path(source_path: &Path, member: &str) -> PathBuf {
    member
        .split('/')
        .fold(source_path.to_path_buf(), |path, component| {
            path.join(component)
        })
}

impl TasEditorExecutionProvider for DirectPceCdTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectPceCdTasExecutionLoader::load_editor_engine(self, project)
    }
}

#[cfg(test)]
mod tests;
