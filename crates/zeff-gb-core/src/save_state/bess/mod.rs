mod export;
mod import;

pub use import::import_bess;

use std::ops::Range;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow, bail};

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
// Zeff-boy-private BESS extension. BESS requires readers to ignore unsupported
// four-character blocks, so portable BESS import must never depend on this
pub(super) const BLOCK_ZEFF_PRIVATE_EXTENSION: [u8; 4] = *b"ZBEX";
pub(super) const BLOCK_END: [u8; 4] = *b"END ";

pub(super) const CORE_BLOCK_LEN: u32 = 0xD0;
pub(super) const INFO_BLOCK_LEN: u32 = 0x12;
pub(super) const RTC_BLOCK_LEN: u32 = 0x30;
pub(super) const ZEFF_EXTENSION_FRAMEBUFFER_LEN: usize =
    crate::hardware::ppu::SCREEN_W * crate::hardware::ppu::SCREEN_H * 4;
pub(super) const ZEFF_EXTENSION_BLOCK_LEN: u32 = (8 + ZEFF_EXTENSION_FRAMEBUFFER_LEN) as u32;

const NATIVE_SAVE_STATE_MAGIC: &[u8; 8] = b"ZBSTATE\0";
const NATIVE_LAST_OPCODE_OFFSET: usize = 97;
const NATIVE_LAST_OPCODE_PC_OFFSET: usize = 98;
const NATIVE_LAST_OPCODE_END: usize = NATIVE_LAST_OPCODE_PC_OFFSET + 2;

pub fn has_bess_footer(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && &bytes[bytes.len() - 4..] == BESS_MAGIC
}

pub(super) fn append_bess_with_optional_zeff_extension(
    writer: &mut super::StateWriter,
    cpu: &crate::hardware::cpu::Cpu,
    bus: &crate::hardware::bus::Bus,
    hardware_mode: HardwareMode,
    frame_count: Option<u64>,
) -> Result<()> {
    export::append_bess_with_optional_zeff_extension(writer, cpu, bus, hardware_mode, frame_count)
}

pub(super) struct ZeffExtension {
    pub(super) frame_count: u64,
    pub(super) lcd_framebuffer: Box<[u8]>,
}

struct LocatedZeffExtension {
    extension: ZeffExtension,
    block_range: Range<usize>,
}

pub(super) fn required_zeff_extension(
    bytes: &[u8],
    minimum_first_block_offset: usize,
) -> Result<ZeffExtension> {
    locate_zeff_extension(bytes, minimum_first_block_offset)?
        .map(|located| located.extension)
        .ok_or_else(|| {
            anyhow!("native save state is missing required Zeff-private `ZBEX` BESS extension")
        })
}

pub fn project_replay_state_bytes(bytes: &mut Vec<u8>) -> Result<()> {
    if !bytes.starts_with(NATIVE_SAVE_STATE_MAGIC) {
        return Ok(());
    }
    if bytes.len() < 12 {
        bail!("native save-state header is truncated");
    }

    let format_version = read_u32_le(&bytes[8..12]);
    if format_version <= 12 {
        return Ok(());
    }
    if !matches!(format_version, 13 | 14) {
        bail!("unsupported native save-state format {format_version} for replay projection");
    }

    let located = locate_zeff_extension(bytes, NATIVE_LAST_OPCODE_END)?.ok_or_else(|| {
        anyhow!("native save state is missing required Zeff-private `ZBEX` BESS extension")
    })?;
    let first_block_offset = bess_first_block_offset(bytes)?;
    let continuation_range = if format_version >= 14 {
        let native_payload_end = bess_mbc_ram_offset(bytes, first_block_offset)?;
        Some(sgb_native_continuation_range(bytes, native_payload_end)?)
    } else {
        None
    };
    bytes.drain(located.block_range);
    if let Some(continuation_range) = continuation_range {
        let continuation_start = continuation_range.start;
        bytes.drain(continuation_range);
        adjust_bess_offsets_after_removal(
            bytes,
            first_block_offset,
            continuation_start,
            super::SGB_NATIVE_CONTINUATION_BLOCK_LEN,
        )?;
    }
    bytes[8..12].copy_from_slice(&12u32.to_le_bytes());
    Ok(())
}

