use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::FrameLifecycle;
use zeff_pce_core::hardware::{
    CDROM2_BRAM_LEN, PadButtons, PceArcadeCardMode, PceCartridgeHardware, PceConsoleWiring,
    PceControllerMode, PceHardwareTopology, PceHuCardBoard, PceMemoryBaseMode, PsgRevision,
};

use super::{ActiveSystem, EmuBackend, TasProjectRuntimeWitness, tas_firmware_identity};
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasFirmwareIdentity,
    TasProject, TasProjectIdentity,
};

const PCE_CD_TWO_BUTTON_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0pce-two-button-controller\0";
const PCE_CD_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_CHD_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-chd\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_ISO_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-iso\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_PPF_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-ppf\0raw-source=base-disc-plus-ordered-ppf-sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=ordered-ppf-exact\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-arcade-card\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_CHD_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-chd-arcade-card\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_ISO_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-iso-arcade-card\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-memory-base-128\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=enabled-exact-catalog\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_CHD_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-chd-memory-base-128\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=enabled-exact-catalog\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_ISO_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-iso-memory-base-128\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=enabled-exact-catalog\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_PPF_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-ppf-memory-base-128\0raw-source=base-disc-plus-ordered-ppf-sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=enabled-exact-source-catalog\0arcade-card=disabled\0mods=ordered-ppf-exact\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";

pub(crate) fn direct_pce_cd_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_cd_chd_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_CHD_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_cd_iso_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_ISO_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_cd_ppf_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_PPF_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_cd_arcade_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_ARCADE_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_cd_chd_arcade_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_CHD_ARCADE_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_cd_iso_arcade_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_ISO_ARCADE_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_cd_memory_base_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_MEMORY_BASE_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_cd_chd_memory_base_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_CHD_MEMORY_BASE_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_cd_iso_memory_base_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_ISO_MEMORY_BASE_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_cd_ppf_memory_base_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_PPF_MEMORY_BASE_SYNC_CONFIGURATION)
}

fn sync_config_for_runtime(
    chd: bool,
    iso: bool,
    ppf: bool,
    arcade_card: bool,
    memory_base: bool,
) -> TasDigest {
    if chd {
        if arcade_card {
            direct_pce_cd_chd_arcade_tas_sync_config_sha256()
        } else if memory_base {
            direct_pce_cd_chd_memory_base_tas_sync_config_sha256()
        } else {
            direct_pce_cd_chd_tas_sync_config_sha256()
        }
    } else if iso {
        if arcade_card {
            direct_pce_cd_iso_arcade_tas_sync_config_sha256()
        } else if memory_base {
            direct_pce_cd_iso_memory_base_tas_sync_config_sha256()
        } else {
            direct_pce_cd_iso_tas_sync_config_sha256()
        }
    } else if ppf {
        if memory_base {
            direct_pce_cd_ppf_memory_base_tas_sync_config_sha256()
        } else {
            direct_pce_cd_ppf_tas_sync_config_sha256()
        }
    } else if arcade_card {
        direct_pce_cd_arcade_tas_sync_config_sha256()
    } else if memory_base {
        direct_pce_cd_memory_base_tas_sync_config_sha256()
    } else {
        direct_pce_cd_tas_sync_config_sha256()
    }
}

fn arcade_sync_config(sync_config_sha256: TasDigest) -> bool {
    sync_config_sha256 == direct_pce_cd_arcade_tas_sync_config_sha256()
        || sync_config_sha256 == direct_pce_cd_chd_arcade_tas_sync_config_sha256()
        || sync_config_sha256 == direct_pce_cd_iso_arcade_tas_sync_config_sha256()
}

fn memory_base_sync_config(sync_config_sha256: TasDigest) -> bool {
    sync_config_sha256 == direct_pce_cd_memory_base_tas_sync_config_sha256()
        || sync_config_sha256 == direct_pce_cd_chd_memory_base_tas_sync_config_sha256()
        || sync_config_sha256 == direct_pce_cd_iso_memory_base_tas_sync_config_sha256()
        || sync_config_sha256 == direct_pce_cd_ppf_memory_base_tas_sync_config_sha256()
}

