use anyhow::{Context, bail};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::emulator::Emulator;
use crate::hardware::constants::{
    EWRAM_SIZE, IO_SIZE, IWRAM_SIZE, OAM_SIZE, PALETTE_RAM_SIZE, VRAM_SIZE,
};
use crate::hardware::cpu::CpuState;
use crate::hardware::dma::{DmaChannel, DmaController};
use crate::hardware::timer::{Timer, Timers};

const MAGIC: &[u8; 8] = b"ZBGBAST\0";
const VERSION: u32 = 1;
const MAX_BACKUP_SIZE: usize = 0x20_000;

pub fn encode_state(emu: &Emulator) -> anyhow::Result<Vec<u8>> {
    let mut w = StateWriter::with_capacity(0x80_000);
    w.write_bytes(MAGIC);
    w.write_u32(VERSION);

    for reg in emu.cpu.regs {
        w.write_u32(reg);
    }
    w.write_u32(emu.cpu.cpsr);
    w.write_u32(emu.cpu.spsr);
    w.write_u64(emu.cpu.cycles);
    w.write_u8(match emu.cpu.state {
        CpuState::Running => 0,
        CpuState::Halted => 1,
        CpuState::Suspended => 2,
    });
    w.write_u32(emu.cpu.last_opcode_pc);
    w.write_bool(emu.cpu.break_after_next_stub);
    w.write_bool(emu.cpu.next_fetch_sequential);
    w.write_u64(emu.frame_count);

    w.write_vec(&emu.bus.ewram);
    w.write_vec(&emu.bus.iwram);
    w.write_vec(&emu.bus.io);
    w.write_vec(&emu.bus.palette_ram);
    w.write_vec(&emu.bus.vram);
    w.write_vec(&emu.bus.oam);

    w.write_u16(emu.bus.keypad.read_keyinput());
    w.write_u16(emu.bus.keypad.read_keycnt());

    for timer in emu.bus.timers.all() {
        w.write_u16(timer.reload);
        w.write_u16(timer.counter);
        w.write_u16(timer.control);
    }

    for ch in emu.bus.dma.channels() {
        w.write_u32(ch.source);
        w.write_u32(ch.destination);
        w.write_u16(ch.count);
        w.write_u16(ch.control);
    }

    w.write_vec(&emu.bus.cartridge.dump_battery_data().unwrap_or_default());
    w.write_u32(emu.bus.apu.sample_rate());
    w.write_bool(emu.bus.apu.sample_generation_enabled());
    for mute in emu.bus.apu.channel_mutes() {
        w.write_bool(mute);
    }

    Ok(w.into_bytes())
}

pub fn decode_state(emu: &mut Emulator, data: &[u8]) -> anyhow::Result<()> {
    let mut r = StateReader::new(data);
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        bail!("not a zeff GBA save state");
    }
    let version = r.read_u32()?;
    if version != VERSION {
        bail!("unsupported GBA save-state version {version}");
    }

    for reg in &mut emu.cpu.regs {
        *reg = r.read_u32()?;
    }
    emu.cpu.cpsr = r.read_u32()?;
    emu.cpu.spsr = r.read_u32()?;
    emu.cpu.cycles = r.read_u64()?;
    emu.cpu.state = match r.read_u8()? {
        0 => CpuState::Running,
        1 => CpuState::Halted,
        2 => CpuState::Suspended,
        other => bail!("invalid GBA CPU state {other}"),
    };
    emu.cpu.last_opcode_pc = r.read_u32()?;
    emu.cpu.break_after_next_stub = r.read_bool()?;
    emu.cpu.next_fetch_sequential = r.read_bool()?;
    emu.frame_count = r.read_u64()?;

    read_fixed_vec(&mut r, &mut emu.bus.ewram, EWRAM_SIZE).context("EWRAM")?;
    read_fixed_vec(&mut r, &mut emu.bus.iwram, IWRAM_SIZE).context("IWRAM")?;
    read_fixed_vec(&mut r, &mut emu.bus.io, IO_SIZE).context("IO")?;
    read_fixed_vec(&mut r, &mut emu.bus.palette_ram, PALETTE_RAM_SIZE).context("palette RAM")?;
    read_fixed_vec(&mut r, &mut emu.bus.vram, VRAM_SIZE).context("VRAM")?;
    read_fixed_vec(&mut r, &mut emu.bus.oam, OAM_SIZE).context("OAM")?;

    let _keyinput = r.read_u16()?;
    let keycnt = r.read_u16()?;
    emu.bus.keypad.write_keycnt(keycnt);

    let mut timers = [Timer::default(); 4];
    for timer in &mut timers {
        timer.reload = r.read_u16()?;
        timer.counter = r.read_u16()?;
        timer.control = r.read_u16()?;
    }
    let mut timer_regs = Timers::default();
    timer_regs.set_all(timers);
    emu.bus.timers = timer_regs;

    let mut channels = [DmaChannel::default(); 4];
    for ch in &mut channels {
        ch.source = r.read_u32()?;
        ch.destination = r.read_u32()?;
        ch.count = r.read_u16()?;
        ch.control = r.read_u16()?;
    }
    let mut dma = DmaController::default();
    dma.set_channels(channels);
    emu.bus.dma = dma;

    let backup = r.read_vec(MAX_BACKUP_SIZE)?;
    if !backup.is_empty() {
        emu.bus.cartridge.load_battery_data(&backup)?;
    }

    emu.bus.apu.set_sample_rate(r.read_u32()?);
    emu.bus.apu.set_sample_generation_enabled(r.read_bool()?);
    let mut mutes = [false; 6];
    for mute in &mut mutes {
        *mute = r.read_bool()?;
    }
    emu.bus.apu.set_channel_mutes(mutes);

    if !r.is_exhausted() {
        bail!("GBA save state has trailing bytes");
    }
    Ok(())
}

fn read_fixed_vec(
    r: &mut StateReader<'_>,
    out: &mut [u8],
    expected_len: usize,
) -> anyhow::Result<()> {
    let data = r.read_vec(expected_len)?;
    if data.len() != expected_len {
        bail!("expected {expected_len} bytes, got {}", data.len());
    }
    out.copy_from_slice(&data);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_rom() -> Vec<u8> {
        let mut rom = vec![0; 0xC0];
        rom[0xA0..0xA4].copy_from_slice(b"TEST");
        rom[0xB2] = 0x96;
        rom
    }

    #[test]
    fn roundtrips_state() {
        let rom = minimal_rom();
        let mut emu = Emulator::new(&rom, 48_000).unwrap();
        emu.cpu_write8(0x0200_0000, 0x55);
        emu.step_frame();
        let bytes = encode_state(&emu).unwrap();

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &bytes).unwrap();
        assert_eq!(restored.cpu_peek8(0x0200_0000), 0x55);
        assert_eq!(restored.frame_count(), 1);
    }
}
