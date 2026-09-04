use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_ram::SaveRamKind;
use zeff_emu_common::time::FrameLifecycle;
use zeff_pce_core::hardware::{
    CDROM2_BRAM_LEN, PadButtons, PceArcadeCardMode, PceCartridgeHardware, PceConsoleWiring,
    PceControllerMode, PceHardwareTopology, PceHuCardBoard, PceMemoryBaseMode, PsgRevision,
};

use super::{
    ActiveSystem, EmuBackend, TasProjectRuntimeWitness,
    direct_pce::{PceTasHardwareProfile, direct_pce_tas_devices},
    tas_firmware_identity,
};
use crate::tas_project::{
    TasCameraInput, TasDeviceIdentity, TasDigest, TasExternalIdentity, TasFirmwareIdentity,
    TasPatchIdentity, TasProject, TasProjectIdentity,
};

#[path = "direct_pce_cd/rar_identity.rs"]
mod rar_identity;
pub(crate) use rar_identity::{
    arcade_sync_config_sha256 as direct_pce_cd_rar_arcade_tas_sync_config_sha256,
    memory_base_sync_config_sha256 as direct_pce_cd_rar_memory_base_tas_sync_config_sha256,
    multitap_sync_config_sha256 as direct_pce_multitap_cd_rar_tas_sync_config_sha256,
    ppf_source_identity as direct_pce_cd_rar_ppf_source_identity,
    ppf_sync_config_sha256 as direct_pce_cd_rar_ppf_tas_sync_config_sha256,
    selected_arcade_sync_config_sha256 as direct_pce_cd_selected_rar_arcade_tas_sync_config_sha256,
    selected_memory_base_sync_config_sha256 as direct_pce_cd_selected_rar_memory_base_tas_sync_config_sha256,
    selected_multitap_sync_config_sha256 as direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256,
    selected_ppf_sync_config_sha256 as direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256,
    selected_sync_config_sha256 as direct_pce_cd_selected_rar_tas_sync_config_sha256,
    source_identity as direct_pce_cd_rar_source_identity,
    sync_config_sha256 as direct_pce_cd_rar_tas_sync_config_sha256,
};
#[path = "direct_pce_cd/seven_zip_identity.rs"]
mod seven_zip_identity;
pub(crate) use seven_zip_identity::{
    arcade_sync_config_sha256 as direct_pce_cd_archive_arcade_tas_sync_config_sha256,
    memory_base_sync_config_sha256 as direct_pce_cd_archive_memory_base_tas_sync_config_sha256,
    multitap_sync_config_sha256 as direct_pce_multitap_cd_archive_tas_sync_config_sha256,
    ppf_source_identity as direct_pce_cd_archive_ppf_source_identity,
    ppf_sync_config_sha256 as direct_pce_cd_archive_ppf_tas_sync_config_sha256,
    selected_arcade_sync_config_sha256 as direct_pce_cd_selected_archive_arcade_tas_sync_config_sha256,
    selected_memory_base_sync_config_sha256 as direct_pce_cd_selected_archive_memory_base_tas_sync_config_sha256,
    selected_multitap_sync_config_sha256 as direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256,
    selected_ppf_sync_config_sha256 as direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256,
    selected_sync_config_sha256 as direct_pce_cd_selected_archive_tas_sync_config_sha256,
    source_identity as direct_pce_cd_archive_source_identity,
    sync_config_sha256 as direct_pce_cd_archive_tas_sync_config_sha256,
};
#[path = "direct_pce_cd/zip_identity.rs"]
mod zip_identity;
pub(crate) use zip_identity::{
    arcade_sync_config_sha256 as direct_pce_cd_zip_arcade_tas_sync_config_sha256,
    memory_base_sync_config_sha256 as direct_pce_cd_zip_memory_base_tas_sync_config_sha256,
    multitap_sync_config_sha256 as direct_pce_multitap_cd_zip_tas_sync_config_sha256,
    ppf_source_identity as direct_pce_cd_zip_ppf_source_identity,
    ppf_sync_config_sha256 as direct_pce_cd_zip_ppf_tas_sync_config_sha256,
    selected_arcade_sync_config_sha256 as direct_pce_cd_selected_zip_arcade_tas_sync_config_sha256,
    selected_memory_base_sync_config_sha256 as direct_pce_cd_selected_zip_memory_base_tas_sync_config_sha256,
    selected_multitap_sync_config_sha256 as direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256,
    selected_ppf_sync_config_sha256 as direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256,
    selected_sync_config_sha256 as direct_pce_cd_selected_zip_tas_sync_config_sha256,
    source_identity as direct_pce_cd_zip_source_identity,
    sync_config_sha256 as direct_pce_cd_zip_tas_sync_config_sha256,
};
#[path = "direct_pce_cd/execution_runtime.rs"]
mod execution_runtime;
use execution_runtime::validate_direct_pce_cd_tas_execution_runtime_for_controller;
pub(crate) use execution_runtime::{
    validate_direct_pce_cd_tas_execution_runtime,
    validate_direct_pce_multitap_cd_tas_execution_runtime,
};
#[path = "direct_pce_cd/arcade_multitap_identity.rs"]
mod arcade_multitap_identity;
pub(crate) use arcade_multitap_identity::sync_config_sha256 as direct_pce_multitap_cd_arcade_tas_sync_config_sha256;
#[path = "direct_pce_cd/memory_base_multitap_identity.rs"]
mod memory_base_multitap_identity;
pub(crate) use memory_base_multitap_identity::sync_config_sha256 as direct_pce_multitap_cd_memory_base_tas_sync_config_sha256;
#[path = "direct_pce_cd/media_profile.rs"]
mod media_profile;
use media_profile::arcade_sync_config;
#[cfg(test)]
use media_profile::sync_config_for_runtime;
pub(crate) use media_profile::{
    PceCdArchiveFormat, PceCdArchiveSelection, PceCdExpansion, PceCdTasMediaRoute, PceCdTasProfile,
};