fn memory_base_chd_sync_config(sync_config_sha256: TasDigest) -> bool {
    sync_config_sha256 == direct_pce_cd_chd_memory_base_tas_sync_config_sha256()
}

fn memory_base_iso_sync_config(sync_config_sha256: TasDigest) -> bool {
    sync_config_sha256 == direct_pce_cd_iso_memory_base_tas_sync_config_sha256()
}

fn memory_base_cue_sync_config(sync_config_sha256: TasDigest) -> bool {
    sync_config_sha256 == direct_pce_cd_memory_base_tas_sync_config_sha256()
}

fn memory_base_ppf_sync_config(sync_config_sha256: TasDigest) -> bool {
    sync_config_sha256 == direct_pce_cd_ppf_memory_base_tas_sync_config_sha256()
}

pub(crate) fn direct_pce_cd_arcade_eligible(ppf: bool, normalized_disc_sha256: [u8; 32]) -> bool {
    !ppf && crate::emu_backend::pce_profiles::automatic_arcade_card_enabled(Some(
        normalized_disc_sha256,
    ))
}

pub(crate) fn direct_pce_cd_memory_base_eligible(
    chd: bool,
    iso: bool,
    ppf: bool,
    normalized_disc_sha256: [u8; 32],
) -> bool {
    matches!(
        (chd, iso, ppf),
        (false, false, false) | (true, false, false) | (false, true, false) | (false, false, true)
    ) && crate::emu_backend::pce_profiles::automatic_memory_base_enabled(Some(
        normalized_disc_sha256,
    )) && crate::emu_backend::pce_profiles::automatic_controller_mode(normalized_disc_sha256)
        == PceControllerMode::TwoButton
}

pub(crate) fn direct_pce_cd_chd_source_identity(
    raw_source_sha256: [u8; 32],
    raw_source_len: usize,
) -> TasDigest {
    let mut hasher = Sha256::new();
    hasher.update(b"zeff-tas-pce-cd-chd-source:v1\0");
    hasher.update(raw_source_sha256);
    hasher.update((raw_source_len as u64).to_le_bytes());
    TasDigest(hasher.finalize().into())
}

pub(crate) fn direct_pce_cd_iso_source_identity(
    raw_source_sha256: [u8; 32],
    raw_source_len: usize,
) -> TasDigest {
    let mut hasher = Sha256::new();
    hasher.update(b"zeff-tas-pce-cd-iso-source:v1\0");
    hasher.update(raw_source_sha256);
    hasher.update((raw_source_len as u64).to_le_bytes());
    TasDigest(hasher.finalize().into())
}

fn devices() -> Vec<TasDeviceIdentity> {
    vec![TasDeviceIdentity {
        port: "p1".to_owned(),
        device: "pce-two-button-controller".to_owned(),
        configuration_sha256: TasDigest::from_bytes(PCE_CD_TWO_BUTTON_CONFIGURATION),
    }]
}

fn firmware_profile_is_supported(sha256: [u8; 32]) -> bool {
    #[cfg(test)]
    if sha256
        == [
            0x8A, 0x39, 0xD2, 0xAB, 0xD3, 0x99, 0x9A, 0xB7, 0x3C, 0x34, 0xDB, 0x24, 0x76, 0x84,
            0x9C, 0xDD, 0xF3, 0x03, 0xCE, 0x38, 0x9B, 0x35, 0x82, 0x68, 0x50, 0xF9, 0xA7, 0x00,
            0x58, 0x9B, 0x4A, 0x90,
        ]
    {
        return true;
    }
    zeff_firmware::classify_pce_system_card_sha256(sha256).is_some_and(|profile| {
        profile.region() == zeff_firmware::PceSystemCardRegion::Japan
            && profile.tier() == zeff_firmware::PceSystemCardTier::Version3
            && profile.board() == zeff_firmware::PceSystemCardBoard::SuperCdRom2
    })
}

