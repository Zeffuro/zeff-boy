use crate::tas_project::TasDigest;

const SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0pce-direct-cue-arcade-card-multitap\0disc-identity=pce-core-cd-disc-v1\0wiring=pc-engine\0topology=base\0system-card=v3-japan-exact-external\0controller=five-port-multitap\0ports=p1-p5-two-button\0memory-base=disconnected\0arcade-card=enabled-exact-catalog\0catalog-witnesses=arcade-card-and-multitap-independent\0mods=disabled\0host-persistence=disabled\0native-bram=state-owned\0initial-input=neutral\0multitap-active-port=none\0select=high\0clear=high\0sample-rate=48000\0overscan=full\0palette=raw-rgb\0";

pub(crate) fn sync_config_sha256() -> TasDigest {
    TasDigest::from_bytes(SYNC_CONFIGURATION)
}
