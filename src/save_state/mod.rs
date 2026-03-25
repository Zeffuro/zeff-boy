use anyhow::{Context, Result, anyhow, bail};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod bess;

pub(crate) use bess::{has_bess_footer, import_bess};

use crate::hardware::bus::Bus;
use crate::hardware::cpu::CPU;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};

pub(crate) const SAVE_STATE_VERSION: u32 = 1;
pub(crate) const SAVE_STATE_FORMAT_VERSION: u32 = 2;
const SAVE_STATE_EXTENSION: &str = "state";
pub(crate) const SAVE_STATE_MAGIC: [u8; 8] = *b"ZBSTATE\0";
const SAVE_STATE_DECODE_STACK_SIZE: usize = 8 * 1024 * 1024;

pub(crate) struct StateWriter {
    bytes: Vec<u8>,
}

impl StateWriter {
    pub(crate) fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub(crate) fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub(crate) fn write_bool(&mut self, value: bool) {
        self.write_u8(u8::from(value));
    }

    pub(crate) fn write_u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub(crate) fn write_len(&mut self, len: usize) {
        self.write_u32(len as u32);
    }

    pub(crate) fn position(&self) -> usize {
        self.bytes.len()
    }
}

pub(crate) struct StateReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> StateReader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(crate) fn is_exhausted(&self) -> bool {
        self.offset >= self.bytes.len()
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| anyhow!("save-state file offset overflow"))?;
        if end > self.bytes.len() {
            bail!("save-state file is truncated")
        }
        let slice = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(slice)
    }

    pub(crate) fn read_u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub(crate) fn read_bool(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => bail!("invalid boolean value in save-state file: {other}"),
        }
    }

    pub(crate) fn read_u16(&mut self) -> Result<u16> {
        let mut bytes = [0u8; 2];
        bytes.copy_from_slice(self.take(2)?);
        Ok(u16::from_le_bytes(bytes))
    }

    pub(crate) fn read_u32(&mut self) -> Result<u32> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(bytes))
    }

    pub(crate) fn read_u64(&mut self) -> Result<u64> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(bytes))
    }

    pub(crate) fn read_exact(&mut self, out: &mut [u8]) -> Result<()> {
        out.copy_from_slice(self.take(out.len())?);
        Ok(())
    }

    pub(crate) fn read_vec(&mut self, max_len: usize) -> Result<Vec<u8>> {
        let len = self.read_u32()? as usize;
        if len > max_len {
            bail!("save-state vector length {len} exceeds maximum {max_len}")
        }
        Ok(self.take(len)?.to_vec())
    }
}

pub(crate) struct SaveState {
    pub(crate) version: u32,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) cpu: CPU,
    pub(crate) bus: Bus,
    pub(crate) hardware_mode_preference: HardwareModePreference,
    pub(crate) hardware_mode: HardwareMode,
    pub(crate) cycle_count: u64,
    pub(crate) last_opcode: u8,
    pub(crate) last_opcode_pc: u16,
}

pub(crate) struct SaveStateRef<'a> {
    pub(crate) version: u32,
    pub(crate) rom_hash: [u8; 32],
    pub(crate) cpu: &'a CPU,
    pub(crate) bus: &'a Bus,
    pub(crate) hardware_mode_preference: HardwareModePreference,
    pub(crate) hardware_mode: HardwareMode,
    pub(crate) cycle_count: u64,
    pub(crate) last_opcode: u8,
    pub(crate) last_opcode_pc: u16,
}

pub(crate) fn slot_path(rom_hash: [u8; 32], slot: u8) -> Result<PathBuf> {
    validate_slot(slot)?;
    let mut path = save_dir_path();
    path.push(format!(
        "{}_slot{}.{}",
        rom_hash_hex(rom_hash),
        slot,
        SAVE_STATE_EXTENSION
    ));
    Ok(path)
}

pub(crate) fn auto_save_path(rom_hash: [u8; 32]) -> PathBuf {
    let mut path = save_dir_path();
    path.push(format!(
        "{}_auto.{}",
        rom_hash_hex(rom_hash),
        SAVE_STATE_EXTENSION
    ));
    path
}

pub(crate) fn write_to_file(path: &Path, state: &SaveStateRef<'_>) -> Result<()> {
    let bytes = encode_state_bytes(state)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create save-state directory")?;
    }

    let tmp_path = temp_path_for(path);
    {
        let mut file = File::create(&tmp_path)
            .with_context(|| format!("failed to create temp save state: {}", tmp_path.display()))?;
        file.write_all(&bytes)
            .with_context(|| format!("failed to write temp save state: {}", tmp_path.display()))?;
        file.sync_all()
            .with_context(|| format!("failed to flush temp save state: {}", tmp_path.display()))?;
    }

    if path.exists() {
        let _ = fs::remove_file(path);
    }

    fs::rename(&tmp_path, path)
        .with_context(|| format!("failed to finalize save state: {}", path.display()))
}

