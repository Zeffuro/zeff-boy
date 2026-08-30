#![cfg(not(target_arch = "wasm32"))]

use std::collections::BTreeMap;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail, ensure};
use zeff_emu_common::replay::{ReplayFirmwareManifest, ReplayStartMetadata};

use super::{BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::tas_project::verification::TasExecutionSession;
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasEditorExecutionAttachment,
    TasEditorExecutionEngine, TasEditorExecutionProvider, TasEditorExecutionUnavailableReason,
    TasExternalIdentity, TasFirmwareIdentity, TasInitialBranch, TasProject, TasProjectIdentity,
};

mod direct_gb;
mod live_binding;
pub(crate) use direct_gb::{
    DirectGbTasExecutionLoader, DirectGbTasRuntimeWitness, direct_gb_tas_sync_config_sha256,
    validate_direct_gb_tas_project_identity, validate_direct_gb_tas_project_witness,
    validate_direct_gb_tas_runtime, validate_direct_gb_tas_state,
};
pub(crate) use live_binding::{
    DirectNesTasRuntimeWitness, validate_direct_nes_tas_project_witness,
};

pub(crate) const MAX_NES_CARTRIDGE_BYTES: u64 = 64 * 1024 * 1024;
const NES_GAMEPAD_CONFIGURATION: &[u8] = b"zeff-tas-device-config-v1\0nes-standard-controller\0";
const NES_CARTRIDGE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0nes-cartridge\0mods=disabled\0initial-input=neutral\0sample-rate=core-default\0external-state=absent\0";

pub(crate) fn direct_nes_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(NES_CARTRIDGE_SYNC_CONFIGURATION)
}

pub(crate) fn classify_direct_tas_execution_profile(
    project: &TasProject,
) -> Result<crate::emu_thread::TasExecutionProfile> {
    project.validate()?;
    match project.identity().system.as_str() {
        system if system == ActiveSystem::Nes.code() => {
            validate_direct_nes_tas_project_identity(project)?;
            Ok(crate::emu_thread::TasExecutionProfile::DirectNesCartridge)
        }
        system if system == ActiveSystem::GameBoy.code() => {
            validate_direct_gb_tas_project_identity(project)?;
            Ok(crate::emu_thread::TasExecutionProfile::DirectGbRomOnlyDmg)
        }
        _ => bail!("the TAS project does not identify a live execution profile"),
    }
}

pub(crate) struct TasProjectRuntimeWitness<'a> {
    pub(crate) profile: crate::emu_thread::TasExecutionProfile,
    pub(crate) source_media_sha256: TasDigest,
    pub(crate) effective_media_sha256: TasDigest,
    pub(crate) current_state_bytes: &'a [u8],
    pub(crate) current_state_sha256: TasDigest,
    pub(crate) determinism_abi: &'a str,
    pub(crate) state_format_compatibility_id: &'a str,
    pub(crate) sync_config_sha256: TasDigest,
}

pub(crate) fn validate_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    ensure!(
        classify_direct_tas_execution_profile(project)? == witness.profile,
        "worker execution profile does not match the TAS project"
    );
    match witness.profile {
        crate::emu_thread::TasExecutionProfile::DirectNesCartridge => {
            validate_direct_nes_tas_project_witness(
                project,
                branch_id,
                DirectNesTasRuntimeWitness {
                    source_media_sha256: witness.source_media_sha256,
                    effective_media_sha256: witness.effective_media_sha256,
                    current_state_bytes: witness.current_state_bytes,
                    current_state_sha256: witness.current_state_sha256,
                    determinism_abi: witness.determinism_abi,
                    state_format_compatibility_id: witness.state_format_compatibility_id,
                    sync_config_sha256: witness.sync_config_sha256,
                },
            )
        }
        crate::emu_thread::TasExecutionProfile::DirectGbRomOnlyDmg => {
            validate_direct_gb_tas_project_witness(
                project,
                branch_id,
                DirectGbTasRuntimeWitness {
                    source_media_sha256: witness.source_media_sha256,
                    effective_media_sha256: witness.effective_media_sha256,
                    current_state_bytes: witness.current_state_bytes,
                    current_state_sha256: witness.current_state_sha256,
                    determinism_abi: witness.determinism_abi,
                    state_format_compatibility_id: witness.state_format_compatibility_id,
                    sync_config_sha256: witness.sync_config_sha256,
                },
            )
        }
    }
}