const PCE_CD_TWO_BUTTON_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0pce-two-button-controller\0";
const PCE_CD_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_MULTITAP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_CHD_MULTITAP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-chd\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_ISO_MULTITAP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-iso\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_PPF_MULTITAP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-ppf\0raw-source=base-disc-plus-ordered-ppf-sha256-length\0source-disc-identity=pce-core-cd-disc-v1\0effective-disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=ordered-ppf-exact\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
pub(crate) const PCE_CD_UNPATCHED_DISC_PATCH_FORMAT: &str = "pce-cd-unpatched-disc-v1";
const PCE_CD_CHD_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-chd\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_ISO_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-iso\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_PPF_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-ppf\0raw-source=base-disc-plus-ordered-ppf-sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=ordered-ppf-exact\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-arcade-card\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_CHD_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-chd-arcade-card\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_ISO_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-iso-arcade-card\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_PPF_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-ppf-arcade-card\0raw-source=base-disc-plus-ordered-ppf-sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=enabled-exact-source-catalog\0mods=ordered-ppf-exact\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-memory-base-128\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=enabled-exact-catalog\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_CHD_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-chd-memory-base-128\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=enabled-exact-catalog\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_ISO_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-iso-memory-base-128\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=enabled-exact-catalog\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PCE_CD_PPF_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-ppf-memory-base-128\0raw-source=base-disc-plus-ordered-ppf-sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=enabled-exact-source-catalog\0arcade-card=disabled\0mods=ordered-ppf-exact\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";

pub(crate) fn direct_pce_cd_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_multitap_cd_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_MULTITAP_SYNC_CONFIGURATION)
}

pub(crate) fn is_direct_pce_cd_archive_ppf_tas_sync_config_sha256(
    sync_config_sha256: TasDigest,
) -> bool {
    direct_pce_cd_archive_ppf_tas_sync_configs().contains(&sync_config_sha256)
}

fn direct_pce_cd_archive_ppf_tas_sync_configs() -> [TasDigest; 6] {
    [
        direct_pce_cd_archive_ppf_tas_sync_config_sha256(),
        direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256(),
        direct_pce_cd_rar_ppf_tas_sync_config_sha256(),
        direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256(),
        direct_pce_cd_zip_ppf_tas_sync_config_sha256(),
        direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256(),
    ]
}

#[cfg(test)]
pub(crate) fn direct_pce_cd_archive_ppf_tas_sync_configs_for_test() -> [TasDigest; 6] {
    direct_pce_cd_archive_ppf_tas_sync_configs()
}

