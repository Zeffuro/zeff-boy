use zeff_gba_core::hardware::cartridge::TiltState;

use crate::tas_project::{TasDeviceIdentity, TasDigest, TasExternalIdentity};

const DEVICE_CONFIGURATION: &[u8] =
    b"zeff-tas-device-config-v1\0gba-cartridge-tilt\0two-axis-ieee754-bits\0";
const DIRECT_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gba-direct-cartridge-tilt\0startup=internal-post-boot\0persistence=project-owned-eeprom\0rtc=absent\0link=absent\0sensor=gba-tilt\0sensor-input=recorded-ieee754-bits\0host-sampling=disabled\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=hle:nintendo.gba.bios:zeff-gba-hle:1\0";
const ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gba-zip-member-tilt\0startup=internal-post-boot\0persistence=project-owned-eeprom\0rtc=absent\0link=absent\0sensor=gba-tilt\0sensor-input=recorded-ieee754-bits\0host-sampling=disabled\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=hle:nintendo.gba.bios:zeff-gba-hle:1\0member=";

pub(super) fn direct_sync_config() -> TasDigest {
    TasDigest::from_bytes(DIRECT_SYNC_CONFIGURATION)
}

pub(super) fn zip_sync_config(member_name: &str) -> TasDigest {
    let mut bytes = Vec::with_capacity(ZIP_SYNC_CONFIGURATION.len() + member_name.len());
    bytes.extend_from_slice(ZIP_SYNC_CONFIGURATION);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

pub(super) fn device() -> TasDeviceIdentity {
    TasDeviceIdentity {
        port: "cartridge.sensor".to_owned(),
        device: "gba-tilt-sensor".to_owned(),
        configuration_sha256: TasDigest::from_bytes(DEVICE_CONFIGURATION),
    }
}

pub(super) fn identity(state: Option<TiltState>) -> TasExternalIdentity {
    state.map_or(TasExternalIdentity::Absent, |state| {
        TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&state_bytes(state)))
    })
}

pub(super) fn is_initial(state: TiltState) -> bool {
    state
        == TiltState {
            host_x_bits: 0,
            host_y_bits: 0,
            x_latch: 0x0FFF,
            y_latch: 0x0FFF,
            latch_ready: false,
        }
}

fn state_bytes(state: TiltState) -> [u8; 13] {
    let mut bytes = [0; 13];
    bytes[..4].copy_from_slice(&state.host_x_bits.to_le_bytes());
    bytes[4..8].copy_from_slice(&state.host_y_bits.to_le_bytes());
    bytes[8..10].copy_from_slice(&state.x_latch.to_le_bytes());
    bytes[10..12].copy_from_slice(&state.y_latch.to_le_bytes());
    bytes[12] = u8::from(state.latch_ready);
    bytes
}