fn direct_nes_tas_devices() -> Vec<TasDeviceIdentity> {
    ["p1", "p2"]
        .into_iter()
        .map(|port| TasDeviceIdentity {
            port: port.to_owned(),
            device: "nes-standard-controller".to_owned(),
            configuration_sha256: TasDigest::from_bytes(NES_GAMEPAD_CONFIGURATION),
        })
        .collect()
}

#[derive(Clone, Debug)]
pub(crate) struct DirectNesTasExecutionLoader {
    source_path: PathBuf,
    firmware_search_dirs: Vec<PathBuf>,
}

impl DirectNesTasExecutionLoader {
    pub(crate) fn new(source_path: PathBuf, firmware_search_dirs: Vec<PathBuf>) -> Self {
        Self {
            source_path,
            firmware_search_dirs,
        }
    }

    pub(crate) fn validate_project_branch_scope(
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        validate_direct_nes_tas_branch_scope(project, branch_id)
    }

    pub(crate) fn load_session(&self, start_state: &[u8]) -> Result<TasExecutionSession> {
        validate_current_nes_start_state(start_state)?;
        let (mut backend, source_bytes) = self.load_fresh_backend()?;
        backend
            .load_state_from_bytes(start_state.to_vec())
            .context("failed to restore TAS starting state for device-profile validation")?;
        ensure!(
            backend.nes_has_standard_controller_topology() == Some(true),
            "TAS starting state does not restore the standard NES controller topology"
        );
        let identity = direct_nes_tas_identity(&backend, &source_bytes, start_state)?;
        Ok(TasExecutionSession::new(backend, identity))
    }

    pub(crate) fn create_project(&self) -> Result<TasProject> {
        let (backend, source_bytes) = self.load_fresh_backend()?;
        let start_state = backend.encode_state_bytes()?;
        validate_current_nes_start_state(&start_state)?;
        let identity = direct_nes_tas_identity(&backend, &source_bytes, &start_state)?;
        let project_id = format!("nes-{}", identity.source_media_sha256.to_hex());
        TasProject::new(
            project_id,
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

    fn load_fresh_backend(&self) -> Result<(EmuBackend, Vec<u8>)> {
        ensure!(
            self.source_path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("nes")),
            "TAS execution currently supports direct .nes cartridge files only"
        );
        let source_bytes = read_nes_cartridge_bounded(&self.source_path)?;
        let backend = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &self.source_path,
            &self.source_path,
            Some(source_bytes.clone()),
            BackendLoadConfig {
                firmware_search_dirs: self.firmware_search_dirs.clone(),
                sample_rate: None,
                apply_mods: false,
                initial_input: None,
                nes_load_battery_sram: false,
                ..BackendLoadConfig::default()
            },
        )?
        .backend;
        ensure!(
            backend.nes_has_standard_controller_topology() == Some(true),
            "TAS creation requires the standard NES controller topology"
        );
        Ok((backend, source_bytes))
    }
}

impl TasEditorExecutionProvider for DirectNesTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        DirectNesTasExecutionLoader::load_editor_engine(self, project)
    }
}

#[derive(Clone, Debug)]
pub(crate) enum PrivateTasExecutionLoader {
    DirectNes(DirectNesTasExecutionLoader),
    DirectGb(DirectGbTasExecutionLoader),
}

impl PrivateTasExecutionLoader {
    pub(crate) fn create_project_file(&self, path: &Path) -> Result<TasProject> {
        match self {
            Self::DirectNes(loader) => loader.create_project_file(path),
            Self::DirectGb(loader) => loader.create_project_file(path),
        }
    }

    pub(crate) fn replace_project_file(&self, path: &Path) -> Result<TasProject> {
        match self {
            Self::DirectNes(loader) => loader.replace_project_file(path),
            Self::DirectGb(loader) => loader.replace_project_file(path),
        }
    }
}

impl TasEditorExecutionProvider for PrivateTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        match self {
            Self::DirectNes(loader) => loader.load_editor_engine(project),
            Self::DirectGb(loader) => loader.load_editor_engine(project),
        }
    }
}