fn firmware(
    backend: &EmuBackend,
    system_card_sha256: [u8; 32],
) -> Result<Vec<TasFirmwareIdentity>> {
    let pce = backend
        .pce()
        .context("PC Engine backend became unavailable")?;
    let manifests = pce.firmware_manifests();
    ensure!(
        manifests.len() == 1,
        "PC Engine CD TAS requires one System Card firmware"
    );
    let zeff_emu_common::replay::ReplayFirmwareManifest::External {
        firmware_id,
        sha256,
        ..
    } = &manifests[0]
    else {
        anyhow::bail!("PC Engine CD TAS requires exact external System Card firmware");
    };
    ensure!(
        firmware_id == "nec.pce.cd.system_card"
            && *sha256 == system_card_sha256
            && firmware_profile_is_supported(*sha256),
        "PC Engine CD TAS requires exact Japanese Super System Card v3 firmware"
    );
    Ok(vec![tas_firmware_identity(&manifests[0])])
}

pub(crate) fn direct_pce_cd_tas_identity(
    backend: &EmuBackend,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    let inspection = validate_direct_pce_cd_tas_runtime(backend, false)?;
    let metadata = backend.replay_metadata();
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "PC Engine CD TAS start state differs from the loaded baseline"
    );
    let state = backend
        .pce()
        .context("PC Engine backend became unavailable")?
        .inspect_current_native_cd_tas_state_for_profile(
            start_state,
            inspection.arcade_card_enabled,
            inspection.memory_base_enabled,
        )?;
    ensure!(
        state.system_card_sha256 == inspection.system_card_sha256
            && state.disc_sha256 == inspection.disc_sha256,
        "PC Engine CD TAS start state identity differs from the loaded core"
    );
    let provenance = backend
        .pce()
        .and_then(crate::emu_backend::PceBackend::tas_load_provenance)
        .context("PC Engine backend omitted CD load provenance")?;
    Ok(TasProjectIdentity {
        system: metadata
            .system
            .context("PC Engine backend omitted its system identity")?,
        core_family: metadata
            .core_family
            .context("PC Engine backend omitted its core-family identity")?,
        determinism_abi: zeff_pce_core::hardware::save_state::tas::TAS_DETERMINISM_ABI_ID
            .to_owned(),
        source_media_sha256: TasDigest(provenance.load.tas_source_media_sha256),
        effective_media_sha256: TasDigest(inspection.disc_sha256),
        patches: Vec::new(),
        firmware: firmware(backend, inspection.system_card_sha256)?,
        devices: devices(),
        sync_config_sha256: sync_config_for_runtime(
            provenance.load.direct_pce_cd_chd,
            provenance.load.direct_pce_cd_iso,
            provenance.load.direct_pce_cd_ppf,
            inspection.arcade_card_enabled,
            inspection.memory_base_enabled,
        ),
        persistent_state: TasExternalIdentity::Absent,
        rtc_state: TasExternalIdentity::Absent,
        sensor_state: TasExternalIdentity::Absent,
        cheats: TasExternalIdentity::Absent,
        state_format_compatibility_id:
            zeff_pce_core::hardware::save_state::tas::TAS_STATE_FORMAT_COMPATIBILITY_ID.to_owned(),
        start_state_sha256: TasDigest::from_bytes(start_state),
    })
}