fn bess_first_block_offset(bytes: &[u8]) -> Result<usize> {
    if !has_bess_footer(bytes) {
        bail!("native save state is missing BESS footer");
    }
    let footer_start = bytes.len() - 8;
    Ok(read_u32_le(&bytes[footer_start..footer_start + 4]) as usize)
}

fn bess_mbc_ram_offset(bytes: &[u8], first_block_offset: usize) -> Result<usize> {
    let footer_start = bytes.len() - 8;
    let mut pos = first_block_offset;
    while pos < footer_start {
        let header_end = pos
            .checked_add(8)
            .ok_or_else(|| anyhow!("BESS block header offset overflow"))?;
        if header_end > footer_start {
            bail!("BESS block header truncated");
        }
        let len = read_u32_le(&bytes[pos + 4..header_end]) as usize;
        let data_end = header_end
            .checked_add(len)
            .ok_or_else(|| anyhow!("BESS block data offset overflow"))?;
        if data_end > footer_start {
            bail!("BESS block data truncated");
        }
        if bytes[pos..pos + 4] == BLOCK_CORE {
            if len != CORE_BLOCK_LEN as usize {
                bail!("invalid BESS CORE block length");
            }
            return Ok(read_u32_le(&bytes[header_end + 0xAC..header_end + 0xB0]) as usize);
        }
        pos = data_end;
    }
    bail!("native save state is missing BESS CORE block")
}

fn sgb_native_continuation_range(bytes: &[u8], native_payload_end: usize) -> Result<Range<usize>> {
    let start = native_payload_end
        .checked_sub(super::SGB_NATIVE_CONTINUATION_BLOCK_LEN)
        .ok_or_else(|| anyhow!("SGB native continuation offset underflow"))?;
    let header_end = start + 8;
    if &bytes[start..start + 4] != super::SGB_NATIVE_CONTINUATION_MAGIC.as_slice() {
        bail!("invalid SGB native continuation header");
    }
    let payload_len = read_u32_le(&bytes[start + 4..header_end]) as usize;
    if payload_len != super::SGB_NATIVE_CONTINUATION_PAYLOAD_LEN {
        bail!("invalid SGB native continuation length {payload_len}");
    }
    Ok(start..native_payload_end)
}

fn adjust_bess_offsets_after_removal(
    bytes: &mut [u8],
    original_first_block_offset: usize,
    removed_start: usize,
    removed_len: usize,
) -> Result<()> {
    let footer_start = bytes.len() - 8;
    let first_block_offset = original_first_block_offset
        .checked_sub(removed_len)
        .ok_or_else(|| anyhow!("BESS first-block offset underflow"))?;
    bytes[footer_start..footer_start + 4]
        .copy_from_slice(&(first_block_offset as u32).to_le_bytes());

    let mut pos = first_block_offset;
    while pos < footer_start {
        let header_end = pos
            .checked_add(8)
            .ok_or_else(|| anyhow!("BESS block header offset overflow"))?;
        if header_end > footer_start {
            bail!("BESS block header truncated");
        }
        let len = read_u32_le(&bytes[pos + 4..header_end]) as usize;
        let data_end = header_end
            .checked_add(len)
            .ok_or_else(|| anyhow!("BESS block data offset overflow"))?;
        if data_end > footer_start {
            bail!("BESS block data truncated");
        }
        if bytes[pos..pos + 4] == BLOCK_CORE {
            if len != CORE_BLOCK_LEN as usize {
                bail!("invalid BESS CORE block length");
            }
            for offset in [0x9C, 0xA4, 0xAC, 0xB4, 0xBC, 0xC4, 0xCC] {
                let range = header_end + offset..header_end + offset + 4;
                let value = read_u32_le(&bytes[range.clone()]) as usize;
                if value >= removed_start && value != 0 {
                    let adjusted = value
                        .checked_sub(removed_len)
                        .ok_or_else(|| anyhow!("BESS memory offset underflow"))?;
                    bytes[range].copy_from_slice(&(adjusted as u32).to_le_bytes());
                }
            }
            return Ok(());
        }
        pos = data_end;
    }
    bail!("native save state is missing BESS CORE block")
}