pub(crate) fn select_private_tas_execution_loader(
    source_path: PathBuf,
    system: ActiveSystem,
    firmware_search_dirs: Vec<PathBuf>,
) -> Result<PrivateTasExecutionLoader> {
    match system {
        ActiveSystem::Nes if has_extension(&source_path, "nes") => {
            Ok(PrivateTasExecutionLoader::DirectNes(
                DirectNesTasExecutionLoader::new(source_path, firmware_search_dirs),
            ))
        }
        ActiveSystem::GameBoy if has_extension(&source_path, "gb") => {
            Ok(PrivateTasExecutionLoader::DirectGb(
                DirectGbTasExecutionLoader::new(source_path, firmware_search_dirs),
            ))
        }
        ActiveSystem::Nes => bail!("NES TAS execution currently requires a direct .nes cartridge"),
        ActiveSystem::GameBoy => {
            bail!("Game Boy TAS execution currently requires a direct .gb cartridge")
        }
        _ => bail!("{system} does not have a TAS execution profile"),
    }
}

pub(crate) fn select_private_tas_execution_attachment(
    source_path: Option<PathBuf>,
    system: Option<ActiveSystem>,
    firmware_search_dirs: Vec<PathBuf>,
) -> TasEditorExecutionAttachment {
    let (Some(source_path), Some(system)) = (source_path, system) else {
        return TasEditorExecutionAttachment::Unavailable(
            TasEditorExecutionUnavailableReason::NoRunningEmulator,
        );
    };
    match select_private_tas_execution_loader(source_path, system, firmware_search_dirs) {
        Ok(loader) => TasEditorExecutionAttachment::Available(Box::new(loader)),
        Err(error) if matches!(system, ActiveSystem::Nes | ActiveSystem::GameBoy) => {
            TasEditorExecutionAttachment::Unavailable(
                TasEditorExecutionUnavailableReason::UnsupportedMedia(error.to_string()),
            )
        }
        Err(_) => TasEditorExecutionAttachment::Unavailable(
            TasEditorExecutionUnavailableReason::UnsupportedSystem(system.to_string()),
        ),
    }
}

fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|candidate| candidate.to_str())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
}

pub(super) fn publish_new_project(path: &Path, project: &TasProject) -> Result<()> {
    let bytes = project.encode()?;
    let read_limit = u64::try_from(bytes.len())?
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("TAS project validation bound overflows"))?;
    crate::platform::write_new_file_atomically_validated(path, &bytes, |temp_file| {
        temp_file.rewind()?;
        let mut temp_bytes = Vec::with_capacity(bytes.len());
        temp_file.take(read_limit).read_to_end(&mut temp_bytes)?;
        ensure!(
            temp_bytes.len() == bytes.len(),
            "temporary TAS project length changed during validation"
        );
        let decoded =
            TasProject::decode(&temp_bytes).context("temporary TAS project failed validation")?;
        ensure!(
            decoded == *project,
            "temporary TAS project changed project semantics"
        );
        Ok(())
    })
    .with_context(|| format!("failed to atomically create TAS project {}", path.display()))
}