pub(crate) fn direct_pce_multitap_cd_chd_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_CHD_MULTITAP_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_multitap_cd_iso_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_ISO_MULTITAP_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_multitap_cd_ppf_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_PPF_MULTITAP_SYNC_CONFIGURATION)
}

pub(crate) fn direct_pce_multitap_cd_sync_config(sync: TasDigest) -> bool {
    PceCdTasProfile::from_sync(sync)
        .is_some_and(|profile| profile.controller() == PceControllerMode::Multitap)
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

pub(crate) fn direct_pce_cd_ppf_arcade_tas_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PCE_CD_PPF_ARCADE_SYNC_CONFIGURATION)
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

fn memory_base_chd_sync_config(sync: TasDigest) -> bool {
    PceCdTasProfile::from_sync(sync).is_some_and(|profile| {
        profile.media() == PceCdTasMediaRoute::Chd
            && profile.expansion() == PceCdExpansion::MemoryBase128
    })
}

fn memory_base_sync_config(sync: TasDigest) -> bool {
    PceCdTasProfile::from_sync(sync)
        .is_some_and(|profile| profile.expansion() == PceCdExpansion::MemoryBase128)
}

fn memory_base_iso_sync_config(sync: TasDigest) -> bool {
    PceCdTasProfile::from_sync(sync).is_some_and(|profile| {
        profile.media() == PceCdTasMediaRoute::Iso
            && profile.expansion() == PceCdExpansion::MemoryBase128
    })
}

fn memory_base_cue_sync_config(sync: TasDigest) -> bool {
    PceCdTasProfile::from_sync(sync).is_some_and(|profile| {
        profile.media() == PceCdTasMediaRoute::Cue
            && profile.expansion() == PceCdExpansion::MemoryBase128
    })
}

fn memory_base_ppf_sync_config(sync: TasDigest) -> bool {
    PceCdTasProfile::from_sync(sync).is_some_and(|profile| {
        profile.media() == PceCdTasMediaRoute::Ppf
            && profile.expansion() == PceCdExpansion::MemoryBase128
    })
}

fn archive_sync_config(sync: TasDigest) -> bool {
    PceCdTasProfile::from_sync(sync)
        .is_some_and(|profile| matches!(profile.archive(), Some((PceCdArchiveFormat::SevenZip, _))))
}

fn rar_sync_config(sync: TasDigest) -> bool {
    PceCdTasProfile::from_sync(sync)
        .is_some_and(|profile| matches!(profile.archive(), Some((PceCdArchiveFormat::Rar, _))))
}

fn zip_sync_config(sync: TasDigest) -> bool {
    PceCdTasProfile::from_sync(sync)
        .is_some_and(|profile| matches!(profile.archive(), Some((PceCdArchiveFormat::Zip, _))))
}

