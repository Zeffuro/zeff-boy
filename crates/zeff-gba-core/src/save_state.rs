use anyhow::{Context, bail, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::emulator::Emulator;
use crate::hardware::apu::ApuSaveState;
use crate::hardware::constants::{
    EWRAM_SIZE, IO_SIZE, IWRAM_SIZE, OAM_SIZE, PALETTE_RAM_SIZE, VRAM_SIZE,
};
use crate::hardware::cpu::CpuState;
use crate::hardware::dma::{DmaChannel, DmaController};
use crate::hardware::timer::{Timer, Timers};

const MAGIC: &[u8; 8] = b"ZBGBAST\0";
const VERSION: u32 = 6;
const MAX_BACKUP_SIZE: usize = 0x20_000;
const MAX_FIFO_SIZE: usize = 32;

pub fn encode_state(emu: &Emulator) -> anyhow::Result<Vec<u8>> {
    let mut w = StateWriter::with_capacity(0x80_000);
    w.write_bytes(MAGIC);
    w.write_u32(VERSION);

    let mut cpu = emu.cpu.clone();
    cpu.sync_active_bank();

    for reg in cpu.regs {
        w.write_u32(reg);
    }
    w.write_u32(cpu.cpsr);
    w.write_u32(cpu.spsr);
    w.write_u64(cpu.cycles);
    w.write_u8(match cpu.state {
        CpuState::Running => 0,
        CpuState::Halted => 1,
        CpuState::Suspended => 2,
    });
    w.write_u32(cpu.last_opcode_pc);
    w.write_bool(cpu.break_after_next_stub);
    w.write_bool(cpu.next_fetch_sequential);
    w.write_bool(cpu.swi_wait_return_pc.is_some());
    w.write_u32(cpu.swi_wait_return_pc.unwrap_or_default());
    for value in cpu.banked_sp {
        w.write_u32(value);
    }
    for value in cpu.banked_lr {
        w.write_u32(value);
    }
    for value in cpu.banked_spsr {
        w.write_u32(value);
    }
    for bank in cpu.banked_r8_r12 {
        for value in bank {
            w.write_u32(value);
        }
    }
    w.write_u64(emu.frame_count);
    let (ppu_vcount, ppu_line_cycles, ppu_frame_ready) = emu.bus.ppu.state();
    w.write_u16(ppu_vcount);
    w.write_u32(ppu_line_cycles);
    w.write_bool(ppu_frame_ready);

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
        w.write_u32(ch.active_source);
        w.write_u32(ch.active_destination);
        w.write_u16(ch.active_count);
        w.write_u32(ch.data_latch);
    }

    w.write_vec(&emu.bus.cartridge.dump_battery_data().unwrap_or_default());
    w.write_u32(crate::emulator::DEFAULT_SAMPLE_RATE);
    w.write_bool(true);
    for _ in 0..6 {
        w.write_bool(false);
    }
    let apu_state = emu.bus.apu.save_state();
    w.write_vec(&apu_state.fifo_a);
    w.write_vec(&apu_state.fifo_b);
    w.write_u8(apu_state.current_a as u8);
    w.write_u8(apu_state.current_b as u8);
    w.write_f64(0.0);
    w.write_f64(0.0);
    write_f32(&mut w, 0.0);
    write_f32(&mut w, 0.0);
    w.write_u32(0);
    write_f32(&mut w, 0.0);
    write_f32(&mut w, 0.0);
    write_f32(&mut w, 0.0);
    write_f32(&mut w, 0.0);
    w.write_u32(apu_state.psg_cycle_accum);
    w.write_u64(0);
    w.write_u64(0);
    w.write_u64(0);
    emu.bus.cartridge.write_rtc_state(&mut w);
    w.write_bool(emu.bus.has_external_bios());

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
    if !(2..=VERSION).contains(&version) {
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
    emu.cpu.swi_wait_return_pc = if version >= 4 && r.read_bool()? {
        Some(r.read_u32()?)
    } else {
        if version >= 4 {
            let _unused = r.read_u32()?;
        }
        None
    };
    for value in &mut emu.cpu.banked_sp {
        *value = r.read_u32()?;
    }
    for value in &mut emu.cpu.banked_lr {
        *value = r.read_u32()?;
    }
    for value in &mut emu.cpu.banked_spsr {
        *value = r.read_u32()?;
    }
    for bank in &mut emu.cpu.banked_r8_r12 {
        for value in bank {
            *value = r.read_u32()?;
        }
    }
    emu.frame_count = r.read_u64()?;
    let ppu_vcount = r.read_u16()?;
    let ppu_line_cycles = r.read_u32()?;
    let ppu_frame_ready = r.read_bool()?;
    emu.bus
        .ppu
        .set_state(ppu_vcount, ppu_line_cycles, ppu_frame_ready);

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
        ch.active_source = r.read_u32()?;
        ch.active_destination = r.read_u32()?;
        ch.active_count = r.read_u16()?;
        if version >= 3 {
            ch.data_latch = r.read_u32()?;
        }
    }
    let mut dma = DmaController::default();
    dma.set_channels(channels);
    emu.bus.dma = dma;

    let backup = r.read_vec(MAX_BACKUP_SIZE)?;
    if !backup.is_empty() {
        emu.bus.cartridge.load_battery_data(&backup)?;
    }

    let _saved_sample_rate = r.read_u32()?;
    let _saved_sample_generation_enabled = r.read_bool()?;
    for _ in 0..6 {
        let _saved_mute = r.read_bool()?;
    }
    let apu_state = ApuSaveState {
        fifo_a: r.read_vec(MAX_FIFO_SIZE)?,
        fifo_b: r.read_vec(MAX_FIFO_SIZE)?,
        current_a: r.read_u8()? as i8,
        current_b: r.read_u8()? as i8,
        output_phase: r.read_f64()?,
        dac_phase: r.read_f64()?,
        dac_accum_left: read_f32(&mut r)?,
        dac_accum_right: read_f32(&mut r)?,
        dac_accum_count: r.read_u32()?,
        last_dac_left: read_f32(&mut r)?,
        last_dac_right: read_f32(&mut r)?,
        output_filter_left: read_f32(&mut r)?,
        output_filter_right: read_f32(&mut r)?,
        psg_cycle_accum: r.read_u32()?,
        output_pairs_generated: r.read_u64()?,
        direct_pairs_generated: r.read_u64()?,
        psg_pairs_generated: r.read_u64()?,
    };
    emu.bus.apu.load_save_state(apu_state);

    if version >= 5 {
        emu.bus.cartridge.read_rtc_state(&mut r)?;
    } else {
        emu.bus.cartridge.reset_rtc_state();
    }
    if version >= 6 {
        let state_uses_external_bios = r.read_bool()?;
        ensure!(
            state_uses_external_bios == emu.bus.has_external_bios() || emu.cpu.pc() > 0x3FFF,
            "GBA save state firmware mode does not match the loaded emulator"
        );
    }

    if !r.is_exhausted() {
        bail!("GBA save state has trailing bytes");
    }
    Ok(())
}