fn locate_zeff_extension(
    bytes: &[u8],
    minimum_first_block_offset: usize,
) -> Result<Option<LocatedZeffExtension>> {
    if !has_bess_footer(bytes) {
        bail!("native save state is missing BESS footer");
    }

    let footer_start = bytes.len() - 8;
    let first_offset = read_u32_le(&bytes[footer_start..footer_start + 4]) as usize;
    if first_offset < minimum_first_block_offset {
        bail!("BESS first-block offset precedes completed native payload");
    }
    if first_offset >= footer_start {
        bail!("BESS first-block offset out of range");
    }

    let mut located: Option<LocatedZeffExtension> = None;
    let mut pos = first_offset;
    loop {
        let header_end = pos
            .checked_add(8)
            .ok_or_else(|| anyhow!("BESS block header offset overflow"))?;
        if header_end > footer_start {
            bail!("BESS block header truncated");
        }
        let id: [u8; 4] = bytes[pos..pos + 4].try_into().expect("slice is 4 bytes");
        let len = read_u32_le(&bytes[pos + 4..header_end]) as usize;
        let data_end = header_end
            .checked_add(len)
            .ok_or_else(|| anyhow!("BESS block data offset overflow"))?;
        if data_end > footer_start {
            bail!("BESS block data truncated");
        }

        if id == BLOCK_END {
            if len != 0 {
                bail!("BESS END block has non-zero length");
            }
            if data_end != footer_start {
                bail!("BESS data follows END block");
            }
            if let Some(extension) = &located
                && extension.block_range.end != pos
            {
                bail!("native Zeff-private `ZBEX` BESS extension must immediately precede END");
            }
            return Ok(located);
        }

        if id == BLOCK_ZEFF_PRIVATE_EXTENSION {
            if located.is_some() {
                bail!("duplicate Zeff-private `ZBEX` BESS extension");
            }
            if len != ZEFF_EXTENSION_BLOCK_LEN as usize {
                bail!(
                    "invalid Zeff-private `ZBEX` BESS extension length {len} (expected {ZEFF_EXTENSION_BLOCK_LEN})"
                );
            }
            let frame_count = u64::from_le_bytes(
                bytes[header_end..header_end + 8]
                    .try_into()
                    .expect("ZBEX frame count is 8 bytes"),
            );
            located = Some(LocatedZeffExtension {
                extension: ZeffExtension {
                    frame_count,
                    lcd_framebuffer: bytes[header_end + 8..data_end].to_vec().into_boxed_slice(),
                },
                block_range: pos..data_end,
            });
        }

        pos = data_end;
    }
}

pub fn canonicalize_replay_hash_bytes(bytes: &mut [u8]) {
    canonicalize_native_debug_observers(bytes);
    canonicalize_bess_rtc_timestamp(bytes);
}

