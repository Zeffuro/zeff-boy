use zeff_gba_core::hardware::cartridge::{BackupKind, RtcState};

use crate::tas_project::{TasDigest, TasExternalIdentity};

const RTC_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gba-direct-cartridge\0startup=internal-post-boot\0rtc=deterministic-cycle-clock\0epoch=2000-01-01T00:00:00\0link=absent\0sensors=absent\0mods=disabled\0initial-input=neutral\0sample-rate=48000\0firmware=hle:nintendo.gba.bios:zeff-gba-hle:1\0persistence=";
const ZIP_SYNC_CONFIGURATION: &[u8] = b"zeff-tas-sync-config-v1\0gba-zip-member\0";

pub(super) fn direct_sync_config(backup_kind: BackupKind) -> TasDigest {
    let mut bytes = Vec::from(RTC_SYNC_CONFIGURATION);
    bytes.extend_from_slice(if backup_kind == BackupKind::None {
        b"absent\0".as_slice()
    } else {
        b"project-owned-cartridge-save\0".as_slice()
    });
    bytes.extend_from_slice(b"backup-kind=");
    bytes.push(backup_kind_byte(backup_kind));
    TasDigest::from_bytes(&bytes)
}

pub(super) fn zip_sync_config(member_name: &str, backup_kind: BackupKind) -> TasDigest {
    let base = direct_sync_config(backup_kind);
    let mut bytes = Vec::with_capacity(ZIP_SYNC_CONFIGURATION.len() + 32 + member_name.len());
    bytes.extend_from_slice(ZIP_SYNC_CONFIGURATION);
    bytes.extend_from_slice(&base.0);
    bytes.extend_from_slice(member_name.as_bytes());
    TasDigest::from_bytes(&bytes)
}

pub(super) fn identity(state: Option<&[u8]>) -> TasExternalIdentity {
    let Some(state) = state else {
        return TasExternalIdentity::Absent;
    };
    let mut bytes = Vec::from(
        b"zeff-gba-tas-rtc-state-v1\0policy=deterministic-cycle-clock\0epoch=2000-01-01T00:00:00\0"
            .as_slice(),
    );
    bytes.extend_from_slice(state);
    TasExternalIdentity::ExternalSha256(TasDigest::from_bytes(&bytes))
}

pub(super) fn is_initial_epoch(state: RtcState) -> bool {
    state.data_latch == 0
        && state.pin_state == 0
        && state.direction == 0
        && !state.read_write
        && state.transfer_step == 0
        && !state.last_sck
        && !state.read_bit_sampled
        && state.bits_read == 0
        && state.bits == 0
        && state.command.is_none()
        && !state.command_reading
        && state.bytes_remaining == 0
        && state.transfer_bytes == [0; 7]
        && state.transfer_index == 0
        && state.control == 0x40
        && state.date_time.year() == 2000
        && state.date_time.month() == 1
        && state.date_time.day() == 1
        && state.date_time.weekday() == 6
        && state.date_time.hour() == 0
        && state.date_time.minute() == 0
        && state.date_time.second() == 0
        && state.subsecond_cycles == 0
}

pub(super) const fn supported_backup_kinds() -> [BackupKind; 5] {
    [
        BackupKind::None,
        BackupKind::Sram,
        BackupKind::Flash512,
        BackupKind::Flash1M,
        BackupKind::Eeprom,
    ]
}

const fn backup_kind_byte(kind: BackupKind) -> u8 {
    match kind {
        BackupKind::None => 0,
        BackupKind::Sram => 1,
        BackupKind::Flash512 => 2,
        BackupKind::Flash1M => 3,
        BackupKind::Eeprom => 4,
    }
}