pub(crate) fn validate_direct_pce_cd_tas_project_identity(project: &TasProject) -> Result<()> {
    let identity = project.identity();
    ensure!(
        identity.system == ActiveSystem::Pce.code()
            && identity.core_family
                == format!("{:?}", zeff_emu_common::system::CoreFamily::PcEngine)
            && identity.determinism_abi
                == zeff_pce_core::hardware::save_state::tas::TAS_DETERMINISM_ABI_ID
            && identity.state_format_compatibility_id
                == zeff_pce_core::hardware::save_state::tas::TAS_STATE_FORMAT_COMPATIBILITY_ID,
        "TAS project does not identify the current native PC Engine core"
    );
    let arcade_card = arcade_sync_config(identity.sync_config_sha256);
    let memory_base = memory_base_sync_config(identity.sync_config_sha256);
    let memory_base_chd = memory_base_chd_sync_config(identity.sync_config_sha256);
    let memory_base_iso = memory_base_iso_sync_config(identity.sync_config_sha256);
    let memory_base_ppf = memory_base_ppf_sync_config(identity.sync_config_sha256);
    let state = crate::emu_backend::pce::inspect_pce_cd_tas_state_identity_for_arcade_card(
        project.start_state(),
        arcade_card,
    )?;
    let cue = identity.sync_config_sha256 == direct_pce_cd_tas_sync_config_sha256()
        || identity.sync_config_sha256 == direct_pce_cd_arcade_tas_sync_config_sha256()
        || memory_base_cue_sync_config(identity.sync_config_sha256);
    ensure!(
        state.board == PceHuCardBoard::SystemCardV3
            && TasDigest(state.disc_sha256) == identity.effective_media_sha256
            && (!arcade_card || direct_pce_cd_arcade_eligible(false, state.disc_sha256))
            && (!memory_base
                || memory_base_ppf
                || direct_pce_cd_memory_base_eligible(
                    memory_base_chd,
                    memory_base_iso,
                    false,
                    state.disc_sha256,
                ))
            && !(arcade_card && memory_base)
            && (identity.sync_config_sha256 == direct_pce_cd_tas_sync_config_sha256()
                || identity.sync_config_sha256 == direct_pce_cd_chd_tas_sync_config_sha256()
                || identity.sync_config_sha256 == direct_pce_cd_iso_tas_sync_config_sha256()
                || identity.sync_config_sha256 == direct_pce_cd_ppf_tas_sync_config_sha256()
                || arcade_sync_config(identity.sync_config_sha256)
                || memory_base_sync_config(identity.sync_config_sha256))
            && (!cue || identity.source_media_sha256 == identity.effective_media_sha256)
            && (!memory_base_ppf
                || identity.source_media_sha256 != identity.effective_media_sha256)
            && identity.patches.is_empty()
            && identity.devices == devices()
            && identity.firmware.len() == 1,
        "TAS project media, hardware, or sync configuration is incompatible"
    );
    let TasFirmwareIdentity::External {
        firmware_id,
        sha256,
        ..
    } = &identity.firmware[0]
    else {
        anyhow::bail!("PC Engine CD TAS project requires external System Card firmware");
    };
    ensure!(
        firmware_id == "nec.pce.cd.system_card"
            && sha256.0 == state.system_card_sha256
            && firmware_profile_is_supported(sha256.0),
        "PC Engine CD TAS project requires exact Japanese Super System Card v3 firmware"
    );
    ensure!(
        identity.persistent_state == TasExternalIdentity::Absent
            && identity.rtc_state == TasExternalIdentity::Absent
            && identity.sensor_state == TasExternalIdentity::Absent
            && identity.cheats == TasExternalIdentity::Absent
            && TasDigest::from_bytes(project.start_state()) == identity.start_state_sha256,
        "PC Engine CD TAS project declares unsupported external state"
    );
    Ok(())
}

