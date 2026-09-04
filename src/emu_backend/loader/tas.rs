#![cfg(not(target_arch = "wasm32"))]

use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, bail, ensure};
use zeff_emu_common::replay::ReplayFirmwareManifest;
#[cfg(test)]
use zeff_emu_common::replay::ReplayStartMetadata;

use super::{BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::tas_project::verification::{TasExecutionSession, TasVerifiedReplayExportPhase};
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasEditorExecutionAttachment,
    TasEditorExecutionEngine, TasEditorExecutionProvider, TasEditorExecutionUnavailableReason,
    TasEditorSession, TasExecutionWitness, TasExternalIdentity, TasFirmwareIdentity,
    TasInitialBranch, TasProject, TasProjectIdentity, TasZrplImportWitness,
};

mod direct_coleco;
mod direct_coleco_loader;
mod direct_fds;
mod direct_fds_loader;
mod direct_game_gear;
mod direct_game_gear_loader;
mod direct_gb;
mod direct_gb_loader;
mod direct_gba_loader;
mod direct_gbc;
mod direct_nes_loader;
mod direct_pce;
mod direct_pce_cd;
mod direct_pce_cd_loader;
#[cfg(test)]
mod direct_pce_cd_replay_tests;
mod direct_pce_loader;
mod direct_sg1000;
mod direct_sg1000_loader;
mod direct_sms;
mod direct_sms_loader;
mod direct_ws;
mod direct_ws_loader;
mod gb_rtc;
mod live_binding;
mod media;
mod project_profile;
mod selection;
pub(crate) use direct_coleco::{
    direct_coleco_tas_identity, direct_coleco_tas_sync_config_sha256,
    validate_direct_coleco_tas_branch_scope, validate_direct_coleco_tas_execution_runtime,
    validate_direct_coleco_tas_project_identity, validate_direct_coleco_tas_project_witness,
    validate_direct_coleco_tas_runtime, validate_direct_coleco_tas_state,
    zip_coleco_tas_sync_config_sha256,
};
pub(crate) use direct_coleco_loader::DirectColecoTasExecutionLoader;
#[allow(unused_imports)]
pub(crate) use direct_fds::{
    MAX_FDS_IMAGE_BYTES, direct_fds_tas_sync_config_sha256, validate_fds_tas_branch_scope,
    validate_fds_tas_execution_runtime, validate_fds_tas_private_runtime,
    validate_fds_tas_project_identity, validate_fds_tas_project_witness,
    zip_fds_tas_sync_config_sha256,
};
pub(crate) use direct_fds_loader::DirectFdsTasExecutionLoader;
pub(crate) use direct_game_gear::{
    DirectGameGearTasBoardChoice, direct_game_gear_tas_sync_config_sha256,
    restore_direct_game_gear_tas_private_execution_state,
    validate_direct_game_gear_tas_execution_runtime,
    validate_direct_game_gear_tas_private_execution_runtime,
    validate_direct_game_gear_tas_private_runtime, validate_direct_game_gear_tas_private_state,
    validate_direct_game_gear_tas_project_identity, validate_direct_game_gear_tas_project_witness,
    validate_direct_game_gear_tas_runtime, validate_direct_game_gear_tas_state,
    zip_game_gear_tas_sync_config_sha256,
};
pub(crate) use direct_game_gear_loader::{
    DirectGameGearTasExecutionLoader, MAX_DIRECT_GAME_GEAR_ROM_BYTES,
};
#[cfg(test)]
pub(crate) use direct_game_gear_loader::{
    TestGameGearBoardCatalogGuard, register_test_game_gear_board_catalog_entry,
};
pub(crate) use direct_gb::{
    DirectGbTasRuntimeWitness, direct_gb_tas_sync_config_sha256,
    is_supported_direct_gb_tas_cartridge, validate_direct_gb_tas_project_identity,
    validate_direct_gb_tas_project_witness, validate_direct_gb_tas_runtime,
    validate_direct_gb_tas_runtime_with_project_rtc,
    validate_direct_gb_tas_runtime_with_project_sram, validate_direct_gb_tas_state,
    zip_gb_battery_tas_sync_config_sha256, zip_gb_tas_sync_config_sha256,
};
pub(crate) use direct_gb_loader::DirectGbTasExecutionLoader;
pub(crate) use direct_gba_loader::DirectGbaTasExecutionLoader;
pub(crate) use direct_gbc::{
    DirectGbcTasExecutionLoader, direct_gbc_tas_sync_config_sha256,
    validate_direct_gbc_state_for_backend, validate_direct_gbc_state_for_backend_with_project_rtc,
    validate_direct_gbc_state_for_backend_with_project_sram,
    validate_direct_gbc_tas_project_identity, validate_direct_gbc_tas_project_witness,
    validate_direct_gbc_tas_runtime, validate_direct_gbc_tas_runtime_with_project_rtc,
    validate_direct_gbc_tas_runtime_with_project_sram, zip_gbc_battery_tas_sync_config_sha256,
    zip_gbc_tas_sync_config_sha256,
};
pub(crate) use direct_nes_loader::DirectNesTasExecutionLoader;
pub(crate) use direct_pce::{
    direct_pce_tas_host_persistence_absent, direct_pce_tas_project_profile,
    direct_pce_tas_sync_config_sha256_for_profile,
    validate_direct_pce_multitap_tas_execution_runtime, validate_direct_pce_multitap_tas_runtime,
    validate_direct_pce_six_button_tas_execution_runtime,
    validate_direct_pce_six_button_tas_runtime, validate_direct_pce_tas_execution_runtime,
    validate_direct_pce_tas_project_identity, validate_direct_pce_tas_project_witness,
    validate_direct_pce_tas_runtime, validate_direct_pce_tas_state,
    zip_pce_tas_sync_config_sha256_for_profile,
};
#[cfg(test)]
pub(crate) use direct_pce_cd::direct_pce_cd_archive_ppf_tas_sync_configs_for_test;
pub(crate) use direct_pce_cd::{
    direct_pce_multitap_cd_ppf_tas_sync_config_sha256,
    is_direct_pce_cd_archive_ppf_tas_sync_config_sha256,
    validate_direct_pce_cd_tas_execution_runtime, validate_direct_pce_cd_tas_project_identity,
    validate_direct_pce_cd_tas_project_witness, validate_direct_pce_cd_tas_runtime,
    validate_direct_pce_cd_tas_state, validate_direct_pce_multitap_cd_tas_execution_runtime,
    validate_direct_pce_multitap_cd_tas_project_identity,
    validate_direct_pce_multitap_cd_tas_project_witness,
    validate_direct_pce_multitap_cd_tas_runtime, validate_direct_pce_multitap_cd_tas_state,
};
pub(crate) use direct_pce_cd_loader::DirectPceCdTasExecutionLoader;
#[cfg(test)]
pub(crate) use direct_pce_cd_loader::{
    register_test_pce_cd_ppf_stack, register_test_pce_cd_system_card,
};
pub(crate) use direct_pce_loader::{
    DirectPceTasExecutionLoader, MAX_DIRECT_PCE_HUCARD_BYTES, classify_direct_pce_tas_hardware,
};
pub(crate) use direct_sg1000::{
    direct_sg1000_tas_sync_config_sha256, validate_direct_sg1000_tas_execution_runtime,
    validate_direct_sg1000_tas_project_identity, validate_direct_sg1000_tas_project_witness,
    validate_direct_sg1000_tas_runtime, validate_direct_sg1000_tas_state,
    zip_sg1000_tas_sync_config_sha256,
};
pub(crate) use direct_sg1000_loader::{
    DirectSg1000TasExecutionLoader, MAX_DIRECT_SG1000_ROM_BYTES,
};
#[allow(unused_imports)]
pub(crate) use direct_sms::{
    direct_sms_tas_sync_config_sha256, validate_direct_sms_tas_execution_runtime,
    validate_direct_sms_tas_project_identity, validate_direct_sms_tas_project_witness,
    validate_direct_sms_tas_runtime, validate_direct_sms_tas_state, zip_sms_tas_sync_config_sha256,
};
#[allow(unused_imports)]
pub(crate) use direct_sms_loader::{DirectSmsTasExecutionLoader, MAX_DIRECT_SMS_ROM_BYTES};
#[allow(unused_imports)]
pub(crate) use direct_ws::{
    direct_ws_battery_tas_sync_config_sha256, direct_ws_rtc_tas_sync_config_sha256,
    direct_ws_tas_orientation, direct_ws_tas_sync_config_sha256,
    validate_direct_ws_tas_branch_scope, validate_direct_ws_tas_execution_runtime,
    validate_direct_ws_tas_linked_runtime, validate_direct_ws_tas_private_execution_runtime,
    validate_direct_ws_tas_private_runtime, validate_direct_ws_tas_private_state,
    validate_direct_ws_tas_project_identity, validate_direct_ws_tas_project_witness,
    validate_direct_ws_tas_runtime, validate_direct_ws_tas_state,
    zip_ws_rtc_tas_sync_config_sha256, zip_ws_tas_sync_config_sha256,
};
pub(crate) use direct_ws_loader::{DirectWsTasExecutionLoader, MAX_DIRECT_WS_ROM_BYTES};
pub(crate) use gb_rtc::{
    GbRtcPersistenceWitness, gb_rtc_complete_persistence_bytes, gb_rtc_persistence_witness,
};
pub(crate) use live_binding::{
    DirectNesTasRuntimeWitness, validate_direct_nes_tas_project_witness,
};
pub(crate) use project_profile::{
    TasProjectRuntimeWitness, classify_direct_tas_execution_profile, validate_tas_project_witness,
};
pub(crate) use selection::{
    has_extension, select_private_tas_execution_attachment, select_private_tas_execution_loader,
    select_private_tas_execution_loader_for_project,
    select_private_tas_execution_loader_for_replay,
    select_private_tas_execution_loader_with_rom_path,
};

