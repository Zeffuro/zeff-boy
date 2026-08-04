use anyhow::{Context, bail};

use crate::emulator::Emulator;
use crate::hardware::apu::ApuSaveState;
use crate::hardware::cpu::CpuState;

const MAGIC: &[u8; 8] = b"ZEFFWS1\0";
const VERSION: u8 = 8;

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
    w.u8(emu.cpu.interrupt_shadow);
    w.u8(emu.cpu.brk_shadow);

    w.u64(emu.bus.cycles);
    w.vec(&emu.bus.ram)?;
    w.vec(&emu.bus.io)?;
    let (sound_dma_reload_source, sound_dma_reload_length, sound_dma_cycle_accumulator) =
        emu.bus.sound_dma_save_values();
    w.u32(sound_dma_reload_source);
    w.u32(sound_dma_reload_length);
    w.u32(sound_dma_cycle_accumulator);
    let (eeprom_flags, internal_eeprom_done_delay_reads) = emu.bus.eeprom_save_values();
    w.u8(eeprom_flags);
    w.u8(internal_eeprom_done_delay_reads);
    w.vec(&emu.bus.internal_eeprom)?;
    w.u16(emu.bus.cartridge.bank0());
    w.u16(emu.bus.cartridge.bank1());
    w.u8(emu.bus.cartridge.ram_bank());
    w.u8(emu.bus.cartridge.linear_bank());
    w.vec(emu.bus.cartridge.save_data())?;

    let ppu = emu.bus.ppu_debug_snapshot();
    w.u16(ppu.vcount);
    w.u32(ppu.line_cycles);
    w.u8(u8::from(ppu.frame_ready));
    let (sprite_table, sprite_start, sprite_count) = emu.bus.ppu.sprite_cache_state();
    w.bytes(sprite_table);
    w.u8(sprite_start);
    w.u8(sprite_count);
    let apu = emu.bus.apu.save_state();
    for period in apu.period {
        w.u16(period);
    }
    for volume in apu.volume {
        w.u8(volume);
    }
    w.u8(apu.voice_volume);
    w.u8(apu.sweep_step);
    w.u8(apu.sweep_value);
    w.u8(apu.noise_control);
    w.u8(apu.control);
    w.u8(apu.output_control);
    w.u8(apu.sample_ram_pos);
    w.i32(apu.sweep_8192_divider);
    w.u8(apu.sweep_counter);
    for counter in apu.period_counter {
        w.i32(counter);
    }
    for pos in apu.sample_pos {
        w.u8(pos);
    }
    w.u16(apu.nreg);
    w.u8(apu.hyper_voice_sample);
    w.u8(apu.sound_test);
    w.u8(apu.hyper_voice_control);
    w.u8(apu.hyper_voice_channel_control);
    w.u32(apu.sample_cycle_accumulator);
    for muted in apu.channel_mutes {
        w.u8(u8::from(muted));
    }
    Ok(w.finish())
}