pub(crate) fn validate_direct_pce_cd_tas_branch_scope(
    project: &TasProject,
    branch_id: &str,
) -> Result<()> {
    validate_direct_pce_cd_tas_project_identity(project)?;
    ensure!(
        project.replay_start() == &Default::default(),
        "PC Engine CD TAS execution does not support replay start metadata"
    );
    let branch = project
        .branch(branch_id)
        .with_context(|| format!("unknown TAS branch {branch_id:?}"))?;
    ensure!(
        branch.events().is_empty(),
        "PC Engine CD TAS execution does not support replay events"
    );
    for span in branch.input_spans() {
        let input = span.input;
        ensure!(
            input.players[0].buttons & !0x0F == 0
                && input.players[0].dpad & !0x0F == 0
                && input.players[1..]
                    .iter()
                    .all(|player| *player == Default::default())
                && input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
                && input.zapper == Default::default()
                && input.tilt_x_bits == 0
                && input.tilt_y_bits == 0
                && matches!(input.camera, TasCameraInput::None),
            "PC Engine CD TAS execution supports one two-button controller only"
        );
    }
    Ok(())
}

pub(crate) fn validate_direct_pce_cd_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_direct_pce_cd_tas_branch_scope(project, branch_id)?;
    let identity = project.identity();
    ensure!(
        witness.source_media_sha256 == identity.source_media_sha256
            && witness.effective_media_sha256 == identity.effective_media_sha256
            && witness.determinism_abi == identity.determinism_abi
            && witness.state_format_compatibility_id == identity.state_format_compatibility_id
            && witness.sync_config_sha256 == identity.sync_config_sha256
            && TasDigest::from_bytes(witness.current_state_bytes) == witness.current_state_sha256,
        "worker PC Engine CD identity does not match the TAS project"
    );
    let state = crate::emu_backend::pce::inspect_pce_cd_tas_state_identity_for_arcade_card(
        witness.current_state_bytes,
        arcade_sync_config(identity.sync_config_sha256),
    )?;
    ensure!(
        TasDigest(state.disc_sha256) == identity.effective_media_sha256,
        "worker PC Engine CD state identity does not match the TAS project"
    );
    Ok(())
}