pub(crate) const MAX_NES_CARTRIDGE_BYTES: u64 = 64 * 1024 * 1024;
const NES_GAMEPAD_CONFIGURATION: &[u8] = b"zeff-tas-device-config-v1\0nes-standard-controller\0";
const NES_ZAPPER_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0nes-standard-or-zapper-controller\0";
const NES_CARTRIDGE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0nes-cartridge\0mods=disabled\0initial-input=neutral\0sample-rate=core-default\0external-state=absent\0";
const NES_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0nes-cartridge\0mods=disabled\0initial-input=neutral\0sample-rate=core-default\0persistent-state=project-owned-sram\0rtc=absent\0sensors=absent\0";
const NES_ZIP_MEMBER_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0nes-zip-member\0mods=disabled\0initial-input=neutral\0sample-rate=core-default\0external-state=absent\0member=";
const NES_ZIP_BATTERY_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0nes-zip-member\0mods=disabled\0initial-input=neutral\0sample-rate=core-default\0persistent-state=project-owned-sram\0rtc=absent\0sensors=absent\0member=";
pub(crate) const MAX_NES_ZIP_BYTES: u64 = 128 * 1024 * 1024;

pub(crate) fn direct_nes_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(NES_CARTRIDGE_SYNC_CONFIGURATION)
}

