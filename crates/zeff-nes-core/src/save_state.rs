use anyhow::{Result, anyhow, bail};

pub const NES_SAVE_STATE_MAGIC: [u8; 8] = *b"ZBNSTATE";
pub const NES_SAVE_STATE_FORMAT_VERSION: u32 = 1;

pub struct StateWriter {
    bytes: Vec<u8>,
}

impl StateWriter {
    pub fn new() -> Self {
        Self {
            bytes: Vec::with_capacity(16 * 1024),
        }
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

    pub fn write_f64(&mut self, value: f64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub fn write_vec(&mut self, data: &[u8]) {
        self.write_u32(data.len() as u32);
        self.write_bytes(data);
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
            .ok_or_else(|| anyhow!("save-state offset overflow"))?;
        if end > self.bytes.len() {
            bail!("save-state data is truncated");
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
            other => bail!("invalid boolean value in save-state: {other}"),
        }
    }

    pub fn read_u16(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        buf.copy_from_slice(self.take(2)?);
        Ok(u16::from_le_bytes(buf))
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        buf.copy_from_slice(self.take(4)?);
        Ok(u32::from_le_bytes(buf))
    }

    pub fn read_u64(&mut self) -> Result<u64> {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(self.take(8)?);
        Ok(u64::from_le_bytes(buf))
    }

    pub fn read_f64(&mut self) -> Result<f64> {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(self.take(8)?);
        Ok(f64::from_le_bytes(buf))
    }

    pub fn read_exact(&mut self, out: &mut [u8]) -> Result<()> {
        out.copy_from_slice(self.take(out.len())?);
        Ok(())
    }

    /// Read a length-prefixed byte vector, rejecting anything beyond `max_len`.
    pub fn read_vec(&mut self, max_len: usize) -> Result<Vec<u8>> {
        let len = self.read_u32()? as usize;
        if len > max_len {
            bail!("save-state vector length {len} exceeds maximum {max_len}");
        }
        Ok(self.take(len)?.to_vec())
    }
}

pub fn encode_mirroring(m: crate::hardware::cartridge::Mirroring) -> u8 {
    use crate::hardware::cartridge::Mirroring;
    match m {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::SingleScreenLower => 2,
        Mirroring::SingleScreenUpper => 3,
        Mirroring::FourScreen => 4,
    }
}

pub fn decode_mirroring(tag: u8) -> Result<crate::hardware::cartridge::Mirroring> {
    use crate::hardware::cartridge::Mirroring;
    match tag {
        0 => Ok(Mirroring::Horizontal),
        1 => Ok(Mirroring::Vertical),
        2 => Ok(Mirroring::SingleScreenLower),
        3 => Ok(Mirroring::SingleScreenUpper),
        4 => Ok(Mirroring::FourScreen),
        _ => bail!("invalid mirroring tag in save-state: {tag}"),
    }
}

// ─── Top-level encode / decode ─────────────────────────────────────

pub fn encode_state(emu: &crate::emulator::Emulator) -> Result<Vec<u8>> {
    let mut w = StateWriter::new();
    w.write_bytes(&NES_SAVE_STATE_MAGIC);
    w.write_u32(NES_SAVE_STATE_FORMAT_VERSION);
    w.write_bytes(&emu.rom_hash);

    // CPU
    emu.cpu.write_state(&mut w);
    // Bus (PPU, APU, Cartridge, RAM, Controllers)
    emu.bus.write_state(&mut w);

    Ok(w.into_bytes())
}

pub fn decode_state(
    emu: &mut crate::emulator::Emulator,
    bytes: &[u8],
) -> Result<()> {
    let mut r = StateReader::new(bytes);

    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if magic != NES_SAVE_STATE_MAGIC {
        bail!("not a valid NES save-state (bad magic)");
    }

    let format_version = r.read_u32()?;
    if format_version != NES_SAVE_STATE_FORMAT_VERSION {
        bail!(
            "unsupported NES save-state format version {} (expected {})",
            format_version,
            NES_SAVE_STATE_FORMAT_VERSION
        );
    }

    let mut rom_hash = [0u8; 32];
    r.read_exact(&mut rom_hash)?;
    if rom_hash != emu.rom_hash {
        bail!("save-state ROM hash does not match the currently loaded ROM");
    }

    // CPU
    emu.cpu.read_state(&mut r)?;
    // Bus
    emu.bus.read_state(&mut r)?;

    if !r.is_exhausted() {
        bail!("save-state has unexpected trailing data");
    }

    Ok(())
}

const SAVE_STATE_EXTENSION: &str = "nstate";
const SAVE_SYSTEM_SUBDIR: &str = "nes";

fn rom_hash_hex(hash: [u8; 32]) -> String {
    hash.iter().map(|b| format!("{b:02x}")).collect()
}

fn save_root_path() -> std::path::PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        return config_dir.join("zeff-boy").join("saves");
    }

    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("saves")
}

fn save_dir_path() -> std::path::PathBuf {
    save_root_path().join(SAVE_SYSTEM_SUBDIR)
}

fn validate_slot(slot: u8) -> Result<()> {
    if slot > 9 {
        bail!("invalid save-state slot {slot} (must be 0–9)");
    }
    Ok(())
}

pub fn slot_path(rom_hash: [u8; 32], slot: u8) -> Result<std::path::PathBuf> {
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

pub fn auto_save_path(rom_hash: [u8; 32]) -> std::path::PathBuf {
    let mut path = save_dir_path();
    path.push(format!(
        "{}_auto.{}",
        rom_hash_hex(rom_hash),
        SAVE_STATE_EXTENSION
    ));
    path
}


pub fn write_state_bytes_to_file(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow!("failed to create save-state directory: {e}"))?;
    }
    std::fs::write(path, bytes)
        .map_err(|e| anyhow!("failed to write save-state file {}: {e}", path.display()))?;
    Ok(())
}