pub(crate) fn direct_pce_cd_arcade_eligible(ppf: bool, normalized_disc_sha256: [u8; 32]) -> bool {
    let _ = ppf;
    crate::emu_backend::pce_profiles::automatic_arcade_card_enabled(Some(normalized_disc_sha256))
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

pub(crate) fn direct_pce_cd_memory_base_multitap_eligible(
    normalized_disc_sha256: [u8; 32],
) -> bool {
    crate::emu_backend::pce_profiles::automatic_memory_base_enabled(Some(normalized_disc_sha256))
        && crate::emu_backend::pce_profiles::automatic_controller_mode(normalized_disc_sha256)
            == PceControllerMode::Multitap
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

fn devices_for_controller(controller_mode: PceControllerMode) -> Vec<TasDeviceIdentity> {
    if controller_mode == PceControllerMode::Multitap {
        return direct_pce_tas_devices(PceTasHardwareProfile {
            board: PceHuCardBoard::Plain,
            topology: PceHardwareTopology::Base,
            controller_mode,
        });
    }
    devices()
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
    sha256 == zeff_firmware::PCE_SYSTEM_CARD_V3_JAPAN_SHA256
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
    direct_pce_cd_tas_identity_for_controller(backend, start_state, PceControllerMode::TwoButton)
}

pub(crate) fn direct_pce_multitap_cd_tas_identity(
    backend: &EmuBackend,
    start_state: &[u8],
) -> Result<TasProjectIdentity> {
    direct_pce_cd_tas_identity_for_controller(backend, start_state, PceControllerMode::Multitap)
}

fn direct_pce_cd_tas_identity_for_controller(
    backend: &EmuBackend,
    start_state: &[u8],
    controller_mode: PceControllerMode,
) -> Result<TasProjectIdentity> {
    let inspection =
        validate_direct_pce_cd_tas_runtime_for_controller(backend, false, controller_mode)?;
    let metadata = backend.replay_metadata();
    ensure!(
        backend.encode_state_bytes()?.as_slice() == start_state,
        "PC Engine CD TAS start state differs from the loaded baseline"
    );
    let pce = backend
        .pce()
        .context("PC Engine backend became unavailable")?;
    let state = if controller_mode == PceControllerMode::TwoButton {
        pce.inspect_current_native_cd_tas_state_for_profile(
            start_state,
            inspection.arcade_card_enabled,
            inspection.memory_base_enabled,
        )?
    } else {
        pce.inspect_current_native_cd_tas_state_for_profile_and_controller(
            start_state,
            inspection.arcade_card_enabled,
            inspection.memory_base_enabled,
            controller_mode,
        )?
    };
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
        patches: if provenance.load.direct_pce_cd_archive_ppf
            || (controller_mode == PceControllerMode::Multitap && provenance.load.direct_pce_cd_ppf)
        {
            vec![TasPatchIdentity {
                format: PCE_CD_UNPATCHED_DISC_PATCH_FORMAT.to_owned(),
                sha256: TasDigest(
                    provenance.load.source_disc_sha256.context(
                        "PC Engine CD PPF provenance omitted its unpatched disc identity",
                    )?,
                ),
            }]
        } else {
            Vec::new()
        },
        firmware: firmware(backend, inspection.system_card_sha256)?,
        devices: devices_for_controller(controller_mode),
        sync_config_sha256: PceCdTasProfile::from_runtime_flags(
            (
                provenance.load.direct_pce_cd_chd,
                provenance.load.direct_pce_cd_iso,
                provenance.load.direct_pce_cd_ppf,
                provenance.load.direct_pce_cd_archive,
                provenance.load.direct_pce_cd_rar,
                provenance.load.direct_pce_cd_zip,
            ),
            provenance.load.direct_pce_cd_archive_ppf,
            (
                provenance.load.archive_cue_explicitly_selected,
                provenance.load.rar_cue_explicitly_selected,
                provenance.load.zip_cue_explicitly_selected,
            ),
            (
                inspection.arcade_card_enabled,
                inspection.memory_base_enabled,
            ),
            controller_mode,
        )
        .context("PC Engine CD TAS load provenance describes an invalid profile")?
        .sync_config(),
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
    validate_direct_pce_cd_tas_project_identity_for_controller(
        project,
        PceControllerMode::TwoButton,
    )
}

pub(crate) fn validate_direct_pce_multitap_cd_tas_project_identity(
    project: &TasProject,
) -> Result<()> {
    validate_direct_pce_cd_tas_project_identity_for_controller(project, PceControllerMode::Multitap)
}

fn validate_direct_pce_cd_tas_project_identity_for_controller(
    project: &TasProject,
    controller_mode: PceControllerMode,
) -> Result<()> {
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
    let profile = PceCdTasProfile::from_sync(identity.sync_config_sha256)
        .context("TAS project has an unknown PC Engine CD sync configuration")?;
    let arcade_card = profile.expansion() == PceCdExpansion::ArcadeCard;
    let memory_base = memory_base_sync_config(identity.sync_config_sha256);
    let memory_base_chd = memory_base_chd_sync_config(identity.sync_config_sha256);
    let memory_base_iso = memory_base_iso_sync_config(identity.sync_config_sha256);
    let memory_base_ppf = memory_base_ppf_sync_config(identity.sync_config_sha256);
    let memory_base_cue = memory_base_cue_sync_config(identity.sync_config_sha256);
    let archive = archive_sync_config(identity.sync_config_sha256);
    let rar = rar_sync_config(identity.sync_config_sha256);
    let zip = zip_sync_config(identity.sync_config_sha256);
    let arcade_ppf = arcade_card && profile.media() == PceCdTasMediaRoute::Ppf;
    let multitap = profile.controller() == PceControllerMode::Multitap;
    let ppf_multitap = multitap && profile.media() == PceCdTasMediaRoute::Ppf;
    let archive_ppf = profile.archive_ppf();
    let state = crate::emu_backend::pce::inspect_pce_cd_tas_state_identity_for_arcade_card(
        project.start_state(),
        arcade_card,
    )?;
    let multitap_catalog_sha256 = if ppf_multitap {
        match identity.patches.as_slice() {
            [patch] if patch.format == PCE_CD_UNPATCHED_DISC_PATCH_FORMAT => patch.sha256.0,
            _ => [0; 32],
        }
    } else {
        state.disc_sha256
    };
    let cue = profile.media() == PceCdTasMediaRoute::Cue || arcade_ppf || memory_base_cue;
    ensure!(
        state.board == PceHuCardBoard::SystemCardV3
            && TasDigest(state.disc_sha256) == identity.effective_media_sha256
            && (!arcade_card
                || arcade_ppf
                || direct_pce_cd_arcade_eligible(false, state.disc_sha256))
            && (!memory_base
                || (multitap
                    && profile.media() == PceCdTasMediaRoute::Cue
                    && direct_pce_cd_memory_base_multitap_eligible(state.disc_sha256))
                || (!multitap
                    && (memory_base_ppf
                        || direct_pce_cd_memory_base_eligible(
                            memory_base_chd,
                            memory_base_iso,
                            false,
                            state.disc_sha256,
                        ))))
            && !(arcade_card && memory_base)
            && profile.archive().is_some() == (archive || rar || zip)
            && (!cue
                || identity.source_media_sha256 == identity.effective_media_sha256
                || arcade_ppf)
            && (!(memory_base_ppf || arcade_ppf)
                || identity.source_media_sha256 != identity.effective_media_sha256)
            && ((!(ppf_multitap || archive_ppf) && identity.patches.is_empty())
                || ((ppf_multitap || archive_ppf)
                    && matches!(
                        identity.patches.as_slice(),
                        [patch] if patch.format == PCE_CD_UNPATCHED_DISC_PATCH_FORMAT
                    )
                    && (!ppf_multitap
                        || identity.source_media_sha256 != identity.effective_media_sha256)))
            && multitap == (controller_mode == PceControllerMode::Multitap)
            && (!multitap
                || ((!memory_base
                    || (profile.media() == PceCdTasMediaRoute::Cue
                        && direct_pce_cd_memory_base_multitap_eligible(state.disc_sha256)))
                    && (!arcade_card
                        || (profile.media() == PceCdTasMediaRoute::Cue
                            && direct_pce_cd_arcade_eligible(false, state.disc_sha256)))
                    && crate::emu_backend::pce_profiles::automatic_controller_mode(
                        multitap_catalog_sha256,
                    ) == PceControllerMode::Multitap))
            && identity.devices == devices_for_controller(controller_mode)
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
    validate_direct_pce_cd_tas_branch_scope_for_controller(
        project,
        branch_id,
        PceControllerMode::TwoButton,
    )
}

pub(crate) fn validate_direct_pce_multitap_cd_tas_branch_scope(
    project: &TasProject,
    branch_id: &str,
) -> Result<()> {
    validate_direct_pce_cd_tas_branch_scope_for_controller(
        project,
        branch_id,
        PceControllerMode::Multitap,
    )
}

fn validate_direct_pce_cd_tas_branch_scope_for_controller(
    project: &TasProject,
    branch_id: &str,
    controller_mode: PceControllerMode,
) -> Result<()> {
    validate_direct_pce_cd_tas_project_identity_for_controller(project, controller_mode)?;
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
            input.players[..if controller_mode == PceControllerMode::Multitap {
                5
            } else {
                1
            }]
                .iter()
                .all(|player| player.buttons & !0x0F == 0 && player.dpad & !0x0F == 0)
                && input.players[if controller_mode == PceControllerMode::Multitap {
                    5
                } else {
                    1
                }..]
                    .iter()
                    .all(|player| *player == Default::default())
                && input.coleco == [crate::tas_project::TasColecoControllerInput::default(); 2]
                && input.zapper == Default::default()
                && input.tilt_x_bits == 0
                && input.tilt_y_bits == 0
                && matches!(input.camera, TasCameraInput::None),
            "PC Engine CD TAS execution input exceeds its controller topology"
        );
    }
    Ok(())
}

pub(crate) fn validate_direct_pce_cd_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_direct_pce_cd_tas_project_witness_for_controller(
        project,
        branch_id,
        witness,
        PceControllerMode::TwoButton,
    )
}