pub(crate) fn direct_nes_battery_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(NES_BATTERY_SYNC_CONFIGURATION)
}

pub(crate) fn zip_nes_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    zip_nes_tas_sync_config_sha256_for_profile(member_name, false)
}

pub(crate) fn zip_nes_battery_tas_sync_config_sha256(member_name: &str) -> TasDigest {
    zip_nes_tas_sync_config_sha256_for_profile(member_name, true)
}

fn zip_nes_tas_sync_config_sha256_for_profile(member_name: &str, battery: bool) -> TasDigest {
    let configuration = if battery {
        NES_ZIP_BATTERY_SYNC_CONFIGURATION
    } else {
        NES_ZIP_MEMBER_SYNC_CONFIGURATION
    };
    let mut bytes = Vec::with_capacity(configuration.len() + member_name.len());
    bytes.extend_from_slice(configuration);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
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

fn direct_nes_zapper_tas_devices() -> Vec<TasDeviceIdentity> {
    vec![
        TasDeviceIdentity {
            port: "p1".to_owned(),
            device: "nes-standard-controller".to_owned(),
            configuration_sha256: TasDigest::from_bytes(NES_GAMEPAD_CONFIGURATION),
        },
        TasDeviceIdentity {
            port: "p2".to_owned(),
            device: "nes-standard-or-zapper-controller".to_owned(),
            configuration_sha256: TasDigest::from_bytes(NES_ZAPPER_CONFIGURATION),
        },
    ]
}

fn direct_nes_devices_for_backend(backend: &EmuBackend) -> Result<Vec<TasDeviceIdentity>> {
    if backend.nes_has_standard_controller_topology() == Some(true) {
        Ok(direct_nes_tas_devices())
    } else if backend.nes_has_standard_or_zapper_controller_topology() == Some(true) {
        Ok(direct_nes_zapper_tas_devices())
    } else {
        bail!("TAS starting state restores an unsupported NES controller topology")
    }
}

#[derive(Clone, Debug)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum PrivateTasExecutionLoader {
    DirectNes(DirectNesTasExecutionLoader),
    DirectFds(DirectFdsTasExecutionLoader),
    DirectGb(DirectGbTasExecutionLoader),
    DirectGbc(DirectGbcTasExecutionLoader),
    DirectColeco(DirectColecoTasExecutionLoader),
    DirectSms(DirectSmsTasExecutionLoader),
    DirectGameGear(DirectGameGearTasExecutionLoader),
    DirectGba(DirectGbaTasExecutionLoader),
    DirectSg1000(DirectSg1000TasExecutionLoader),
    DirectWs(DirectWsTasExecutionLoader),
    DirectPce(DirectPceTasExecutionLoader),
    DirectPceCd(DirectPceCdTasExecutionLoader),
}