pub(crate) fn direct_nes_tas_identity(
    backend: &EmuBackend,
    source_bytes: &[u8],
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    ensure!(
        backend.system() == ActiveSystem::Nes,
        "TAS execution profile requires a NES backend"
    );
    ensure!(
        backend
            .nes()
            .is_some_and(|nes| nes.has_standard_console_hardware()),
        "TAS execution profile requires ordinary NES console hardware"
    );
    let metadata = backend.replay_metadata();
    let system = metadata
        .system
        .context("NES backend omitted its system identity")?;
    let core_family = metadata
        .core_family
        .context("NES backend omitted its core-family identity")?;
    let effective_media_sha256 = TasDigest(
        metadata
            .rom_sha256
            .context("NES backend omitted its effective media identity")?,
    );
    let source_media_sha256 = TasDigest::from_bytes(source_bytes);
    ensure!(
        source_media_sha256 == effective_media_sha256,
        "cartridge NES loader changed media bytes without a declared patch chain"
    );
    ensure!(
        metadata.cheat_sha256.is_none(),
        "cartridge NES execution unexpectedly enabled cheats"
    );
    ensure!(
        metadata.firmware.is_empty(),
        "cartridge NES execution unexpectedly selected firmware"
    );
    Ok(TasProjectIdentity {
        system,
        core_family,
        determinism_abi: zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID.to_owned(),
        source_media_sha256,
        effective_media_sha256,
        patches: Vec::new(),
        firmware: metadata
            .firmware
            .iter()
            .map(tas_firmware_identity)
            .collect(),
        devices: direct_nes_tas_devices(),
        sync_config_sha256: direct_nes_tas_sync_config_sha256(),
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
            .to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

pub(crate) fn read_nes_cartridge_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("failed to open TAS source media {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_NES_CARTRIDGE_BYTES,
        "TAS source media exceeds the {MAX_NES_CARTRIDGE_BYTES}-byte cartridge limit"
    );
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_NES_CARTRIDGE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read TAS source media {}", path.display()))?;
    ensure!(
        bytes.len() as u64 <= MAX_NES_CARTRIDGE_BYTES,
        "TAS source media exceeds the {MAX_NES_CARTRIDGE_BYTES}-byte cartridge limit"
    );
    Ok(bytes)
}

pub(crate) fn validate_current_nes_start_state(start_state: &[u8]) -> Result<()> {
    ensure!(
        start_state.len() >= 12
            && start_state[..8] == zeff_nes_core::save_state::NES_SAVE_STATE_MAGIC,
        "TAS starting state is not a native NES save state"
    );
    let version = u32::from_le_bytes(start_state[8..12].try_into().expect("length checked above"));
    ensure!(
        version == zeff_nes_core::save_state::NES_SAVE_STATE_FORMAT_VERSION,
        "TAS execution requires native NES state format {}, got {version}",
        zeff_nes_core::save_state::NES_SAVE_STATE_FORMAT_VERSION
    );
    let mut projected = start_state.to_vec();
    zeff_nes_core::save_state::project_replay_state_bytes(&mut projected)
        .context("TAS starting state failed canonical NES validation")
}

pub(crate) fn validate_direct_nes_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::Nes.code()
            && identity.core_family == format!("{:?}", zeff_emu_common::system::CoreFamily::Nes),
        "TAS project does not identify the native NES core"
    );
    ensure!(
        identity.determinism_abi == zeff_nes_core::save_state::TAS_DETERMINISM_ABI_ID
            && identity.state_format_compatibility_id
                == zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        "TAS project uses an incompatible NES determinism or state format"
    );
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256
            && identity.patches.is_empty(),
        "TAS project media is outside the direct unmodified NES profile"
    );
    ensure!(
        identity.firmware.is_empty()
            && identity.devices == direct_nes_tas_devices()
            && identity.sync_config_sha256 == direct_nes_tas_sync_config_sha256(),
        "TAS project firmware, devices, or sync configuration is incompatible"
    );
    ensure!(
        identity.persistent_state == TasExternalIdentity::Absent
            && identity.rtc_state == TasExternalIdentity::Absent
            && identity.sensor_state == TasExternalIdentity::Absent
            && identity.cheats == TasExternalIdentity::Absent,
        "TAS project declares unsupported external state"
    );
    validate_current_nes_start_state(project.start_state())
}

fn validate_direct_nes_tas_branch_scope(project: &TasProject, branch_id: &str) -> Result<()> {
    let branch = project
        .branch(branch_id)
        .ok_or_else(|| anyhow::anyhow!("unknown TAS branch {branch_id:?}"))?;
    ensure!(
        project.replay_start() == &Default::default(),
        "cartridge NES TAS execution does not support replay start metadata"
    );
    ensure!(
        branch.events().is_empty(),
        "cartridge NES TAS execution does not support synchronized media or link events"
    );
    for span in branch.input_spans() {
        let input = span.input;
        if input.players[2..]
            .iter()
            .any(|player| player.buttons != 0 || player.dpad != 0)
        {
            bail!("cartridge NES TAS execution supports players 1 and 2 only");
        }
        if input.zapper.enabled
            || input.zapper.trigger
            || input.zapper.hit
            || input.zapper.screen_pos.is_some()
        {
            bail!("cartridge NES TAS execution does not support Zapper input");
        }
        if input.tilt_x_bits != 0 || input.tilt_y_bits != 0 {
            bail!("cartridge NES TAS execution does not support tilt input");
        }
        if input.camera != TasCameraInput::None {
            bail!("cartridge NES TAS execution does not support camera input");
        }
    }
    Ok(())
}

pub(super) fn tas_firmware_identity(firmware: &ReplayFirmwareManifest) -> TasFirmwareIdentity {
    match firmware {
        ReplayFirmwareManifest::External {
            firmware_id,
            variant,
            sha256,
        } => TasFirmwareIdentity::External {
            firmware_id: firmware_id.clone(),
            variant: variant.clone(),
            sha256: TasDigest(*sha256),
        },
        ReplayFirmwareManifest::Hle {
            firmware_id,
            implementation,
            compatibility_version,
        } => TasFirmwareIdentity::Hle {
            firmware_id: firmware_id.clone(),
            implementation: implementation.clone(),
            compatibility_version: *compatibility_version,
        },
        ReplayFirmwareManifest::BuiltinOpenSource {
            firmware_id,
            implementation,
            compatibility_version,
            sha256,
        } => TasFirmwareIdentity::BuiltinOpenSource {
            firmware_id: firmware_id.clone(),
            implementation: implementation.clone(),
            compatibility_version: *compatibility_version,
            sha256: TasDigest(*sha256),
        },
        ReplayFirmwareManifest::Skipped {
            firmware_id,
            compatibility_version,
        } => TasFirmwareIdentity::Skipped {
            firmware_id: firmware_id.clone(),
            compatibility_version: *compatibility_version,
        },
    }
}

