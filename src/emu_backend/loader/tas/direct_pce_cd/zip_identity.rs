use sha2::{Digest, Sha256};

use crate::tas_project::TasDigest;

const UNIQUE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-unique-cue\0raw-source=sha256-length-plus-cue-member\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const UNIQUE_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-unique-cue-arcade-card\0raw-source=sha256-length-plus-cue-member\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const UNIQUE_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-unique-cue-memory-base-128\0raw-source=sha256-length-plus-cue-member\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=enabled-exact-catalog\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const SELECTED_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-selected-cue\0raw-source=sha256-length-plus-cue-member\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const SELECTED_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-selected-cue-arcade-card\0raw-source=sha256-length-plus-cue-member\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const SELECTED_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-selected-cue-memory-base-128\0raw-source=sha256-length-plus-cue-member\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=enabled-exact-catalog\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const UNIQUE_MULTITAP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-unique-cue\0raw-source=sha256-length-plus-cue-member\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const SELECTED_MULTITAP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-selected-cue\0raw-source=sha256-length-plus-cue-member\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const UNIQUE_PPF_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-unique-cue-ppf\0raw-source=outer-sha256-length-plus-cue-member-plus-ordered-ppf-path-sha256-length\0source-disc-identity=pce-core-cd-disc-v1\0effective-disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=archive-contained-ordered-ppf-exact\0external-mods=ignored\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const SELECTED_PPF_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-zip-selected-cue-ppf\0raw-source=outer-sha256-length-plus-cue-member-plus-ordered-ppf-path-sha256-length\0source-disc-identity=pce-core-cd-disc-v1\0effective-disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=two-button\0memory-base=disconnected\0arcade-card=disabled\0mods=archive-contained-ordered-ppf-exact\0external-mods=ignored\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";

pub(crate) fn sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(UNIQUE_SYNC_CONFIGURATION)
}
pub(crate) fn arcade_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(UNIQUE_ARCADE_SYNC_CONFIGURATION)
}
pub(crate) fn memory_base_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(UNIQUE_MEMORY_BASE_SYNC_CONFIGURATION)
}
pub(crate) fn selected_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(SELECTED_SYNC_CONFIGURATION)
}
pub(crate) fn selected_arcade_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(SELECTED_ARCADE_SYNC_CONFIGURATION)
}
pub(crate) fn selected_memory_base_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(SELECTED_MEMORY_BASE_SYNC_CONFIGURATION)
}

pub(crate) fn multitap_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(UNIQUE_MULTITAP_SYNC_CONFIGURATION)
}

pub(crate) fn selected_multitap_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(SELECTED_MULTITAP_SYNC_CONFIGURATION)
}

pub(crate) fn ppf_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(UNIQUE_PPF_SYNC_CONFIGURATION)
}

pub(crate) fn selected_ppf_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(SELECTED_PPF_SYNC_CONFIGURATION)
}

pub(crate) fn source_identity(
    raw_source_sha256: [u8; 32],
    raw_source_len: usize,
    cue_member_path_sha256: [u8; 32],
) -> TasDigest {
    let mut hasher = Sha256::new();
    hasher.update(b"zeff-tas-pce-cd-zip-source:v1\0");
    hasher.update(raw_source_sha256);
    hasher.update((raw_source_len as u64).to_le_bytes());
    hasher.update(cue_member_path_sha256);
    TasDigest(hasher.finalize().into())
}

pub(crate) fn ppf_source_identity(
    raw_source_sha256: [u8; 32],
    raw_source_len: usize,
    cue_member_path_sha256: [u8; 32],
    patches: &[(&str, usize, [u8; 32])],
) -> TasDigest {
    let mut hasher = Sha256::new();
    hasher.update(b"zeff-tas-pce-cd-zip-ppf-source:v1\0");
    super::seven_zip_identity::hash_archive_ppf_source(
        &mut hasher,
        raw_source_sha256,
        raw_source_len,
        cue_member_path_sha256,
        patches,
    );
    TasDigest(hasher.finalize().into())
}