impl PrivateTasExecutionLoader {
    pub(crate) fn load_repair_backend(&self, project: &TasProject) -> Result<EmuBackend> {
        self.load_editor_engine(project)
            .map(TasEditorExecutionEngine::into_backend)
    }

    pub(crate) fn validate_project_branch_scope(
        &self,
        project: &TasProject,
        branch_id: &str,
    ) -> Result<()> {
        match self {
            Self::DirectNes(_) => {
                DirectNesTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectFds(_) => {
                DirectFdsTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectGb(_) => {
                DirectGbTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectGbc(_) => {
                DirectGbcTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectColeco(_) => {
                DirectColecoTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectSms(_) => {
                DirectSmsTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectGameGear(_) => {
                DirectGameGearTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectGba(_) => {
                DirectGbaTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectSg1000(_) => {
                DirectSg1000TasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectWs(_) => {
                DirectWsTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectPce(_) => {
                DirectPceTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
            Self::DirectPceCd(_) => {
                DirectPceCdTasExecutionLoader::validate_project_branch_scope(project, branch_id)
            }
        }
    }

    pub(crate) fn create_project_file(&self, path: &Path) -> Result<TasProject> {
        match self {
            Self::DirectNes(loader) => loader.create_project_file(path),
            Self::DirectFds(loader) => loader.create_project_file(path),
            Self::DirectGb(loader) => loader.create_project_file(path),
            Self::DirectGbc(loader) => loader.create_project_file(path),
            Self::DirectColeco(loader) => loader.create_project_file(path),
            Self::DirectSms(loader) => loader.create_project_file(path),
            Self::DirectGameGear(loader) => loader.create_project_file(path),
            Self::DirectGba(loader) => loader.create_project_file(path),
            Self::DirectSg1000(loader) => loader.create_project_file(path),
            Self::DirectWs(loader) => loader.create_project_file(path),
            Self::DirectPce(loader) => loader.create_project_file(path),
            Self::DirectPceCd(loader) => loader.create_project_file(path),
        }
    }

    pub(crate) fn replace_project_file(&self, path: &Path) -> Result<TasProject> {
        match self {
            Self::DirectNes(loader) => loader.replace_project_file(path),
            Self::DirectFds(loader) => loader.replace_project_file(path),
            Self::DirectGb(loader) => loader.replace_project_file(path),
            Self::DirectGbc(loader) => loader.replace_project_file(path),
            Self::DirectColeco(loader) => loader.replace_project_file(path),
            Self::DirectSms(loader) => loader.replace_project_file(path),
            Self::DirectGameGear(loader) => loader.replace_project_file(path),
            Self::DirectGba(loader) => loader.replace_project_file(path),
            Self::DirectSg1000(loader) => loader.replace_project_file(path),
            Self::DirectWs(loader) => loader.replace_project_file(path),
            Self::DirectPce(loader) => loader.replace_project_file(path),
            Self::DirectPceCd(loader) => loader.replace_project_file(path),
        }
    }

    pub(crate) fn import_replay_file(
        &self,
        replay_path: &Path,
        project_path: &Path,
        replace_existing: bool,
    ) -> Result<TasProject> {
        ensure!(
            TasProject::is_project_path(project_path),
            "TAS projects must use the .ztas extension"
        );
        let project =
            TasProject::import_zrpl_with_witness_and_assets(replay_path, |start_state| {
                if let Self::DirectFds(loader) = self {
                    return loader.replay_import_witness(start_state);
                }
                let (prefix, session) = match self {
                    Self::DirectNes(loader) => ("nes", loader.load_session(start_state)?),
                    Self::DirectFds(_) => unreachable!("FDS replay import handled above"),
                    Self::DirectGb(loader) => ("gb", loader.load_session(start_state)?),
                    Self::DirectGbc(loader) => ("gbc", loader.load_session(start_state)?),
                    Self::DirectColeco(loader) => ("coleco", loader.load_session(start_state)?),
                    Self::DirectSms(loader) => ("sms", loader.load_session(start_state)?),
                    Self::DirectGameGear(loader) => {
                        ("game-gear", loader.load_session(start_state)?)
                    }
                    Self::DirectGba(loader) => ("gba", loader.load_session(start_state)?),
                    Self::DirectSg1000(loader) => ("sg1000", loader.load_session(start_state)?),
                    Self::DirectWs(loader) => ("ws", loader.load_session(start_state)?),
                    Self::DirectPce(loader) => ("pce", loader.load_session(start_state)?),
                    Self::DirectPceCd(loader) => ("pce-cd", loader.load_session(start_state)?),
                };
                let identity = session.identity().clone();
                Ok((
                    TasZrplImportWitness {
                        project_id: format!("{prefix}-{}", identity.source_media_sha256.to_hex()),
                        identity,
                    },
                    std::collections::BTreeMap::new(),
                ))
            })?;
        for branch in project.branches() {
            self.validate_project_branch_scope(&project, branch.id())?;
        }
        if replace_existing {
            ensure!(
                project_path.exists(),
                "TAS project destination does not exist"
            );
            project.save_atomic(project_path).with_context(|| {
                format!(
                    "failed to atomically replace TAS project {}",
                    project_path.display()
                )
            })?;
        } else {
            publish_new_project(project_path, &project)?;
        }
        Ok(project)
    }

    #[cfg(test)]
    pub(crate) fn verify_and_export_editor_session(
        &self,
        session: &mut TasEditorSession,
        replay_path: &Path,
    ) -> Result<PathBuf> {
        let start_state = session.project().start_state().to_vec();
        let witness_session = self.load_session(&start_state)?;
        let witness = TasExecutionWitness {
            identity: witness_session.identity().clone(),
        };
        session.verify_save_and_export_active_branch(replay_path, &witness, || {
            self.load_session(&start_state)
        })
    }

    pub(crate) fn verify_and_export_editor_session_cancellable(
        &self,
        session: &mut TasEditorSession,
        replay_path: &Path,
        cancellation: &AtomicBool,
        progress: &mut impl FnMut(TasVerifiedReplayExportPhase),
    ) -> Result<PathBuf> {
        progress(TasVerifiedReplayExportPhase::Preparing);
        let start_state = session.project().start_state().to_vec();
        let witness_session = self.load_session(&start_state)?;
        let witness = TasExecutionWitness {
            identity: witness_session.identity().clone(),
        };
        session.verify_save_and_export_active_branch_cancellable(
            replay_path,
            &witness,
            cancellation,
            progress,
            || self.load_session(&start_state),
        )
    }

    pub(crate) fn load_session(&self, start_state: &[u8]) -> Result<TasExecutionSession> {
        match self {
            Self::DirectNes(loader) => loader.load_session(start_state),
            Self::DirectFds(loader) => loader.load_session(start_state),
            Self::DirectGb(loader) => loader.load_session(start_state),
            Self::DirectGbc(loader) => loader.load_session(start_state),
            Self::DirectColeco(loader) => loader.load_session(start_state),
            Self::DirectSms(loader) => loader.load_session(start_state),
            Self::DirectGameGear(loader) => loader.load_session(start_state),
            Self::DirectGba(loader) => loader.load_session(start_state),
            Self::DirectSg1000(loader) => loader.load_session(start_state),
            Self::DirectWs(loader) => loader.load_session(start_state),
            Self::DirectPce(loader) => loader.load_session(start_state),
            Self::DirectPceCd(loader) => loader.load_session(start_state),
        }
    }
}

impl TasEditorExecutionProvider for PrivateTasExecutionLoader {
    fn load_editor_engine(&self, project: &TasProject) -> Result<TasEditorExecutionEngine> {
        match self {
            Self::DirectNes(loader) => loader.load_editor_engine(project),
            Self::DirectFds(loader) => loader.load_editor_engine(project),
            Self::DirectGb(loader) => loader.load_editor_engine(project),
            Self::DirectGbc(loader) => loader.load_editor_engine(project),
            Self::DirectColeco(loader) => loader.load_editor_engine(project),
            Self::DirectSms(loader) => loader.load_editor_engine(project),
            Self::DirectGameGear(loader) => loader.load_editor_engine(project),
            Self::DirectGba(loader) => loader.load_editor_engine(project),
            Self::DirectSg1000(loader) => loader.load_editor_engine(project),
            Self::DirectWs(loader) => loader.load_editor_engine(project),
            Self::DirectPce(loader) => loader.load_editor_engine(project),
            Self::DirectPceCd(loader) => loader.load_editor_engine(project),
        }
    }
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
    let source_media_sha256 = TasDigest::from_bytes(source_bytes);
    let persistent_state = nes_persistent_identity(backend)?;
    let sync_config_sha256 = match persistent_state {
        TasExternalIdentity::Absent => direct_nes_tas_sync_config_sha256(),
        TasExternalIdentity::ExternalSha256(_) => direct_nes_battery_tas_sync_config_sha256(),
    };
    let identity = nes_tas_identity(
        backend,
        source_media_sha256,
        sync_config_sha256,
        persistent_state,
        start_state,
    )?;
    ensure!(
        identity.source_media_sha256 == identity.effective_media_sha256,
        "cartridge NES loader changed media bytes without a declared patch chain"
    );
    Ok(identity)
}

pub(crate) fn zip_nes_tas_identity(
    backend: &EmuBackend,
    archive_sha256: [u8; 32],
    member_name: &str,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let persistent_state = nes_persistent_identity(backend)?;
    let sync_config_sha256 = match persistent_state {
        TasExternalIdentity::Absent => zip_nes_tas_sync_config_sha256(member_name),
        TasExternalIdentity::ExternalSha256(_) => {
            zip_nes_battery_tas_sync_config_sha256(member_name)
        }
    };
    nes_tas_identity(
        backend,
        TasDigest(archive_sha256),
        sync_config_sha256,
        persistent_state,
        start_state,
    )
}

fn nes_tas_identity(
    backend: &EmuBackend,
    source_media_sha256: TasDigest,
    sync_config_sha256: TasDigest,
    persistent_state: TasExternalIdentity,
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
        devices: direct_nes_devices_for_backend(backend)?,
        sync_config_sha256,
        persistent_state,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id: zeff_nes_core::save_state::TAS_STATE_FORMAT_COMPATIBILITY_ID
            .to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

fn nes_persistent_identity(backend: &EmuBackend) -> Result<TasExternalIdentity> {
    let nes = backend
        .nes()
        .context("TAS execution profile requires a NES backend")?;
    let battery = nes.emu.dump_battery_sram();
    ensure!(
        battery.is_some() == backend.save_ram_kind().is_battery_backed(),
        "NES battery state disagrees with the cartridge save profile"
    );
    Ok(match battery {
        Some(bytes) => TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&bytes)),
        None => TasExternalIdentity::Absent,
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
    let direct_media = identity.source_media_sha256 == identity.effective_media_sha256
        && match identity.persistent_state {
            TasExternalIdentity::Absent => {
                identity.sync_config_sha256 == direct_nes_tas_sync_config_sha256()
            }
            TasExternalIdentity::ExternalSha256(_) => {
                identity.sync_config_sha256 == direct_nes_battery_tas_sync_config_sha256()
            }
        };
    let zip_media = identity.source_media_sha256 != identity.effective_media_sha256
        && identity.sync_config_sha256 != direct_nes_tas_sync_config_sha256()
        && identity.sync_config_sha256 != direct_nes_battery_tas_sync_config_sha256();
    ensure!(
        (direct_media || zip_media) && identity.patches.is_empty(),
        "TAS project media is outside the unmodified NES profile"
    );
    let devices_supported = identity.devices == direct_nes_tas_devices()
        || identity.devices == direct_nes_zapper_tas_devices();
    ensure!(
        identity.firmware.is_empty() && devices_supported,
        "TAS project firmware, devices, or sync configuration is incompatible"
    );
    ensure!(
        identity.rtc_state == TasExternalIdentity::Absent
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
    let supports_zapper = project.identity().devices == direct_nes_zapper_tas_devices();
    for span in branch.input_spans() {
        let input = span.input;
        if input.players[2..]
            .iter()
            .any(|player| player.buttons != 0 || player.dpad != 0)
        {
            bail!("cartridge NES TAS execution supports players 1 and 2 only");
        }
        if !supports_zapper
            && (input.zapper.enabled
                || input.zapper.trigger
                || input.zapper.hit
                || input.zapper.screen_pos.is_some())
        {
            bail!("this cartridge NES TAS project does not declare Zapper input support");
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
mod direct_coleco_loader_tests;
#[cfg(test)]
mod selection_tests;
