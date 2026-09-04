use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, ensure};
use zeff_emu_common::replay::ReplayStartMetadata;

use super::media::read_bounded_direct_rom;
use super::{
    ActiveSystem, BackendLoadConfig, EmuBackend, TasDigest, TasEditorExecutionEngine,
    TasEditorExecutionProvider, TasExecutionSession, TasInitialBranch, TasProject,
    direct_ws::direct_ws_tas_identity, has_extension, validate_direct_ws_tas_private_runtime,
    validate_direct_ws_tas_private_state,
};

pub(crate) const MAX_DIRECT_WS_ROM_BYTES: u64 = 16 * 1024 * 1024;
const MAX_WS_ZIP_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug)]
pub(crate) struct DirectWsTasExecutionLoader {
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
}

pub(crate) struct WsTasMediaIdentity {
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
}

impl DirectWsTasExecutionLoader {
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
        super::direct_ws::validate_direct_ws_tas_project_identity(project)?;
        let orientation = super::direct_ws::direct_ws_tas_orientation(project)?;
        let battery =
            project.identity().persistent_state != crate::tas_project::TasExternalIdentity::Absent;
        let rtc = project.identity().rtc_state != crate::tas_project::TasExternalIdentity::Absent;
        let rtc_save_kinds: &[zeff_ws_core::hardware::cartridge::SaveKind] = if battery {
            &[
                zeff_ws_core::hardware::cartridge::SaveKind::Sram32KId1,
                zeff_ws_core::hardware::cartridge::SaveKind::Sram32K,
                zeff_ws_core::hardware::cartridge::SaveKind::Sram128K,
                zeff_ws_core::hardware::cartridge::SaveKind::Sram256K,
                zeff_ws_core::hardware::cartridge::SaveKind::Sram512K,
                zeff_ws_core::hardware::cartridge::SaveKind::Eeprom128,
                zeff_ws_core::hardware::cartridge::SaveKind::Eeprom1K,
                zeff_ws_core::hardware::cartridge::SaveKind::Eeprom2K,
            ]
        } else {
            &[zeff_ws_core::hardware::cartridge::SaveKind::None]
        };
        let mut matches = Vec::new();
        for (extension, system) in [
            (
                "ws",
                zeff_ws_core::hardware::cartridge::MinimumSystem::WonderSwan,
            ),
            (
                "wsc",
                zeff_ws_core::hardware::cartridge::MinimumSystem::WonderSwanColor,
            ),
        ] {
            let inspection = crate::rom_archive::inspect_bounded_zip_members(
                &source_path,
                extension,
                MAX_WS_ZIP_BYTES,
                MAX_DIRECT_WS_ROM_BYTES,
            )?;
            ensure!(
                TasDigest(inspection.archive_sha256) == project.identity().source_media_sha256,
                "WonderSwan ZIP archive does not match the TAS project"
            );
            matches.extend(inspection.entries.into_iter().filter(|entry| {
                if rtc {
                    rtc_save_kinds.iter().copied().any(|save_kind| {
                        super::direct_ws::zip_ws_rtc_tas_sync_config_sha256(
                            system,
                            orientation,
                            save_kind,
                            &entry.member_name,
                        )
                        .is_ok_and(|sync| sync == project.identity().sync_config_sha256)
                    })
                } else {
                    super::direct_ws::zip_ws_tas_sync_config_sha256(
                        system,
                        orientation,
                        battery,
                        &entry.member_name,
                    )
                    .is_ok_and(|sync| sync == project.identity().sync_config_sha256)
                }
            }));
        }
        ensure!(
            matches.len() == 1,
            "WonderSwan ZIP member does not match the TAS project"
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
        super::direct_ws::validate_direct_ws_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, media) = self.load_creation_backend()?;
        let start_state = backend.encode_state_bytes()?;
        let identity = self.identity(&backend, media, &start_state)?;
        TasProject::new(
            format!("ws-{}", identity.source_media_sha256.to_hex()),
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
        let fresh_start_state = backend.encode_state_bytes()?;
        if backend.save_ram_kind() == zeff_emu_common::save_ram::SaveRamKind::None {
            ensure!(
                fresh_start_state.as_slice() == start_state,
                "WonderSwan TAS starting state does not match the fresh direct-ROM baseline"
            );
        }
        let projection = validate_direct_ws_tas_private_state(&mut backend, start_state)?;
        ensure!(
            projection.frame_count == 0 && projection.framebuffer.as_ref() == backend.framebuffer(),
            "WonderSwan TAS starting state does not restore the fresh baseline frame"
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
            super::direct_ws::validate_direct_ws_tas_branch_scope,
        )
    }

    fn load_creation_backend(&self) -> Result<(EmuBackend, WsTasMediaIdentity)> {
        self.load_backend(true)
    }

    pub(crate) fn load_fresh_backend(&self) -> Result<(EmuBackend, WsTasMediaIdentity)> {
        self.load_backend(false)
    }