pub(crate) fn validate_direct_pce_cd_tas_execution_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateInspection> {
    ensure!(
        backend.system() == ActiveSystem::Pce,
        "TAS execution profile requires a PC Engine backend"
    );
    let metadata = backend.replay_metadata();
    let pce = backend
        .pce()
        .context("PC Engine backend became unavailable")?;
    let provenance = pce
        .tas_load_provenance()
        .context("PC Engine backend omitted CD load provenance")?;
    let arcade_card = provenance.load.selected_arcade_card_mode == PceArcadeCardMode::Enabled;
    let memory_base = provenance.load.selected_memory_base_mode == PceMemoryBaseMode::Enabled;
    ensure!(
        provenance.load.direct_pce_cd && provenance.load.raw_source_media_len != 0,
        "PC Engine CD TAS requires bounded direct media"
    );
    ensure!(
        [
            provenance.load.direct_pce_cd_chd,
            provenance.load.direct_pce_cd_iso,
            provenance.load.direct_pce_cd_ppf,
        ]
        .into_iter()
        .filter(|kind| *kind)
        .count()
            <= 1,
        "PC Engine CD TAS requires one direct media source type"
    );
    ensure!(
        (provenance.load.direct_pce_cd_ppf
            || provenance.load.source_disc_sha256 == provenance.load.effective_disc_sha256)
            && provenance.load.effective_disc_sha256 == pce.normalized_disc_hash(),
        "PC Engine CD TAS normalized disc identity is incompatible"
    );
    ensure!(
        provenance.load.tas_source_media_sha256
            == if provenance.load.direct_pce_cd_chd {
                direct_pce_cd_chd_source_identity(
                    provenance.load.raw_source_media_sha256,
                    provenance.load.raw_source_media_len,
                )
                .0
            } else if provenance.load.direct_pce_cd_iso {
                direct_pce_cd_iso_source_identity(
                    provenance.load.raw_source_media_sha256,
                    provenance.load.raw_source_media_len,
                )
                .0
            } else {
                provenance.load.raw_source_media_sha256
            }
            && provenance.load.tas_source_media_len == provenance.load.raw_source_media_len
            && provenance.load.tas_sync_config_sha256
                == sync_config_for_runtime(
                    provenance.load.direct_pce_cd_chd,
                    provenance.load.direct_pce_cd_iso,
                    provenance.load.direct_pce_cd_ppf,
                    arcade_card,
                    memory_base,
                )
                .0,
        "PC Engine CD TAS source identity is incompatible"
    );
    ensure!(
        ((!provenance.load.direct_pce_cd_ppf
            && !provenance.load.any_mod_enabled
            && !provenance.load.any_mod_applied)
            || (provenance.load.direct_pce_cd_ppf && provenance.load.any_mod_enabled))
            && provenance.load.persistent_load
                == crate::emu_backend::pce::PceTasPersistentLoadOutcome::Skipped
            && provenance.load.initial_input.is_none(),
        "PC Engine CD TAS requires unmodified media with host persistence disabled"
    );
    ensure!(
        provenance.load.direct_pce_cd_chd
            || provenance.load.direct_pce_cd_iso
            || provenance.load.direct_pce_cd_ppf
            || provenance.load.raw_source_media_sha256
                == provenance
                    .load
                    .source_disc_sha256
                    .context("PC Engine backend omitted source disc identity")?,
        "PC Engine CD TAS direct CUE identity is incompatible"
    );
    ensure!(
        !arcade_card
            || provenance.load.effective_disc_sha256.is_some_and(|hash| {
                direct_pce_cd_arcade_eligible(provenance.load.direct_pce_cd_ppf, hash)
            }),
        "PC Engine CD TAS Arcade Card requires an exact direct CUE, CHD, or ISO profile"
    );
    ensure!(
        !memory_base
            || provenance.load.source_disc_sha256.is_some_and(|hash| {
                direct_pce_cd_memory_base_eligible(
                    provenance.load.direct_pce_cd_chd,
                    provenance.load.direct_pce_cd_iso,
                    provenance.load.direct_pce_cd_ppf,
                    hash,
                )
            }),
        "PC Engine CD TAS Memory Base 128 requires an exact direct CUE, CHD, ISO, or ordered PPF profile"
    );
    ensure!(
        !(arcade_card && memory_base),
        "PC Engine CD TAS does not support combined Arcade Card and Memory Base 128 profiles"
    );
    ensure!(
        provenance.load.configured_sample_rate == Some(48_000)
            && provenance.load.initial_sample_rate == 48_000
            && provenance.current_sample_rate == 48_000,
        "PC Engine CD TAS requires an exact 48000 Hz sample rate"
    );
    ensure!(
        provenance.load.selected_wiring == Some(PceConsoleWiring::PcEngine)
            && provenance.load.effective_wiring == PceConsoleWiring::PcEngine
            && provenance.load.selected_board == Some(PceHuCardBoard::SystemCardV3)
            && provenance.load.effective_board == PceHuCardBoard::SystemCardV3
            && provenance.load.selected_hardware == Some(PceCartridgeHardware::Base)
            && provenance.load.effective_topology == PceHardwareTopology::Base
            && provenance.load.selected_controller_mode == PceControllerMode::TwoButton
            && provenance.load.effective_controller_mode == PceControllerMode::TwoButton
            && provenance.load.selected_memory_base_mode
                == provenance.load.effective_memory_base_mode
            && matches!(
                provenance.load.selected_memory_base_mode,
                PceMemoryBaseMode::Disabled | PceMemoryBaseMode::Enabled
            )
            && (provenance.load.selected_arcade_card_mode
                == provenance.load.effective_arcade_card_mode)
            && matches!(
                provenance.load.selected_arcade_card_mode,
                PceArcadeCardMode::Disabled | PceArcadeCardMode::Enabled
            ),
        "PC Engine CD TAS requires Base CD hardware and one two-button pad"
    );
    ensure!(
        !cheats_present && metadata.cheat_sha256.is_none(),
        "PC Engine CD TAS execution enabled cheats"
    );
    ensure!(
        backend.save_ram_kind() == SaveRamKind::known_battery_backed(CDROM2_BRAM_LEN)
            && pce.tas_frame_counters_match()
            && pce.tas_output_policy_is_exact()
            && pce.tas_presented_frame_is_current(),
        "PC Engine CD TAS runtime state is incompatible"
    );
    let state = backend.encode_state_bytes()?;
    let inspection =
        pce.inspect_current_native_cd_tas_state_for_profile(&state, arcade_card, memory_base)?;
    ensure!(
        inspection.board == PceHuCardBoard::SystemCardV3
            && inspection.wiring == PceConsoleWiring::PcEngine
            && inspection.psg_revision == PsgRevision::HuC6280
            && inspection.arcade_card_enabled == arcade_card
            && inspection.memory_base_enabled == memory_base
            && Some(inspection.disc_sha256) == pce.normalized_disc_hash()
            && firmware(backend, inspection.system_card_sha256)?.len() == 1,
        "PC Engine CD core media, firmware, or hardware identity is incompatible"
    );
    Ok(inspection)
}