pub(crate) fn decode_on_thread(bytes: Vec<u8>) -> Result<SaveState> {
    let decode_thread = std::thread::Builder::new()
        .name("save-state-decode".to_string())
        .stack_size(SAVE_STATE_DECODE_STACK_SIZE)
        .spawn(move || decode_state(&bytes))
        .context("failed to spawn save-state decode thread")?;

    decode_thread
        .join()
        .map_err(|_| anyhow!("save-state decode thread panicked"))?
}

pub(crate) fn read_from_file(path: &Path) -> Result<SaveState> {
    let bytes =
        fs::read(path).with_context(|| format!("failed to read save state: {}", path.display()))?;

    decode_on_thread(bytes)
}

fn temp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let ext = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or(SAVE_STATE_EXTENSION);
    tmp.set_extension(format!("{ext}.tmp.{suffix}"));
    tmp
}

pub(crate) fn encode_state_bytes(state: &SaveStateRef<'_>) -> Result<Vec<u8>> {
    let mut writer = StateWriter::new();
    writer.write_bytes(&SAVE_STATE_MAGIC);
    writer.write_u32(SAVE_STATE_FORMAT_VERSION);

    writer.write_u32(state.version);
    writer.write_bytes(&state.rom_hash);
    state.cpu.write_state(&mut writer);
    writer.write_u8(encode_mode_preference(state.hardware_mode_preference));
    writer.write_u8(encode_hardware_mode(state.hardware_mode));
    writer.write_u64(state.cycle_count);
    writer.write_u8(state.last_opcode);
    writer.write_u16(state.last_opcode_pc);
    state.bus.write_state(&mut writer);

    bess::append_bess(&mut writer, state.cpu, state.bus, state.hardware_mode)?;

    Ok(writer.into_bytes())
}

fn decode_state(bytes: &[u8]) -> Result<SaveState> {
    let mut reader = StateReader::new(bytes);

    let mut magic = [0u8; 8];
    reader.read_exact(&mut magic)?;
    if magic != SAVE_STATE_MAGIC {
        bail!("invalid save-state file header");
    }

    let format_version = reader.read_u32()?;
    if format_version != SAVE_STATE_FORMAT_VERSION {
        bail!(
            "unsupported save-state file format {} (expected {})",
            format_version,
            SAVE_STATE_FORMAT_VERSION
        );
    }

    let version = reader.read_u32()?;
    let mut rom_hash = [0u8; 32];
    reader.read_exact(&mut rom_hash)?;
    let cpu = CPU::read_state(&mut reader)?;
    let hardware_mode_preference = decode_mode_preference(reader.read_u8()?)?;
    let hardware_mode = decode_hardware_mode(reader.read_u8()?)?;
    let cycle_count = reader.read_u64()?;
    let last_opcode = reader.read_u8()?;
    let last_opcode_pc = reader.read_u16()?;
    let bus = Bus::read_state(&mut reader)?;

    if !reader.is_exhausted() && !bess::has_bess_footer(bytes) {
        bail!("save-state file has unexpected trailing data");
    }

    Ok(SaveState {
        version,
        rom_hash,
        cpu,
        bus,
        hardware_mode_preference,
        hardware_mode,
        cycle_count,
        last_opcode,
        last_opcode_pc,
    })
}

fn encode_hardware_mode(mode: HardwareMode) -> u8 {
    match mode {
        HardwareMode::DMG => 0,
        HardwareMode::SGB1 => 1,
        HardwareMode::SGB2 => 2,
        HardwareMode::CGBNormal => 3,
        HardwareMode::CGBDouble => 4,
    }
}

pub(crate) fn decode_hardware_mode(tag: u8) -> Result<HardwareMode> {
    match tag {
        0 => Ok(HardwareMode::DMG),
        1 => Ok(HardwareMode::SGB1),
        2 => Ok(HardwareMode::SGB2),
        3 => Ok(HardwareMode::CGBNormal),
        4 => Ok(HardwareMode::CGBDouble),
        _ => bail!("invalid hardware mode tag in save-state file: {tag}"),
    }
}

fn encode_mode_preference(pref: HardwareModePreference) -> u8 {
    match pref {
        HardwareModePreference::Auto => 0,
        HardwareModePreference::ForceDmg => 1,
        HardwareModePreference::ForceCgb => 2,
    }
}

fn decode_mode_preference(tag: u8) -> Result<HardwareModePreference> {
    match tag {
        0 => Ok(HardwareModePreference::Auto),
        1 => Ok(HardwareModePreference::ForceDmg),
        2 => Ok(HardwareModePreference::ForceCgb),
        _ => bail!("invalid hardware mode preference tag in save-state file: {tag}"),
    }
}

fn validate_slot(slot: u8) -> Result<()> {
    if slot <= 9 {
        Ok(())
    } else {
        bail!("invalid save-state slot {}; expected 0..=9", slot);
    }
}

fn save_dir_path() -> PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        return config_dir.join("zeff-boy").join("saves");
    }

    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("saves")
}

fn rom_hash_hex(hash: [u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in hash {
        out.push(hex_nibble(byte >> 4));
        out.push(hex_nibble(byte & 0x0F));
    }
    out
}

fn hex_nibble(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'a' + (nibble - 10)) as char,
        _ => unreachable!(),
    }
}

