use anyhow::{Result, anyhow, bail};

pub const NES_SAVE_STATE_MAGIC: [u8; 8] = *b"ZBNSTATE";
pub const NES_SAVE_STATE_FORMAT_VERSION: u32 = 2;

const FORMAT_VERSION_V1_UNCOMPRESSED: u32 = 1;

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
    // Write the raw state payload (CPU + Bus)
    let mut payload = StateWriter::new();
    payload.write_bytes(&emu.rom_hash);
    emu.cpu.write_state(&mut payload);
    emu.bus.write_state(&mut payload);
    let raw_bytes = payload.into_bytes();

    // Compress payload with lz4
    let compressed = lz4_flex::compress_prepend_size(&raw_bytes);

    // Assemble final: magic + version + compressed_payload
    let mut out = Vec::with_capacity(12 + compressed.len());
    out.extend_from_slice(&NES_SAVE_STATE_MAGIC);
    out.extend_from_slice(&NES_SAVE_STATE_FORMAT_VERSION.to_le_bytes());
    out.extend_from_slice(&compressed);
    Ok(out)
}

pub fn decode_state(
    emu: &mut crate::emulator::Emulator,
    bytes: &[u8],
) -> Result<()> {
    // Read and validate the outer header (magic + version)
    if bytes.len() < 12 {
        bail!("save-state data is too short for header");
    }
    let magic = &bytes[0..8];
    if magic != NES_SAVE_STATE_MAGIC {
        bail!("not a valid NES save-state (bad magic)");
    }
    let format_version = u32::from_le_bytes(bytes[8..12].try_into().unwrap());

    // Get the payload bytes (either raw or lz4-decompressed)
    let payload: Vec<u8>;
    let payload_ref: &[u8] = match format_version {
        FORMAT_VERSION_V1_UNCOMPRESSED => {
            // V1: raw bytes after magic(8) + version(4)
            &bytes[12..]
        }
        NES_SAVE_STATE_FORMAT_VERSION => {
            // V2: lz4-compressed payload after magic(8) + version(4)
            payload = lz4_flex::decompress_size_prepended(&bytes[12..])
                .map_err(|e| anyhow!("failed to decompress save-state: {e}"))?;
            &payload
        }
        other => {
            bail!(
                "unsupported NES save-state format version {} (expected {} or {})",
                other,
                FORMAT_VERSION_V1_UNCOMPRESSED,
                NES_SAVE_STATE_FORMAT_VERSION
            );
        }
    };

    let mut r = StateReader::new(payload_ref);

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

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1;
        rom[5] = 1;
        let prg = 16;
        rom[prg]     = 0xA9;
        rom[prg + 1] = 0x42;
        rom[prg + 2] = 0x85;
        rom[prg + 3] = 0x00;
        rom[prg + 4] = 0xEA;
        rom[prg + 5] = 0xEA;
        rom[prg + 0x3FFC] = 0x00;
        rom[prg + 0x3FFD] = 0x80;
        rom
    }

    fn make_emulator() -> crate::emulator::Emulator {
        let rom = build_test_rom();
        crate::emulator::Emulator::new(&rom, 44_100.0)
            .expect("test ROM should load")
    }

    #[test]
    fn save_state_roundtrip_preserves_cpu_state() {
        let mut emu = make_emulator();

        for _ in 0..4 {
            emu.step_instruction();
        }

        let pc_before = emu.cpu.pc;
        let sp_before = emu.cpu.sp;
        let a_before = emu.cpu.regs.a;
        let x_before = emu.cpu.regs.x;
        let y_before = emu.cpu.regs.y;
        let p_before = emu.cpu.regs.p;
        let cycles_before = emu.cpu.cycles;

        let state_bytes = encode_state(&emu).expect("encode should succeed");

        emu.reset();
        assert_ne!(emu.cpu.cycles, cycles_before);

        decode_state(&mut emu, &state_bytes).expect("decode should succeed");

        assert_eq!(emu.cpu.pc, pc_before);
        assert_eq!(emu.cpu.sp, sp_before);
        assert_eq!(emu.cpu.regs.a, a_before);
        assert_eq!(emu.cpu.regs.x, x_before);
        assert_eq!(emu.cpu.regs.y, y_before);
        assert_eq!(emu.cpu.regs.p, p_before);
        assert_eq!(emu.cpu.cycles, cycles_before);
    }

    #[test]
    fn save_state_roundtrip_preserves_bus_state() {
        let mut emu = make_emulator();

        for _ in 0..4 {
            emu.step_instruction();
        }

        let ram_00_before = emu.bus.ram[0];
        assert_eq!(ram_00_before, 0x42);

        let ppu_cycles_before = emu.bus.ppu_cycles;
        let open_bus_before = emu.bus.cpu_open_bus;

        let state_bytes = encode_state(&emu).expect("encode should succeed");

        emu.bus.ram[0] = 0x00;
        emu.bus.ppu_cycles = 0;

        decode_state(&mut emu, &state_bytes).expect("decode should succeed");

        assert_eq!(emu.bus.ram[0], 0x42);
        assert_eq!(emu.bus.ppu_cycles, ppu_cycles_before);
        assert_eq!(emu.bus.cpu_open_bus, open_bus_before);
    }

    #[test]
    fn save_state_rom_hash_mismatch_rejected() {
        let mut emu = make_emulator();
        let state_bytes = encode_state(&emu).expect("encode should succeed");

        emu.rom_hash[0] ^= 0xFF;

        let result = decode_state(&mut emu, &state_bytes);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("ROM hash"),
            "error should mention ROM hash mismatch, got: {err_msg}"
        );
    }

    #[test]
    fn save_state_truncated_data_rejected() {
        let emu = make_emulator();
        let state_bytes = encode_state(&emu).expect("encode should succeed");

        let mut truncated = state_bytes[..12].to_vec();
        truncated.extend_from_slice(&[0; 4]);

        let mut emu2 = make_emulator();
        let result = decode_state(&mut emu2, &truncated);
        assert!(result.is_err(), "truncated state should fail to decode");
    }

    #[test]
    fn save_state_bad_magic_rejected() {
        let emu = make_emulator();
        let mut state_bytes = encode_state(&emu).expect("encode should succeed");

        state_bytes[0] = b'X';

        let mut emu2 = make_emulator();
        let result = decode_state(&mut emu2, &state_bytes);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("bad magic"),
            "should reject bad magic"
        );
    }

    #[test]
    fn save_state_unsupported_version_rejected() {
        let emu = make_emulator();
        let mut state_bytes = encode_state(&emu).expect("encode should succeed");

        state_bytes[8..12].copy_from_slice(&99u32.to_le_bytes());

        let mut emu2 = make_emulator();
        let result = decode_state(&mut emu2, &state_bytes);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("unsupported"),
            "should reject unsupported version"
        );
    }

    #[test]
    fn save_state_too_short_rejected() {
        let mut emu = make_emulator();
        let result = decode_state(&mut emu, &[0; 4]);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().to_string().contains("too short"),
            "should reject data shorter than header"
        );
    }

    #[test]
    fn save_state_v1_backward_compat() {
        let mut emu = make_emulator();
        for _ in 0..4 {
            emu.step_instruction();
        }
        let pc_before = emu.cpu.pc;

        let mut payload = StateWriter::new();
        payload.write_bytes(&emu.rom_hash);
        emu.cpu.write_state(&mut payload);
        emu.bus.write_state(&mut payload);
        let raw_bytes = payload.into_bytes();

        let mut v1_state = Vec::with_capacity(12 + raw_bytes.len());
        v1_state.extend_from_slice(&NES_SAVE_STATE_MAGIC);
        v1_state.extend_from_slice(&1u32.to_le_bytes()); // version 1
        v1_state.extend_from_slice(&raw_bytes);

        // Reset and restore from V1
        emu.reset();
        decode_state(&mut emu, &v1_state).expect("V1 decode should succeed");
        assert_eq!(emu.cpu.pc, pc_before);
    }
}