pub fn canonicalize_bess_rtc_timestamp(bytes: &mut [u8]) {
    let Some(footer_start) = bytes.len().checked_sub(8) else {
        return;
    };
    if !has_bess_footer(bytes) {
        return;
    }

    let mut pos = read_u32_le(&bytes[footer_start..footer_start + 4]) as usize;
    if pos >= footer_start {
        return;
    }

    loop {
        let Some(header_end) = pos.checked_add(8) else {
            return;
        };
        if header_end > footer_start {
            return;
        }

        let id: [u8; 4] = bytes[pos..pos + 4].try_into().expect("slice is 4 bytes");
        let len = read_u32_le(&bytes[pos + 4..pos + 8]) as usize;
        let data_start = header_end;
        let Some(data_end) = data_start.checked_add(len) else {
            return;
        };
        if data_end > footer_start {
            return;
        }

        if id == BLOCK_RTC && len >= RTC_BLOCK_LEN as usize {
            let timestamp_start = data_start + 0x28;
            let timestamp_end = timestamp_start + 8;
            if timestamp_end <= data_end {
                bytes[timestamp_start..timestamp_end].fill(0);
            }
        }

        if id == BLOCK_END {
            return;
        }

        pos = data_end;
    }
}

fn canonicalize_native_debug_observers(bytes: &mut [u8]) {
    if bytes.len() < NATIVE_LAST_OPCODE_END || !bytes.starts_with(NATIVE_SAVE_STATE_MAGIC) {
        return;
    }

    bytes[NATIVE_LAST_OPCODE_OFFSET] = 0;
    bytes[NATIVE_LAST_OPCODE_PC_OFFSET..NATIVE_LAST_OPCODE_END].fill(0);
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

#[cfg(test)]
mod tests {
    use super::{BESS_MAGIC, BLOCK_END, BLOCK_RTC, RTC_BLOCK_LEN, canonicalize_replay_hash_bytes};
    use crate::save_state::SAVE_STATE_MAGIC;

    #[test]
    fn replay_hash_canonicalization_zeros_bess_rtc_timestamps() {
        let mut original = b"native-state-prefix".to_vec();
        let first_block_offset = original.len() as u32;
        original.extend_from_slice(&BLOCK_RTC);
        original.extend_from_slice(&RTC_BLOCK_LEN.to_le_bytes());
        original.extend_from_slice(&[0x11; 0x28]);
        original.extend_from_slice(&123_456_789u64.to_le_bytes());
        original.extend_from_slice(&BLOCK_END);
        original.extend_from_slice(&0u32.to_le_bytes());
        original.extend_from_slice(&first_block_offset.to_le_bytes());
        original.extend_from_slice(BESS_MAGIC);

        let mut different_timestamp = original.clone();
        let timestamp_start = first_block_offset as usize + 8 + 0x28;
        different_timestamp[timestamp_start..timestamp_start + 8]
            .copy_from_slice(&987_654_321u64.to_le_bytes());

        canonicalize_replay_hash_bytes(&mut original);
        canonicalize_replay_hash_bytes(&mut different_timestamp);

        assert_eq!(original, different_timestamp);
        assert_eq!(&original[timestamp_start..timestamp_start + 8], &[0; 8]);
    }

    #[test]
    fn replay_hash_canonicalization_ignores_non_bess_data() {
        let mut bytes = b"native-state-prefix".to_vec();
        let unchanged = bytes.clone();

        canonicalize_replay_hash_bytes(&mut bytes);

        assert_eq!(bytes, unchanged);
    }

    #[test]
    fn replay_hash_canonicalization_zeros_native_debug_observers() {
        let mut first = vec![0u8; 120];
        first[..SAVE_STATE_MAGIC.len()].copy_from_slice(&SAVE_STATE_MAGIC);
        first[8..12].copy_from_slice(&12u32.to_le_bytes());
        first[97] = 0xF1;
        first[98..100].copy_from_slice(&0x2FEAu16.to_le_bytes());

        let mut second = first.clone();
        second[97] = 0x00;
        second[98..100].copy_from_slice(&0x0460u16.to_le_bytes());

        canonicalize_replay_hash_bytes(&mut first);
        canonicalize_replay_hash_bytes(&mut second);

        assert_eq!(first, second);
        assert_eq!(&first[97..100], &[0, 0, 0]);
    }
}