pub(crate) fn validate_direct_pce_cd_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateInspection> {
    let inspection = validate_direct_pce_cd_tas_execution_runtime(backend, cheats_present)?;
    ensure!(
        inspection.controller_buttons == PadButtons::empty(),
        "PC Engine CD TAS acquisition requires neutral controller input"
    );
    Ok(inspection)
}

pub(crate) fn validate_direct_pce_cd_tas_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<crate::emu_backend::pce::PceTasStateProjection> {
    validate_direct_pce_cd_tas_execution_runtime(backend, false)?;
    let pce = match backend {
        EmuBackend::Pce(pce) => pce,
        _ => anyhow::bail!("TAS state requires a PC Engine backend"),
    };
    let arcade_card = pce.arcade_card_mode() == PceArcadeCardMode::Enabled;
    let memory_base = pce.memory_base_mode() == PceMemoryBaseMode::Enabled;
    let projection = pce.validate_and_load_current_native_cd_tas_state_for_profile(
        state,
        arcade_card,
        memory_base,
    )?;
    ensure!(
        projection.framebuffer.as_ref() == pce.tas_core_framebuffer()
            && pce.tas_presented_frame_is_current()
            && projection.frame_count == pce.frame_count()
            && pce.tas_frame_counters_match(),
        "PC Engine CD TAS state did not restore exact frame and output state"
    );
    validate_direct_pce_cd_tas_execution_runtime(backend, false)?;
    Ok(projection)
}

#[cfg(test)]
mod tests {
    use super::{
        direct_pce_cd_arcade_eligible, direct_pce_cd_chd_arcade_tas_sync_config_sha256,
        direct_pce_cd_chd_memory_base_tas_sync_config_sha256,
        direct_pce_cd_iso_arcade_tas_sync_config_sha256,
        direct_pce_cd_iso_memory_base_tas_sync_config_sha256, direct_pce_cd_memory_base_eligible,
        direct_pce_cd_memory_base_tas_sync_config_sha256,
        direct_pce_cd_ppf_memory_base_tas_sync_config_sha256, firmware_profile_is_supported,
        sync_config_for_runtime,
    };

    #[test]
    fn firmware_profile_rejects_wrong_region_tier_and_unknown_hash() {
        assert!(firmware_profile_is_supported(
            zeff_firmware::PCE_SYSTEM_CARD_V3_JAPAN_SHA256
        ));
        assert!(!firmware_profile_is_supported(
            zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256
        ));
        assert!(!firmware_profile_is_supported(
            zeff_firmware::PCE_SYSTEM_CARD_V2_JAPAN_SHA256
        ));
        assert!(!firmware_profile_is_supported([0; 32]));
    }