#[cfg(test)]
mod creation_tests;

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use zeff_emu_common::replay::ReplayStartMetadata;

    use super::*;
    use crate::tas_project::{
        TasAutosaveConfig, TasAutosaveStore, TasControllerInput, TasEditorSession,
        TasInitialBranch, TasInputFrame, TasSeekStateCache,
    };

    #[test]
    fn private_attachment_selection_reports_current_capability_reasons() {
        assert!(matches!(
            select_private_tas_execution_attachment(None, None, Vec::new()),
            TasEditorExecutionAttachment::Unavailable(
                TasEditorExecutionUnavailableReason::NoRunningEmulator
            )
        ));
        assert!(matches!(
            select_private_tas_execution_attachment(
                Some(PathBuf::from("game.gb")),
                Some(ActiveSystem::GameBoy),
                Vec::new(),
            ),
            TasEditorExecutionAttachment::Available(_)
        ));
        assert!(matches!(
            select_private_tas_execution_attachment(
                Some(PathBuf::from("game.zip")),
                Some(ActiveSystem::Nes),
                Vec::new(),
            ),
            TasEditorExecutionAttachment::Unavailable(
                TasEditorExecutionUnavailableReason::UnsupportedMedia(_)
            )
        ));
        assert!(matches!(
            select_private_tas_execution_attachment(
                Some(PathBuf::from("game.zip")),
                Some(ActiveSystem::GameBoy),
                Vec::new(),
            ),
            TasEditorExecutionAttachment::Unavailable(
                TasEditorExecutionUnavailableReason::UnsupportedMedia(_)
            )
        ));
        assert!(matches!(
            select_private_tas_execution_attachment(
                Some(PathBuf::from("game.nes")),
                Some(ActiveSystem::Nes),
                Vec::new(),
            ),
            TasEditorExecutionAttachment::Available(_)
        ));
    }

    #[test]
    fn attached_direct_nes_profile_is_rechecked_after_a_later_edit() -> Result<()> {
        let directory = crate::test_support::test_directory("tas-loader-persistent-scope")?;
        let rom_path = directory.path().join("game.nes");
        let rom = crate::test_support::build_nes_test_rom();
        std::fs::write(&rom_path, &rom)?;

        let backend = load_backend_from_rom_source(
            ActiveSystem::Nes,
            &rom_path,
            &rom_path,
            Some(rom.clone()),
            BackendLoadConfig {
                sample_rate: None,
                apply_mods: false,
                initial_input: None,
                nes_load_battery_sram: false,
                ..BackendLoadConfig::default()
            },
        )?
        .backend;
        let start_state = backend.encode_state_bytes()?;
        let identity = direct_nes_tas_identity(&backend, &rom, &start_state)?;
        let project = TasProject::new(
            "persistent-direct-nes-scope",
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
        )?;
        let loader = DirectNesTasExecutionLoader::new(rom_path, Vec::new());
        let mut engine = loader.load_editor_engine(&project)?;
        let manual_path = directory.path().join("movie.ztas");
        let autosaves =
            TasAutosaveStore::beside_manual_save(&manual_path, TasAutosaveConfig::default())?;
        let seek_cache = TasSeekStateCache::open(directory.path().join("seek-cache"))?;
        let mut editor = TasEditorSession::new(project, manual_path, autosaves, seek_cache)?;

        editor.edit_transaction(|edit| {
            edit.set_input_range(
                "main",
                0,
                1,
                TasInputFrame {
                    players: [
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                        TasControllerInput {
                            buttons: 1,
                            dpad: 0,
                        },
                        TasControllerInput::default(),
                        TasControllerInput::default(),
                    ],
                    ..TasInputFrame::default()
                },
            )
        })?;

        let error = engine.seek(&mut editor, 1).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("attached editor execution profile")
        );
        assert_eq!(editor.cursor(), 0);
        assert!(editor.load_seek_state()?.is_none());
        Ok(())
    }
}