pub(crate) fn validate_compatibility(state: &SaveState, expected_rom_hash: [u8; 32]) -> Result<()> {
    if state.version != SAVE_STATE_VERSION {
        return Err(anyhow!(
            "unsupported save-state version {} (expected {})",
            state.version,
            SAVE_STATE_VERSION
        ));
    }

    if state.rom_hash != expected_rom_hash {
        return Err(anyhow!(
            "save state ROM hash does not match currently loaded ROM"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        SAVE_STATE_FORMAT_VERSION, SAVE_STATE_MAGIC, SAVE_STATE_VERSION, SaveStateRef,
        decode_state, encode_state_bytes,
    };
    use crate::hardware::bus::Bus;
    use crate::hardware::cpu::CPU;
    use crate::hardware::rom_header::RomHeader;
    use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn decode_rejects_bad_magic() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"BADMAGIC");
        bytes.extend_from_slice(&SAVE_STATE_FORMAT_VERSION.to_le_bytes());
        let err = decode_state(&bytes)
            .err()
            .expect("bad magic should be rejected")
            .to_string();
        assert!(err.contains("invalid save-state file header"));
    }

    #[test]
    fn decode_rejects_unknown_format_version() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&SAVE_STATE_MAGIC);
        bytes.extend_from_slice(&(SAVE_STATE_FORMAT_VERSION + 1).to_le_bytes());

        let err = decode_state(&bytes)
            .err()
            .expect("unknown format version should be rejected")
            .to_string();
        assert!(err.contains("unsupported save-state file format"));
    }

    #[test]
    fn full_save_state_round_trip_handles_large_arrays() {
        let rom = vec![0u8; 0x8000];
        let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
        let bus = *Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize");

        let cpu = CPU::new();
        let state = SaveStateRef {
            version: SAVE_STATE_VERSION,
            rom_hash: [0xAB; 32],
            cpu: &cpu,
            bus: &bus,
            hardware_mode_preference: HardwareModePreference::Auto,
            hardware_mode: HardwareMode::DMG,
            cycle_count: 123,
            last_opcode: 0x00,
            last_opcode_pc: 0x0100,
        };

        let mut path = PathBuf::from(std::env::temp_dir());
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        path.push(format!("zeff-boy-save-state-roundtrip-{unique}.state"));

        super::write_to_file(&path, &state).expect("serialize full save-state should succeed");
        let restored =
            super::read_from_file(&path).expect("deserialize full save-state should succeed");
        let _ = std::fs::remove_file(&path);

        assert_eq!(restored.rom_hash, state.rom_hash);
        assert_eq!(restored.hardware_mode, state.hardware_mode);
        assert_eq!(restored.bus.vram, bus.vram);
        assert_eq!(restored.bus.wram, bus.wram);
        assert!(restored.bus.io.ppu.framebuffer.iter().all(|&b| b == 0));
    }

    #[test]
    fn encoded_state_has_bess_footer() {
        let rom = vec![0u8; 0x8000];
        let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
        let bus = *Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize");
        let cpu = CPU::new();
        let state = SaveStateRef {
            version: SAVE_STATE_VERSION,
            rom_hash: [0xAB; 32],
            cpu: &cpu,
            bus: &bus,
            hardware_mode_preference: HardwareModePreference::Auto,
            hardware_mode: HardwareMode::DMG,
            cycle_count: 0,
            last_opcode: 0x00,
            last_opcode_pc: 0x0100,
        };

        let bytes = encode_state_bytes(&state).expect("encode should succeed");

        assert!(bytes.len() >= 8);
        assert_eq!(&bytes[bytes.len() - 4..], b"BESS");

        let footer_offset = bytes.len() - 8;
        let first_block_offset =
            u32::from_le_bytes(bytes[footer_offset..footer_offset + 4].try_into().unwrap());
        assert!(first_block_offset < bytes.len() as u32);

        let name_id = &bytes[first_block_offset as usize..first_block_offset as usize + 4];
        assert_eq!(name_id, b"NAME");
    }

    #[test]
    fn bess_footer_does_not_break_native_decode() {
        let rom = vec![0u8; 0x8000];
        let header = RomHeader::from_rom(&rom).expect("test ROM header should parse");
        let bus = *Bus::new(rom, &header, HardwareMode::DMG).expect("test bus should initialize");
        let cpu = CPU::new();
        let state = SaveStateRef {
            version: SAVE_STATE_VERSION,
            rom_hash: [0xCD; 32],
            cpu: &cpu,
            bus: &bus,
            hardware_mode_preference: HardwareModePreference::Auto,
            hardware_mode: HardwareMode::DMG,
            cycle_count: 42,
            last_opcode: 0x76,
            last_opcode_pc: 0x0200,
        };

        let bytes = encode_state_bytes(&state).expect("encode should succeed");
        let restored = decode_state(&bytes).expect("decode should succeed with BESS trailing data");

        assert_eq!(restored.rom_hash, [0xCD; 32]);
        assert_eq!(restored.cycle_count, 42);
        assert_eq!(restored.last_opcode, 0x76);
        assert_eq!(restored.last_opcode_pc, 0x0200);
    }
}