pub fn decode_state(emu: &mut Emulator, data: &[u8]) -> anyhow::Result<()> {
    let mut r = StateReader::new(data);
    r.expect_bytes(MAGIC)?;
    let version = r.u8()?;
    if !(2..=VERSION).contains(&version) {
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
    if version >= 8 {
        emu.cpu.interrupt_shadow = r.u8()?;
        emu.cpu.brk_shadow = r.u8()?;
    } else {
        emu.cpu.interrupt_shadow = 0;
        emu.cpu.brk_shadow = 0;
    }
    emu.cpu.last_fetch = None;
    emu.cpu.last_trap = None;

    emu.bus.cycles = r.u64()?;
    r.vec_into(&mut emu.bus.ram)?;
    r.vec_into(&mut emu.bus.io)?;
    if version >= 6 {
        let sound_dma_reload_source = r.u32()?;
        let sound_dma_reload_length = r.u32()?;
        let sound_dma_cycle_accumulator = r.u32()?;
        emu.bus.load_sound_dma_save_values(
            sound_dma_reload_source,
            sound_dma_reload_length,
            sound_dma_cycle_accumulator,
        );
        let eeprom_flags = r.u8()?;
        let internal_eeprom_done_delay_reads = r.u8()?;
        emu.bus
            .load_eeprom_save_values(eeprom_flags, internal_eeprom_done_delay_reads);
    } else {
        emu.bus.load_sound_dma_save_values(0, 0, 0);
        emu.bus.load_eeprom_save_values(0x01, 0);
    }
    if version >= 3 {
        r.vec_into(&mut emu.bus.internal_eeprom)?;
    }
    emu.bus.cartridge.set_bank0(r.u16()? as u8);
    emu.bus.cartridge.set_bank1(r.u16()? as u8);
    emu.bus.cartridge.set_ram_bank(r.u8()?);
    emu.bus.cartridge.set_linear_bank(r.u8()?);
    r.slice_into(emu.bus.cartridge.save_data_mut())?;

    let vcount = r.u16()?;
    let line_cycles = r.u32()?;
    let frame_ready = r.u8()? != 0;
    emu.bus
        .ppu
        .set_timing_state(vcount, line_cycles, frame_ready);
    if version >= 4 {
        let mut sprite_table = [0; 512];
        r.read_exact(&mut sprite_table)?;
        let sprite_start = r.u8()?;
        let sprite_count = r.u8()?;
        emu.bus
            .ppu
            .set_sprite_cache_state(sprite_table, sprite_start, sprite_count);
    } else {
        emu.bus
            .ppu
            .cache_sprites_for_frame(&emu.bus.ram, &emu.bus.io);
    }
    if version >= 5 {
        let mut period = [0; 4];
        for value in &mut period {
            *value = r.u16()?;
        }
        let mut volume = [0; 4];
        for value in &mut volume {
            *value = r.u8()?;
        }
        let voice_volume = r.u8()?;
        let sweep_step = r.u8()?;
        let sweep_value = r.u8()?;
        let noise_control = r.u8()?;
        let control = r.u8()?;
        let output_control = r.u8()?;
        let sample_ram_pos = r.u8()?;
        let sweep_8192_divider = r.i32()?;
        let sweep_counter = r.u8()?;
        let mut period_counter = [0; 4];
        for value in &mut period_counter {
            *value = r.i32()?;
        }
        let mut sample_pos = [0; 4];
        for value in &mut sample_pos {
            *value = r.u8()?;
        }
        let nreg = r.u16()?;
        let hyper_voice_sample = r.u8()?;
        let sound_test = if version >= 7 { r.u8()? } else { 0 };
        let hyper_voice_control = r.u8()?;
        let hyper_voice_channel_control = r.u8()?;
        let sample_cycle_accumulator = r.u32()?;
        let mut channel_mutes = [false; 4];
        for muted in &mut channel_mutes {
            *muted = r.u8()? != 0;
        }
        emu.bus.apu.load_state(ApuSaveState {
            period,
            volume,
            voice_volume,
            sweep_step,
            sweep_value,
            noise_control,
            control,
            output_control,
            sample_ram_pos,
            sweep_8192_divider,
            sweep_counter,
            period_counter,
            sample_pos,
            nreg,
            hyper_voice_sample,
            sound_test,
            hyper_voice_control,
            hyper_voice_channel_control,
            sample_cycle_accumulator,
            channel_mutes,
        });
    } else {
        emu.bus.apu.reset();
    }
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

    fn i32(&mut self, value: i32) {
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

    fn i32(&mut self) -> anyhow::Result<i32> {
        let mut bytes = [0; 4];
        self.read_exact(&mut bytes)?;
        Ok(i32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> anyhow::Result<u64> {
        let mut bytes = [0; 8];
        self.read_exact(&mut bytes)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn vec_into(&mut self, out: &mut [u8]) -> anyhow::Result<()> {
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
        emu.io_write8(0x00BC, 3);
        emu.io_write8(0x00BA, 0x78);
        emu.io_write8(0x00BB, 0x56);
        emu.io_write8(0x00BE, 0x20);
        emu.io_write8(0x0080, 0x34);
        emu.io_write8(0x0081, 0x02);
        emu.io_write8(0x0088, 0xF8);
        emu.io_write8(0x0090, 0x01);
        emu.step_instruction();
        emu.step_instruction();
        emu.step_frame();
        let state = encode_state(&emu).unwrap();

        let mut restored = Emulator::from_rom_data(&rom).unwrap();
        decode_state(&mut restored, &state).unwrap();
        assert_eq!(restored.cpu_registers(), emu.cpu_registers());
        assert_eq!(restored.cpu_peek8(0x1234), 0x56);
        assert_eq!(restored.io_peek8(0x0001), 0x02);
        restored.io_write8(0x00BA, 0);
        restored.io_write8(0x00BB, 0);
        restored.io_write8(0x00BE, 0x10);
        assert_eq!(restored.io_peek8(0x00BA), 0x78);
        assert_eq!(restored.io_peek8(0x00BB), 0x56);
        assert_eq!(restored.ppu_debug_snapshot(), emu.ppu_debug_snapshot());
        assert_eq!(restored.io_peek8(0x0080), 0x34);
        assert_eq!(restored.io_peek8(0x0081), 0x02);
        assert_eq!(restored.io_peek8(0x0088), 0xF8);
        assert_eq!(restored.io_peek8(0x0090), 0x01);
    }
}
