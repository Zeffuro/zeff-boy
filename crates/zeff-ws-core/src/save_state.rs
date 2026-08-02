use anyhow::{Context, bail};

use crate::emulator::Emulator;
use crate::hardware::cpu::CpuState;

const MAGIC: &[u8; 8] = b"ZEFFWS1\0";
const VERSION: u8 = 1;

pub fn encode_state(emu: &Emulator) -> anyhow::Result<Vec<u8>> {
    let mut w = StateWriter::new();
    w.bytes(MAGIC);
    w.u8(VERSION);
    w.bytes(&emu.rom_hash);
    w.u64(emu.frame_count);
    w.u32(emu.sample_rate());
    w.u8(u8::from(emu.apu_sample_generation_enabled()));

    for reg in emu.cpu.regs {
        w.u16(reg);
    }
    for segment in emu.cpu.segments {
        w.u16(segment);
    }
    w.u16(emu.cpu.ip);
    w.u16(emu.cpu.flags);
    w.u64(emu.cpu.cycles);
    w.u8(cpu_state_to_byte(emu.cpu.state));
    w.u8(emu.cpu.last_opcode);

    w.u64(emu.bus.cycles);
    w.vec(&emu.bus.ram)?;
    w.vec(&emu.bus.io)?;
    w.u16(emu.bus.cartridge.bank0());
    w.u16(emu.bus.cartridge.bank1());
    w.u8(emu.bus.cartridge.linear_bank());
    w.vec(emu.bus.cartridge.save_data())?;

    let ppu = emu.bus.ppu_debug_snapshot();
    w.u16(ppu.vcount);
    w.u32(ppu.line_cycles);
    w.u8(u8::from(ppu.frame_ready));
    Ok(w.finish())
}

pub fn decode_state(emu: &mut Emulator, data: &[u8]) -> anyhow::Result<()> {
    let mut r = StateReader::new(data);
    r.expect_bytes(MAGIC)?;
    let version = r.u8()?;
    if version != VERSION {
        bail!("unsupported WonderSwan save-state version: {version}");
    }
    let mut rom_hash = [0u8; 32];
    r.read_exact(&mut rom_hash)?;
    if rom_hash != emu.rom_hash {
        bail!("WonderSwan save state belongs to a different ROM");
    }

    emu.frame_count = r.u64()?;
    emu.set_sample_rate(r.u32()?);
    emu.set_apu_sample_generation_enabled(r.u8()? != 0);

    for reg in &mut emu.cpu.regs {
        *reg = r.u16()?;
    }
    for segment in &mut emu.cpu.segments {
        *segment = r.u16()?;
    }
    emu.cpu.ip = r.u16()?;
    emu.cpu.flags = r.u16()?;
    emu.cpu.cycles = r.u64()?;
    emu.cpu.state = byte_to_cpu_state(r.u8()?)?;
    emu.cpu.last_opcode = r.u8()?;
    emu.cpu.last_fetch = None;
    emu.cpu.last_trap = None;

    emu.bus.cycles = r.u64()?;
    r.vec_into(&mut emu.bus.ram)?;
    r.vec_into(&mut emu.bus.io)?;
    emu.bus.cartridge.set_bank0(r.u16()? as u8);
    emu.bus.cartridge.set_bank1(r.u16()? as u8);
    emu.bus.cartridge.set_linear_bank(r.u8()?);
    r.slice_into(emu.bus.cartridge.save_data_mut())?;

    let vcount = r.u16()?;
    let line_cycles = r.u32()?;
    let frame_ready = r.u8()? != 0;
    emu.bus
        .ppu
        .set_timing_state(vcount, line_cycles, frame_ready);
    r.finish()?;
    Ok(())
}

fn cpu_state_to_byte(state: CpuState) -> u8 {
    match state {
        CpuState::Running => 0,
        CpuState::Halted => 1,
        CpuState::Suspended => 2,
    }
}

fn byte_to_cpu_state(value: u8) -> anyhow::Result<CpuState> {
    match value {
        0 => Ok(CpuState::Running),
        1 => Ok(CpuState::Halted),
        2 => Ok(CpuState::Suspended),
        _ => bail!("invalid WonderSwan CPU state in save state: {value}"),
    }
}