    fn load_backend(&self, load_battery_sram: bool) -> Result<(EmuBackend, WsTasMediaIdentity)> {
        let config = BackendLoadConfig {
            sample_rate: None,
            apply_mods: false,
            initial_input: None,
            ws_load_battery_sram: load_battery_sram,
            ..BackendLoadConfig::default()
        };
        let (backend, selected_name, archive_sha) = if has_extension(&self.source_path, "ws")
            || has_extension(&self.source_path, "wsc")
        {
            let source_bytes = read_direct_ws_rom(&self.source_path)?;
            let backend = crate::emu_backend::loader::load_backend_from_bounded_direct_source(
                ActiveSystem::WonderSwan,
                &self.source_path,
                source_bytes,
                config,
            )?
            .backend;
            (backend, None, None)
        } else if has_extension(&self.source_path, "zip") {
            let selected = self.select_zip_member()?;
            let name = selected.member_name.clone();
            let archive_sha = selected.archive_sha256;
            let backend = super::load_backend_from_rom_source(
                ActiveSystem::WonderSwan,
                &self.source_path,
                &selected.rom_path,
                Some(selected.bytes),
                config,
            )?
            .backend;
            (backend, Some(name), Some(archive_sha))
        } else {
            anyhow::bail!(
                "WonderSwan TAS execution requires a direct .ws/.wsc file or selected ZIP member"
            );
        };
        validate_direct_ws_tas_private_runtime(&backend, false)?;
        let inspection = validate_direct_ws_tas_private_runtime(&backend, false)?;
        let sync_config_sha256 = if let Some(member_name) = selected_name.as_deref() {
            if inspection.rtc_present {
                super::direct_ws::zip_ws_rtc_tas_sync_config_sha256(
                    inspection.minimum_system,
                    inspection.orientation,
                    inspection.save_kind,
                    member_name,
                )?
            } else {
                super::direct_ws::zip_ws_tas_sync_config_sha256(
                    inspection.minimum_system,
                    inspection.orientation,
                    inspection.save_kind != zeff_ws_core::hardware::cartridge::SaveKind::None,
                    member_name,
                )?
            }
        } else {
            super::direct_ws::direct_ws_tas_sync_config_for_inspection(&inspection)?
        };
        let source_media_sha256 = archive_sha
            .map(TasDigest)
            .unwrap_or_else(|| TasDigest(backend.rom_hash()));
        Ok((
            backend,
            WsTasMediaIdentity {
                source_media_sha256,
                sync_config_sha256,
            },
        ))
    }

    fn select_zip_member(&self) -> Result<crate::rom_archive::BoundedZipMember> {
        if let Some(rom_path) = self.rom_path.as_deref() {
            let extension = rom_path
                .extension()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            ensure!(
                extension.eq_ignore_ascii_case("ws") || extension.eq_ignore_ascii_case("wsc"),
                "selected WonderSwan ZIP member must use .ws or .wsc"
            );
            return crate::rom_archive::extract_bounded_zip_member(
                &self.source_path,
                Some(rom_path),
                extension,
                MAX_WS_ZIP_BYTES,
                MAX_DIRECT_WS_ROM_BYTES,
            );
        }
        let mut entries = Vec::new();
        for extension in ["ws", "wsc"] {
            entries.extend(
                crate::rom_archive::inspect_bounded_zip_members(
                    &self.source_path,
                    extension,
                    MAX_WS_ZIP_BYTES,
                    MAX_DIRECT_WS_ROM_BYTES,
                )?
                .entries,
            );
        }
        ensure!(
            entries.len() == 1,
            "WonderSwan ZIP must contain exactly one .ws or .wsc member unless one is selected explicitly"
        );
        let entry = &entries[0];
        let extension = entry
            .rom_path
            .extension()
            .and_then(|value| value.to_str())
            .context("WonderSwan ZIP member omitted its extension")?;
        crate::rom_archive::extract_bounded_zip_member(
            &self.source_path,
            Some(&entry.rom_path),
            extension,
            MAX_WS_ZIP_BYTES,
            MAX_DIRECT_WS_ROM_BYTES,
        )
    }

    fn identity(
        &self,
        backend: &EmuBackend,
        media: WsTasMediaIdentity,
        start_state: &[u8],
    ) -> Result<crate::tas_project::TasProjectIdentity> {
        if !has_extension(&self.source_path, "zip") {
            let source_bytes = read_direct_ws_rom(&self.source_path)?;
            ensure!(
                media.source_media_sha256 == TasDigest::from_bytes(&source_bytes),
                "WonderSwan source changed while constructing TAS identity"
            );
            return direct_ws_tas_identity(backend, &source_bytes, start_state);
        }
        let selected = self.select_zip_member()?;
        ensure!(
            media.source_media_sha256 == TasDigest(selected.archive_sha256)
                && TasDigest::from_bytes(&selected.bytes) == TasDigest(backend.rom_hash()),
            "WonderSwan ZIP changed while constructing TAS identity"
        );
        let identity = super::direct_ws::zip_ws_tas_identity(
            backend,
            selected.archive_sha256,
            &selected.member_name,
            start_state,
        )?;
        ensure!(
            identity.sync_config_sha256 == media.sync_config_sha256,
            "WonderSwan ZIP profile changed while constructing TAS identity"
        );
        Ok(identity)
    }
}

impl TasEditorExecutionProvider for DirectWsTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectWsTasExecutionLoader::load_editor_engine(self, project)
    }
}

fn read_direct_ws_rom(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to inspect TAS source media {}", path.display()))?;
    ensure!(
        (10..=MAX_DIRECT_WS_ROM_BYTES).contains(&metadata.len()),
        "direct WonderSwan TAS media has an unsupported size"
    );
    let expected_len = usize::try_from(metadata.len()).context("WonderSwan media is too large")?;
    read_bounded_direct_rom(
        path,
        expected_len,
        "direct WonderSwan TAS media changed while it was read",
    )
}

#[cfg(test)]
mod tests;
