mod export;
mod import;

pub use export::append_bess;
pub use import::import_bess;

use std::time::{SystemTime, UNIX_EPOCH};

use crate::hardware::types::hardware_mode::HardwareMode;

pub(super) const BESS_MAGIC: &[u8; 4] = b"BESS";
pub(super) const BESS_MAJOR: u16 = 1;
pub(super) const BESS_MINOR: u16 = 1;
pub(super) const EMULATOR_NAME: &[u8] = b"zeff-boy";

pub(super) const BLOCK_NAME: [u8; 4] = *b"NAME";
pub(super) const BLOCK_INFO: [u8; 4] = *b"INFO";
pub(super) const BLOCK_CORE: [u8; 4] = *b"CORE";
pub(super) const BLOCK_MBC: [u8; 4] = *b"MBC ";
pub(super) const BLOCK_RTC: [u8; 4] = *b"RTC ";
pub(super) const BLOCK_END: [u8; 4] = *b"END ";

pub(super) const CORE_BLOCK_LEN: u32 = 0xD0;
pub(super) const INFO_BLOCK_LEN: u32 = 0x12;
pub(super) const RTC_BLOCK_LEN: u32 = 0x30;

pub fn has_bess_footer(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[bytes.len() - 4..] == BESS_MAGIC
}

pub(super) fn write_block_header(writer: &mut super::StateWriter, id: &[u8; 4], len: u32) {
    writer.write_bytes(id);
    writer.write_u32(len);
}

pub(super) fn mode_to_bess_model(mode: HardwareMode) -> [u8; 4] {
    match mode {
        HardwareMode::DMG => *b"GD  ",
        HardwareMode::SGB1 => *b"SN  ",
        HardwareMode::SGB2 => *b"S2  ",
        HardwareMode::CGBNormal | HardwareMode::CGBDouble => *b"CC  ",
    }
}

pub(super) fn bess_model_to_mode(model: &[u8], core: &[u8]) -> anyhow::Result<HardwareMode> {
    match model[0] {
        b'G' => Ok(HardwareMode::DMG),
        b'S' => {
            if model.len() >= 2 && model[1] == b'2' {
                Ok(HardwareMode::SGB2)
            } else {
                Ok(HardwareMode::SGB1)
            }
        }
        b'C' => {
            let key1 = core[0x18 + 0x4D];
            if key1 & 0x80 != 0 {
                Ok(HardwareMode::CGBDouble)
            } else {
                Ok(HardwareMode::CGBNormal)
            }
        }
        _ => anyhow::bail!("unknown BESS model family '{}'", char::from(model[0])),
    }
}

pub(super) fn read_u16_le(b: &[u8]) -> u16 {
    u16::from_le_bytes([b[0], b[1]])
}

pub(super) fn read_u32_le(b: &[u8]) -> u32 {
    u32::from_le_bytes([b[0], b[1], b[2], b[3]])
}

pub(super) fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(super) fn copy_buffer(file: &[u8], offset: usize, size: usize, dest: &mut [u8]) {
    if size == 0 || offset + size > file.len() {
        return;
    }
    let copy_len = size.min(dest.len());
    dest[..copy_len].copy_from_slice(&file[offset..offset + copy_len]);

    for b in &mut dest[copy_len..] {
        *b = 0;
    }
}