struct StateWriter {
    bytes: Vec<u8>,
}

impl StateWriter {
    fn new() -> Self {
        Self { bytes: Vec::new() }
    }

    fn finish(self) -> Vec<u8> {
        self.bytes
    }

    fn bytes(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    fn u8(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn vec(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let len = u32::try_from(bytes.len()).context("WonderSwan save-state section too large")?;
        self.u32(len);
        self.bytes(bytes);
        Ok(())
    }
}

struct StateReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> StateReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn finish(&self) -> anyhow::Result<()> {
        if self.pos != self.data.len() {
            bail!(
                "WonderSwan save state has {} trailing byte(s)",
                self.data.len() - self.pos
            );
        }
        Ok(())
    }

    fn expect_bytes(&mut self, expected: &[u8]) -> anyhow::Result<()> {
        let bytes = self.read_slice(expected.len())?;
        if bytes != expected {
            bail!("invalid WonderSwan save-state magic");
        }
        Ok(())
    }

    fn read_exact(&mut self, out: &mut [u8]) -> anyhow::Result<()> {
        let bytes = self.read_slice(out.len())?;
        out.copy_from_slice(bytes);
        Ok(())
    }

    fn read_slice(&mut self, len: usize) -> anyhow::Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(len)
            .context("WonderSwan save-state cursor overflow")?;
        let bytes = self
            .data
            .get(self.pos..end)
            .context("WonderSwan save state is truncated")?;
        self.pos = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> anyhow::Result<u8> {
        Ok(self.read_slice(1)?[0])
    }

    fn u16(&mut self) -> anyhow::Result<u16> {
        let mut bytes = [0; 2];
        self.read_exact(&mut bytes)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn u32(&mut self) -> anyhow::Result<u32> {
        let mut bytes = [0; 4];
        self.read_exact(&mut bytes)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> anyhow::Result<u64> {
        let mut bytes = [0; 8];
        self.read_exact(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn vec_into(&mut self, out: &mut Vec<u8>) -> anyhow::Result<()> {
        let len = self.u32()? as usize;
        if len != out.len() {
            bail!(
                "WonderSwan save-state section size mismatch: got {len}, expected {}",
                out.len()
            );
        }
        let bytes = self.read_slice(len)?;
        out.copy_from_slice(bytes);
        Ok(())
    }

    fn slice_into(&mut self, out: &mut [u8]) -> anyhow::Result<()> {
        let len = self.u32()? as usize;
        if len != out.len() {
            bail!(
                "WonderSwan save-state section size mismatch: got {len}, expected {}",
                out.len()
            );
        }
        let bytes = self.read_slice(len)?;
        out.copy_from_slice(bytes);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::cartridge::compute_footer_checksum;

    fn rom_with_reset_code(code: &[u8]) -> Vec<u8> {
        let mut rom = vec![0xFF; 0x10000];
        rom[..code.len()].copy_from_slice(code);
        let reset = rom.len() - 16;
        rom[reset..reset + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
        let footer = rom.len() - 10;
        rom[footer + 4] = 0x01;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        rom
    }

    #[test]
    fn restores_cpu_ram_io_and_ppu_state() {
        let rom = rom_with_reset_code(&[0xB8, 0x34, 0x12, 0xF4]);
        let mut emu = Emulator::from_rom_data(&rom).unwrap();
        emu.cpu_write8(0x1234, 0x56);
        emu.io_write8(0x0001, 0x02);
        emu.step_instruction();
        emu.step_instruction();
        emu.step_frame();
        let state = encode_state(&emu).unwrap();

        let mut restored = Emulator::from_rom_data(&rom).unwrap();
        decode_state(&mut restored, &state).unwrap();
        assert_eq!(restored.cpu_registers(), emu.cpu_registers());
        assert_eq!(restored.cpu_peek8(0x1234), 0x56);
        assert_eq!(restored.io_peek8(0x0001), 0x02);
        assert_eq!(restored.ppu_debug_snapshot(), emu.ppu_debug_snapshot());
    }
}