    #[test]
    fn arcade_catalog_eligibility_excludes_ppf_and_selects_media_specific_identity() {
        let arcade_catalog_disc = [
            0xa3, 0x88, 0x7d, 0xa6, 0x25, 0xbb, 0x8d, 0xee, 0x4f, 0xe3, 0x44, 0x76, 0x51, 0x52,
            0xab, 0x43, 0x73, 0xe8, 0xc5, 0x3d, 0x80, 0xda, 0x78, 0x1b, 0x1a, 0xc9, 0x3e, 0x7d,
            0x0e, 0x6d, 0xb8, 0xb2,
        ];
        assert!(direct_pce_cd_arcade_eligible(false, arcade_catalog_disc));
        assert!(!direct_pce_cd_arcade_eligible(true, arcade_catalog_disc));
        assert!(!direct_pce_cd_arcade_eligible(false, [0; 32]));
        assert_eq!(
            sync_config_for_runtime(true, false, false, true, false),
            direct_pce_cd_chd_arcade_tas_sync_config_sha256()
        );
        assert_eq!(
            sync_config_for_runtime(false, true, false, true, false),
            direct_pce_cd_iso_arcade_tas_sync_config_sha256()
        );
    }

    #[test]
    fn memory_base_catalog_eligibility_selects_each_exact_direct_source_route() {
        let memory_base_catalog_disc = [
            0x6d, 0x9c, 0x62, 0x34, 0x57, 0x8f, 0x65, 0x3d, 0x4c, 0x81, 0x37, 0x9e, 0x0b, 0xef,
            0xfb, 0x4b, 0x80, 0xbe, 0x18, 0x16, 0xf6, 0x61, 0x42, 0xfd, 0x08, 0x63, 0xa7, 0x79,
            0xe6, 0x8f, 0xab, 0x8f,
        ];
        assert!(direct_pce_cd_memory_base_eligible(
            false,
            false,
            false,
            memory_base_catalog_disc,
        ));
        assert!(direct_pce_cd_memory_base_eligible(
            true,
            false,
            false,
            memory_base_catalog_disc,
        ));
        assert!(direct_pce_cd_memory_base_eligible(
            false,
            true,
            false,
            memory_base_catalog_disc,
        ));
        assert!(direct_pce_cd_memory_base_eligible(
            false,
            false,
            true,
            memory_base_catalog_disc,
        ));
        assert!(!direct_pce_cd_memory_base_eligible(
            true,
            false,
            true,
            memory_base_catalog_disc,
        ));
        assert!(!direct_pce_cd_memory_base_eligible(
            false,
            true,
            true,
            memory_base_catalog_disc,
        ));
        assert!(!direct_pce_cd_memory_base_eligible(
            false,
            false,
            false,
            [
                0x65, 0xca, 0x62, 0xef, 0x00, 0xa6, 0x46, 0xc1, 0x15, 0x35, 0x4b, 0x7e, 0x96, 0xec,
                0xb9, 0xbc, 0x51, 0xca, 0x13, 0x88, 0x0a, 0x94, 0x07, 0x95, 0x81, 0x78, 0x47, 0x6b,
                0x78, 0xc0, 0x84, 0x26,
            ],
        ));
        assert_eq!(
            sync_config_for_runtime(false, false, false, false, true),
            direct_pce_cd_memory_base_tas_sync_config_sha256()
        );
        assert_eq!(
            sync_config_for_runtime(true, false, false, false, true),
            direct_pce_cd_chd_memory_base_tas_sync_config_sha256()
        );
        assert_eq!(
            sync_config_for_runtime(false, true, false, false, true),
            direct_pce_cd_iso_memory_base_tas_sync_config_sha256()
        );
        assert_eq!(
            sync_config_for_runtime(false, false, true, false, true),
            direct_pce_cd_ppf_memory_base_tas_sync_config_sha256()
        );
    }
}