pub(crate) fn validate_direct_pce_multitap_cd_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    validate_direct_pce_cd_tas_project_witness_for_controller(
        project,
        branch_id,
        witness,
        PceControllerMode::Multitap,
    )
}

fn validate_direct_pce_cd_tas_project_witness_for_controller(
    project: &TasProject,
    branch_id: &str,
    witness: TasProjectRuntimeWitness<'_>,
    controller_mode: PceControllerMode,
) -> Result<()> {
    validate_direct_pce_cd_tas_branch_scope_for_controller(project, branch_id, controller_mode)?;
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

pub(crate) fn validate_direct_pce_cd_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateInspection> {
    validate_direct_pce_cd_tas_runtime_for_controller(
        backend,
        cheats_present,
        PceControllerMode::TwoButton,
    )
}

pub(crate) fn validate_direct_pce_multitap_cd_tas_runtime(
    backend: &EmuBackend,
    cheats_present: bool,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateInspection> {
    validate_direct_pce_cd_tas_runtime_for_controller(
        backend,
        cheats_present,
        PceControllerMode::Multitap,
    )
}

fn validate_direct_pce_cd_tas_runtime_for_controller(
    backend: &EmuBackend,
    cheats_present: bool,
    controller_mode: PceControllerMode,
) -> Result<zeff_pce_core::hardware::save_state::tas::CurrentNativePceCdTasStateInspection> {
    let inspection = validate_direct_pce_cd_tas_execution_runtime_for_controller(
        backend,
        cheats_present,
        controller_mode,
    )?;
    let multitap_neutral = inspection.controller_multitap.is_some_and(|state| {
        state.buttons.iter().all(|buttons| buttons.is_empty())
            && state.active_port.is_none()
            && state.select_high
            && state.clear_high
    });
    ensure!(
        (controller_mode == PceControllerMode::TwoButton
            && inspection.controller_buttons == PadButtons::empty()
            && inspection.controller_multitap.is_none())
            || (controller_mode == PceControllerMode::Multitap && multitap_neutral),
        "PC Engine CD TAS acquisition requires neutral controller input"
    );
    Ok(inspection)
}

pub(crate) fn validate_direct_pce_cd_tas_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<crate::emu_backend::pce::PceTasStateProjection> {
    validate_direct_pce_cd_tas_state_for_controller(backend, state, PceControllerMode::TwoButton)
}

pub(crate) fn validate_direct_pce_multitap_cd_tas_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<crate::emu_backend::pce::PceTasStateProjection> {
    validate_direct_pce_cd_tas_state_for_controller(backend, state, PceControllerMode::Multitap)
}

fn validate_direct_pce_cd_tas_state_for_controller(
    backend: &mut EmuBackend,
    state: &[u8],
    controller_mode: PceControllerMode,
) -> Result<crate::emu_backend::pce::PceTasStateProjection> {
    validate_direct_pce_cd_tas_execution_runtime_for_controller(backend, false, controller_mode)?;
    let pce = match backend {
        EmuBackend::Pce(pce) => pce,
        _ => anyhow::bail!("TAS state requires a PC Engine backend"),
    };
    let arcade_card = pce.arcade_card_mode() == PceArcadeCardMode::Enabled;
    let memory_base = pce.memory_base_mode() == PceMemoryBaseMode::Enabled;
    let projection = if controller_mode == PceControllerMode::TwoButton {
        pce.validate_and_load_current_native_cd_tas_state_for_profile(
            state,
            arcade_card,
            memory_base,
        )?
    } else {
        pce.validate_and_load_current_native_cd_tas_state_for_profile_and_controller(
            state,
            arcade_card,
            memory_base,
            controller_mode,
        )?
    };
    ensure!(
        projection.framebuffer.as_ref() == pce.tas_core_framebuffer()
            && pce.tas_presented_frame_is_current()
            && projection.frame_count == pce.frame_count()
            && pce.tas_frame_counters_match(),
        "PC Engine CD TAS state did not restore exact frame and output state"
    );
    validate_direct_pce_cd_tas_execution_runtime_for_controller(backend, false, controller_mode)?;
    Ok(projection)
}

#[cfg(test)]
#[path = "direct_pce_cd/identity_tests.rs"]
mod identity_tests;
