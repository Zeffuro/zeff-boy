use crate::tas_project::TasDigest;

const CHD_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-chd-arcade-card-multitap\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0catalog-witnesses=arcade-card-and-multitap-independent\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const CHD_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-chd-memory-base-128-multitap\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=enabled-exact-catalog\0arcade-card=disabled\0catalog-witnesses=memory-base-and-multitap-independent\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0native-memory-base=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const ISO_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-iso-arcade-card-multitap\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0catalog-witnesses=arcade-card-and-multitap-independent\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const ISO_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-iso-memory-base-128-multitap\0raw-source=sha256-length\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=enabled-exact-catalog\0arcade-card=disabled\0catalog-witnesses=memory-base-and-multitap-independent\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0native-memory-base=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PPF_ARCADE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-ppf-arcade-card-multitap\0raw-source=base-disc-plus-ordered-ppf-sha256-length\0source-disc-identity=pce-core-cd-disc-v1\0effective-disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=disconnected\0arcade-card=enabled-exact-source-catalog\0catalog-witnesses=arcade-card-and-multitap-independent\0mods=ordered-ppf-exact\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";
const PPF_MEMORY_BASE_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-ppf-memory-base-128-multitap\0raw-source=base-disc-plus-ordered-ppf-sha256-length\0source-disc-identity=pce-core-cd-disc-v1\0effective-disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=enabled-exact-source-catalog\0arcade-card=disabled\0catalog-witnesses=memory-base-and-multitap-independent\0mods=ordered-ppf-exact\0host-persistence=disabled\0native-bram=state-owned\0native-memory-base=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";

pub(crate) fn chd_arcade_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(CHD_ARCADE_SYNC_CONFIGURATION)
}

pub(crate) fn chd_memory_base_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(CHD_MEMORY_BASE_SYNC_CONFIGURATION)
}

pub(crate) fn iso_arcade_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(ISO_ARCADE_SYNC_CONFIGURATION)
}

pub(crate) fn iso_memory_base_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(ISO_MEMORY_BASE_SYNC_CONFIGURATION)
}

pub(crate) fn ppf_arcade_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PPF_ARCADE_SYNC_CONFIGURATION)
}

pub(crate) fn ppf_memory_base_sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(PPF_MEMORY_BASE_SYNC_CONFIGURATION)
}