fn write_f32(w: &mut StateWriter, value: f32) {
    w.write_u32(value.to_bits());
}

fn read_f32(r: &mut StateReader<'_>) -> anyhow::Result<f32> {
    Ok(f32::from_bits(r.read_u32()?))
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

    fn emerald_rom() -> Vec<u8> {
        let mut rom = minimal_rom();
        rom[0xAC..0xB0].copy_from_slice(b"BPEE");
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

    #[test]
    fn save_state_allows_post_boot_bios_mode_changes() {
        let rom = minimal_rom();
        let bios = vec![0; crate::hardware::constants::BIOS_SIZE];
        let hle_state = encode_state(&Emulator::new(&rom, 48_000).unwrap()).unwrap();
        let mut external = Emulator::new_with_bios(&rom, &bios, 48_000).unwrap();
        external.cpu.set_pc(0x0800_0000);
        let external_state = encode_state(&external).unwrap();

        let mut hle = Emulator::new(&rom, 48_000).unwrap();
        let mut external = Emulator::new_with_bios(&rom, &bios, 48_000).unwrap();
        decode_state(&mut hle, &external_state).unwrap();
        decode_state(&mut external, &hle_state).unwrap();
        decode_state(&mut external, &external_state).unwrap();
    }

    #[test]
    fn save_state_requires_matching_bios_while_executing_in_it() {
        let rom = minimal_rom();
        let bios = vec![0; crate::hardware::constants::BIOS_SIZE];
        let external_state =
            encode_state(&Emulator::new_with_bios(&rom, &bios, 48_000).unwrap()).unwrap();

        let mut hle = Emulator::new(&rom, 48_000).unwrap();
        assert!(decode_state(&mut hle, &external_state).is_err());
    }

    #[test]
    fn roundtrips_fiq_banked_r8_state() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        saved.cpu.regs[8] = 0x1111_2222;
        saved.cpu.set_cpsr(0xC0 | 0x11);
        saved.cpu.regs[8] = 0x3333_4444;
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.cpu.regs[8], 0x3333_4444);
        restored.cpu.set_cpsr(0xC0 | 0x1F);
        assert_eq!(restored.cpu.regs[8], 0x1111_2222);
        restored.cpu.set_cpsr(0xC0 | 0x11);
        assert_eq!(restored.cpu.regs[8], 0x3333_4444);
    }

    #[test]
    fn rejects_unsupported_state_version() {
        let rom = minimal_rom();
        let emu = Emulator::new(&rom, 48_000).unwrap();
        let mut bytes = encode_state(&emu).unwrap();
        bytes[8..12].copy_from_slice(&(VERSION + 1).to_le_bytes());

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        let err = decode_state(&mut restored, &bytes).unwrap_err();

        assert!(err.to_string().contains(&format!(
            "unsupported GBA save-state version {}",
            VERSION + 1
        )));
    }

    #[test]
    fn decode_preserves_runtime_audio_config() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 96_000).unwrap();
        saved.set_apu_sample_generation_enabled(true);
        saved.set_apu_channel_mutes([true, false, true, false, true, false]);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        restored.set_apu_sample_generation_enabled(false);
        restored.set_apu_channel_mutes([false, true, false, true, false, true]);

        decode_state(&mut restored, &bytes).unwrap();

        let apu = restored.apu_debug_snapshot();
        assert_eq!(apu.sample_rate, 48_000);
        assert!(!apu.sample_generation_enabled);
        assert_eq!(apu.channel_mutes, [false, true, false, true, false, true]);
    }

    #[test]
    fn roundtrips_rtc_gpio_state_and_v4_defaults() {
        let rom = emerald_rom();
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        saved.bus.write16(0x0800_00C8, 1);
        saved.bus.write32(0x0800_00C4, 0x0007_0005);
        let state = encode_state(&saved).unwrap();

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &state).unwrap();
        assert_eq!(restored.bus.read16(0x0800_00C8), 1);
        assert_eq!(restored.bus.read16(0x0800_00C6), 7);

        let mut v4 = state[..state.len() - 34].to_vec();
        v4[8..12].copy_from_slice(&4u32.to_le_bytes());
        let mut legacy = Emulator::new(&rom, 48_000).unwrap();
        legacy.set_rtc_date_time(
            crate::hardware::cartridge::RtcDateTime::new(2031, 7, 8, 2, [12, 34, 56]).unwrap(),
        );
        let default_control = legacy.bus.read16(0x0800_00C8);
        decode_state(&mut legacy, &v4).unwrap();
        assert_eq!(legacy.bus.read16(0x0800_00C8), default_control);
        assert_eq!(legacy.rtc_date_time().unwrap().year(), 2000);
    }
}
