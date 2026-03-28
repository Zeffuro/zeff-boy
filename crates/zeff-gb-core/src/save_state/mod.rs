use anyhow::{Context, Result, anyhow, bail};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

mod bess;

pub use bess::{has_bess_footer, import_bess};

use crate::hardware::bus::Bus;
use crate::hardware::cpu::Cpu;
use crate::hardware::types::hardware_mode::{HardwareMode, HardwareModePreference};

pub const SAVE_STATE_VERSION: u32 = 1;
pub const SAVE_STATE_FORMAT_VERSION: u32 = 2;
pub const SAVE_STATE_MAGIC: [u8; 8] = *b"ZBSTATE\0";
const SAVE_STATE_DECODE_STACK_SIZE: usize = 8 * 1024 * 1024;

pub struct StateWriter {
    bytes: Vec<u8>,
}

impl Default for StateWriter {
    fn default() -> Self {
        Self::new()
    }
}

impl StateWriter {
    pub fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    pub fn write_u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    pub fn write_bool(&mut self, value: bool) {
        self.write_u8(u8::from(value));
    }

    pub fn write_u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn write_len(&mut self, len: usize) {
        self.write_u32(len as u32);
    }

    pub fn write_hardware_mode(&mut self, mode: HardwareMode) {
        self.write_u8(encode_hardware_mode(mode));
    }

    pub fn position(&self) -> usize {
        self.bytes.len()
    }
}

pub struct StateReader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> StateReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub fn is_exhausted(&self) -> bool {
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

    pub fn read_u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }

    pub fn read_bool(&mut self) -> Result<bool> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            other => bail!("invalid boolean value in save-state file: {other}"),
        }
    }

    pub fn read_u16(&mut self) -> Result<u16> {
        let mut bytes = [0u8; 2];
        bytes.copy_from_slice(self.take(2)?);
        Ok(u16::from_le_bytes(bytes))
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(bytes))
    }

    pub fn read_u64(&mut self) -> Result<u64> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(bytes))
    }

    pub fn read_exact(&mut self, out: &mut [u8]) -> Result<()> {
        out.copy_from_slice(self.take(out.len())?);
        Ok(())
    }

    pub fn read_vec(&mut self, max_len: usize) -> Result<Vec<u8>> {
        let len = self.read_u32()? as usize;
        if len > max_len {
            bail!("save-state vector length {len} exceeds maximum {max_len}")
        }
        Ok(self.take(len)?.to_vec())
    }

    pub fn read_hardware_mode(&mut self) -> Result<HardwareMode> {
        decode_hardware_mode(self.read_u8()?)
    }
}

pub struct SaveState {
    pub version: u32,
    pub rom_hash: [u8; 32],
    pub cpu: Cpu,
    pub bus: Bus,
    pub hardware_mode_preference: HardwareModePreference,
    pub hardware_mode: HardwareMode,
    pub cycle_count: u64,
    pub last_opcode: u8,
    pub last_opcode_pc: u16,
}

pub struct SaveStateRef<'a> {
    pub version: u32,
    pub rom_hash: [u8; 32],
    pub cpu: &'a Cpu,
    pub bus: &'a Bus,
    pub hardware_mode_preference: HardwareModePreference,
    pub hardware_mode: HardwareMode,
    pub cycle_count: u64,
    pub last_opcode: u8,
    pub last_opcode_pc: u16,
}


pub fn write_to_file(path: &Path, state: &SaveStateRef<'_>) -> Result<()> {
    let bytes = encode_state_bytes(state)?;
    write_state_bytes_to_file(path, &bytes)
}

pub fn write_state_bytes_to_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("failed to create save-state directory")?;
    }

    let tmp_path = temp_path_for(path);
    {
        let mut file = File::create(&tmp_path)
            .with_context(|| format!("failed to create temp save state: {}", tmp_path.display()))?;
        file.write_all(bytes)
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

pub fn decode_on_thread(bytes: Vec<u8>) -> Result<SaveState> {
    let decode_thread = std::thread::Builder::new()
        .name("save-state-decode".to_string())
        .stack_size(SAVE_STATE_DECODE_STACK_SIZE)
        .spawn(move || decode_state(&bytes))
        .context("failed to spawn save-state decode thread")?;

    decode_thread
        .join()
        .map_err(|_| anyhow!("save-state decode thread panicked"))?
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
        .unwrap_or("state");
    tmp.set_extension(format!("{ext}.tmp.{suffix}"));
    tmp
}

pub fn encode_state_bytes(state: &SaveStateRef<'_>) -> Result<Vec<u8>> {
    let mut writer = StateWriter::new();
    writer.write_bytes(&SAVE_STATE_MAGIC);
    writer.write_u32(SAVE_STATE_FORMAT_VERSION);

    writer.write_u32(state.version);
    writer.write_bytes(&state.rom_hash);
    state.cpu.write_state(&mut writer);
    writer.write_u8(encode_mode_preference(state.hardware_mode_preference));
    writer.write_hardware_mode(state.hardware_mode);
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
    let cpu = Cpu::read_state(&mut reader)?;
    let hardware_mode_preference = decode_mode_preference(reader.read_u8()?)?;
    let hardware_mode = reader.read_hardware_mode()?;
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

pub fn encode_hardware_mode(mode: HardwareMode) -> u8 {
    match mode {
        HardwareMode::DMG => 0,
        HardwareMode::SGB1 => 1,
        HardwareMode::SGB2 => 2,
        HardwareMode::CGBNormal => 3,
        HardwareMode::CGBDouble => 4,
    }
}

pub fn decode_hardware_mode(tag: u8) -> Result<HardwareMode> {
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

pub fn validate_compatibility(state: &SaveState, expected_rom_hash: [u8; 32]) -> Result<()> {
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
mod tests;
