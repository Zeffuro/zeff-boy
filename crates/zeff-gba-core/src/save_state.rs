use anyhow::{Context, bail, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use crate::emulator::Emulator;
use crate::hardware::apu::ApuSaveState;
use crate::hardware::constants::{
    EWRAM_SIZE, IO_SIZE, IWRAM_SIZE, OAM_SIZE, PALETTE_RAM_SIZE, VRAM_SIZE,
};
use crate::hardware::cpu::{
    CpuBusOperation, CpuExecutionPhase, CpuExecutionState, CpuPipelineEntryState, CpuPipelineState,
    CpuState,
};
use crate::hardware::dma::{DmaChannel, DmaController};
use crate::hardware::timer::{Timer, TimerTimingState, Timers};

const MAGIC: &[u8; 8] = b"ZBGBAST\0";
const VERSION: u32 = 8;
const MAX_BACKUP_SIZE: usize = 0x20_000;
const MAX_FIFO_SIZE: usize = 32;
#[cfg(test)]
const VERSION_7_RUNTIME_STATE_SIZE: usize = 49;
#[cfg(test)]
const VERSION_8_EXECUTION_STATE_SIZE: usize = 71;

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
    let timer_timing = emu.bus.timers.timing_state();
    for accum in timer_timing.cycle_accum {
        w.write_u32(accum);
    }
    for delay in timer_timing.start_delay_cycles {
        w.write_u8(delay);
    }
    w.write_u16(timer_timing.clock_phase);
    w.write_bool(emu.bus.irq_delay_state().is_some());
    w.write_u32(emu.bus.irq_delay_state().unwrap_or_default());
    w.write_u16(emu.cpu.swi_wait_mask);
    let pipeline = cpu.pipeline_state();
    w.write_u8(pipeline.len);
    for entry in pipeline.entries {
        w.write_u32(entry.pc);
        w.write_u32(entry.raw);
        w.write_bool(entry.thumb);
    }
    w.write_bool(pipeline.pending_load_internal_cycle);
    let execution = cpu.execution_state();
    w.write_u8(execution.phase.tag());
    w.write_u8(execution.phase_cycles_remaining);
    w.write_bool(execution.instruction_active);
    w.write_u32(execution.active_pc);
    w.write_u32(execution.active_raw);
    w.write_bool(execution.active_thumb);
    w.write_bool(execution.condition_passed);
    w.write_u32(execution.active_fetch_cycles);
    w.write_u8(execution.bus_operation.tag());
    w.write_u32(execution.bus_address);
    w.write_u8(execution.bus_width);
    w.write_bool(execution.bus_sequential);
    w.write_u32(execution.bus_value);
    w.write_u32(execution.bus_read_latch);
    w.write_u32(execution.transfer_original_base);
    w.write_u32(execution.transfer_current_address);
    w.write_u16(execution.transfer_register_mask);
    w.write_u8(execution.transfer_next_register);
    w.write_bool(execution.transfer_first_access);
    w.write_bool(execution.transfer_force_user);
    w.write_bool(execution.transfer_exception_return);
    w.write_bool(execution.transfer_writeback);
    w.write_bool(execution.writeback_present);
    w.write_u8(execution.writeback_register);
    w.write_u32(execution.writeback_value);
    w.write_u32(execution.refill_target);
    w.write_bool(execution.refill_thumb);
    w.write_u8(execution.refill_index);
    w.write_u32(execution.data_access_elapsed_cycles);
    w.write_u32(execution.data_access_count);
    w.write_u32(execution.data_bus_phase_cycles);

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
    if version >= 7 {
        let mut timer_timing = TimerTimingState::default();
        for accum in &mut timer_timing.cycle_accum {
            *accum = r.read_u32()?;
        }
        for delay in &mut timer_timing.start_delay_cycles {
            *delay = r.read_u8()?;
        }
        timer_timing.clock_phase = r.read_u16()?;
        ensure!(
            emu.bus.timers.set_timing_state(timer_timing),
            "invalid GBA timer timing state"
        );
        let irq_delay = if r.read_bool()? {
            Some(r.read_u32()?)
        } else {
            let _unused = r.read_u32()?;
            None
        };
        ensure!(
            emu.bus.set_irq_delay_state(irq_delay),
            "invalid GBA IRQ delay state"
        );
        emu.cpu.swi_wait_mask = r.read_u16()?;
        ensure!(
            emu.cpu.swi_wait_mask & !0x3FFF == 0
                && (emu.cpu.swi_wait_return_pc.is_some() || emu.cpu.swi_wait_mask == 0),
            "invalid GBA interrupt wait mask"
        );
        let mut pipeline = CpuPipelineState {
            len: r.read_u8()?,
            ..CpuPipelineState::default()
        };
        for entry in &mut pipeline.entries {
            *entry = CpuPipelineEntryState {
                pc: r.read_u32()?,
                raw: r.read_u32()?,
                thumb: r.read_bool()?,
            };
        }
        pipeline.pending_load_internal_cycle = r.read_bool()?;
        ensure!(
            emu.cpu.set_pipeline_state(pipeline),
            "invalid GBA CPU pipeline state"
        );
    } else {
        emu.bus.timers.migrate_legacy_timing(emu.cpu.cycles);
        emu.bus.migrate_legacy_irq_delay();
        emu.cpu.migrate_legacy_pipeline();
        emu.cpu.swi_wait_mask = if emu.cpu.swi_wait_return_pc.is_some() {
            emu.cpu.regs[1] as u16 & 0x3FFF
        } else {
            0
        };
    }

    if version >= 8 {
        let phase_tag = r.read_u8()?;
        let Some(phase) = CpuExecutionPhase::from_tag(phase_tag) else {
            bail!("invalid GBA CPU execution phase {phase_tag}");
        };
        let phase_cycles_remaining = r.read_u8()?;
        let instruction_active = r.read_bool()?;
        let active_pc = r.read_u32()?;
        let active_raw = r.read_u32()?;
        let active_thumb = r.read_bool()?;
        let condition_passed = r.read_bool()?;
        let active_fetch_cycles = r.read_u32()?;
        let bus_operation_tag = r.read_u8()?;
        let Some(bus_operation) = CpuBusOperation::from_tag(bus_operation_tag) else {
            bail!("invalid GBA CPU bus operation {bus_operation_tag}");
        };
        let execution = CpuExecutionState {
            phase,
            phase_cycles_remaining,
            instruction_active,
            active_pc,
            active_raw,
            active_thumb,
            condition_passed,
            active_fetch_cycles,
            bus_operation,
            bus_address: r.read_u32()?,
            bus_width: r.read_u8()?,
            bus_sequential: r.read_bool()?,
            bus_value: r.read_u32()?,
            bus_read_latch: r.read_u32()?,
            transfer_original_base: r.read_u32()?,
            transfer_current_address: r.read_u32()?,
            transfer_register_mask: r.read_u16()?,
            transfer_next_register: r.read_u8()?,
            transfer_first_access: r.read_bool()?,
            transfer_force_user: r.read_bool()?,
            transfer_exception_return: r.read_bool()?,
            transfer_writeback: r.read_bool()?,
            writeback_present: r.read_bool()?,
            writeback_register: r.read_u8()?,
            writeback_value: r.read_u32()?,
            refill_target: r.read_u32()?,
            refill_thumb: r.read_bool()?,
            refill_index: r.read_u8()?,
            data_access_elapsed_cycles: r.read_u32()?,
            data_access_count: r.read_u32()?,
            data_bus_phase_cycles: r.read_u32()?,
        };
        ensure!(
            emu.cpu.set_execution_state(execution),
            "invalid GBA CPU execution state"
        );
    } else {
        emu.cpu.migrate_legacy_execution_state();
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
    use crate::hardware::bus::DebugTraceEvent;

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

    fn seed_host_audio_output(emu: &mut Emulator) {
        emu.bus.apu.seed_host_output_for_state_load_test();
        assert_ne!(emu.apu_debug_snapshot().sample_buffer_len, 0);
        for channel in 0..2 {
            assert!(
                emu.apu_direct_debug_samples_ordered(channel)
                    .iter()
                    .any(|&sample| sample != 0.0)
            );
        }
        for channel in 0..4 {
            assert!(
                emu.apu_psg_channel_debug_samples_ordered(channel)
                    .iter()
                    .any(|&sample| sample != 0.0)
            );
        }
        assert!(
            emu.apu_master_debug_samples_ordered()
                .iter()
                .any(|&sample| sample != 0.0)
        );
        assert!(
            emu.apu_psg_master_debug_samples_ordered()
                .iter()
                .any(|&sample| sample != 0.0)
        );
        let (sample_count, capture_remainder) = emu.bus.apu.psg_host_output_state_for_test();
        assert_ne!(sample_count, 0);
        assert_ne!(capture_remainder, 0);
    }

    fn assert_host_audio_output_cleared(emu: &Emulator) {
        assert_eq!(emu.apu_debug_snapshot().sample_buffer_len, 0);
        for channel in 0..2 {
            assert!(
                emu.apu_direct_debug_samples_ordered(channel)
                    .iter()
                    .all(|&sample| sample == 0.0)
            );
        }
        for channel in 0..4 {
            assert!(
                emu.apu_psg_channel_debug_samples_ordered(channel)
                    .iter()
                    .all(|&sample| sample == 0.0)
            );
        }
        assert!(
            emu.apu_master_debug_samples_ordered()
                .iter()
                .all(|&sample| sample == 0.0)
        );
        assert!(
            emu.apu_psg_master_debug_samples_ordered()
                .iter()
                .all(|&sample| sample == 0.0)
        );
        assert_eq!(emu.bus.apu.psg_host_output_state_for_test(), (0, 0));
    }

    fn assert_host_audio_output_eq(actual: &Emulator, expected: &Emulator) {
        assert_eq!(actual.apu_debug_snapshot(), expected.apu_debug_snapshot());
        for channel in 0..2 {
            assert_eq!(
                actual.apu_direct_debug_samples_ordered(channel),
                expected.apu_direct_debug_samples_ordered(channel)
            );
        }
        for channel in 0..4 {
            assert_eq!(
                actual.apu_psg_channel_debug_samples_ordered(channel),
                expected.apu_psg_channel_debug_samples_ordered(channel)
            );
        }
        assert_eq!(
            actual.apu_master_debug_samples_ordered(),
            expected.apu_master_debug_samples_ordered()
        );
        assert_eq!(
            actual.apu_psg_master_debug_samples_ordered(),
            expected.apu_psg_master_debug_samples_ordered()
        );
        assert_eq!(
            actual.bus.apu.psg_host_output_state_for_test(),
            expected.bus.apu.psg_host_output_state_for_test()
        );
    }

    fn assert_timers_eq(actual: &Timers, expected: &Timers) {
        for (actual, expected) in actual.all().iter().zip(expected.all().iter()) {
            assert_eq!(actual.reload, expected.reload);
            assert_eq!(actual.counter, expected.counter);
            assert_eq!(actual.control, expected.control);
        }
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
    fn public_load_clears_host_audio_output_and_preserves_runtime_policy() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 96_000).unwrap();
        saved.bus.apu.write_fifo_halfword(0, 0x2211);
        saved.bus.apu.write_fifo_halfword(0, 0x4433);
        saved.bus.apu.write_fifo_halfword(1, 0x6655);
        saved.bus.apu.write_fifo_halfword(1, 0x0877);
        saved
            .bus
            .apu
            .on_timer_overflows([1, 0, 0, 0], (1 << 8) | (1 << 12));
        let saved_apu = saved.apu_debug_snapshot();
        assert_eq!(saved_apu.fifo_len, [3, 3]);
        assert_eq!(saved_apu.current_sample, [0x11, 0x55]);
        let state = saved.encode_state().unwrap();

        let mut restored = Emulator::new(&rom, 44_100).unwrap();
        let mutes = [true, false, true, false, true, false];
        restored.set_apu_sample_generation_enabled(false);
        restored.set_apu_channel_mutes(mutes);
        restored.set_apu_debug_capture_enabled(true);
        seed_host_audio_output(&mut restored);

        restored.load_state(&state).unwrap();

        let restored_apu = restored.apu_debug_snapshot();
        assert_eq!(restored_apu.sample_rate, 44_100);
        assert_eq!(restored_apu.psg_sample_rate, 44_100);
        assert!(!restored_apu.sample_generation_enabled);
        assert!(restored_apu.debug_capture_enabled);
        assert_eq!(restored_apu.channel_mutes, mutes);
        assert_eq!(restored_apu.fifo_len, saved_apu.fifo_len);
        assert_eq!(restored_apu.current_sample, saved_apu.current_sample);
        assert_host_audio_output_cleared(&restored);
        let mut audio = Vec::new();
        restored.drain_audio_samples_into(&mut audio);
        assert!(audio.is_empty());
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

        let mut v4 = state
            [..state.len() - 34 - VERSION_7_RUNTIME_STATE_SIZE - VERSION_8_EXECUTION_STATE_SIZE]
            .to_vec();
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

    #[test]
    fn roundtrips_timer_scheduler_phase_and_irq_timing() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        saved.bus.step_cycles(37);
        saved.bus.write16(0x0400_0200, 1 << 3);
        saved.bus.write16(0x0400_0208, 1);
        saved.bus.write16(0x0400_0100, 0xFFFC);
        saved.bus.write16(0x0400_0102, 0x00C1);
        saved.bus.step_cycles(19);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &bytes).unwrap();
        assert_timers_eq(&restored.bus.timers, &saved.bus.timers);
        assert_eq!(
            restored.bus.timers.timing_state(),
            saved.bus.timers.timing_state()
        );
        assert_eq!(restored.bus.irq_delay_state(), saved.bus.irq_delay_state());
        for cycles in [1, 7, 63, 64, 211] {
            saved.bus.step_cycles(cycles);
            restored.bus.step_cycles(cycles);
            assert_timers_eq(&restored.bus.timers, &saved.bus.timers);
            assert_eq!(
                restored.bus.timers.timing_state(),
                saved.bus.timers.timing_state()
            );
            assert_eq!(
                restored.bus.read16(0x0400_0202),
                saved.bus.read16(0x0400_0202)
            );
            assert_eq!(restored.bus.irq_delay_state(), saved.bus.irq_delay_state());
        }
    }

    #[test]
    fn roundtrips_timer_global_divider_phase() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        saved.bus.step_cycles(37);
        saved.bus.write16(0x0400_0100, 0xFFFF);
        saved.bus.write16(0x0400_0102, 0x0081);
        assert_eq!(saved.bus.timers.cycles_until_overflow(0), Some(27));
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &bytes).unwrap();
        assert_eq!(
            restored.bus.timers.timing_state(),
            saved.bus.timers.timing_state()
        );

        for cycles in [26, 1] {
            saved.bus.step_cycles(cycles);
            restored.bus.step_cycles(cycles);
            assert_timers_eq(&restored.bus.timers, &saved.bus.timers);
            assert_eq!(
                restored.bus.timers.timing_state(),
                saved.bus.timers.timing_state()
            );
        }
    }

    #[test]
    fn roundtrips_pending_timer_start_delay() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        saved.bus.step_cycles(16);
        saved.bus.write16(0x0400_0100, 0xFFFF);
        saved.bus.write16(0x0400_0102, 0x0080);
        assert_eq!(saved.bus.timers.timing_state().start_delay_cycles[0], 1);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &bytes).unwrap();
        assert_eq!(
            restored.bus.timers.timing_state(),
            saved.bus.timers.timing_state()
        );

        for cycles in [1, 1] {
            saved.bus.step_cycles(cycles);
            restored.bus.step_cycles(cycles);
            assert_timers_eq(&restored.bus.timers, &saved.bus.timers);
            assert_eq!(
                restored.bus.timers.timing_state(),
                saved.bus.timers.timing_state()
            );
        }
    }

    #[test]
    fn roundtrips_interrupt_wait_mask() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        saved.cpu.swi_wait_return_pc = Some(0x0800_1234);
        saved.cpu.swi_wait_mask = 1 << 3;
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.cpu.swi_wait_return_pc, Some(0x0800_1234));
        assert_eq!(restored.cpu.swi_wait_mask, 1 << 3);
    }

    #[test]
    fn roundtrips_prefetched_pipeline_contents() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        let code = 0x0300_0000;
        saved.bus.write32(code, 0xE3A0_0001);
        saved.bus.write32(code + 4, 0xE3A0_1002);
        saved.bus.write32(code + 8, 0xE3A0_2003);
        saved.cpu.set_pc(code);
        let _ = saved.step_instruction();
        saved.bus.write32(code + 4, 0xE3A0_1009);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
        assert_eq!(restored.cpu.execution_state(), CpuExecutionState::default());
        let _ = saved.step_instruction();
        let _ = restored.step_instruction();
        assert_eq!(saved.cpu.regs, restored.cpu.regs);
        assert_eq!(restored.cpu.regs[1], 2);
        assert_eq!(saved.cpu.cycles, restored.cpu.cycles);
        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
    }

    #[test]
    fn roundtrips_pending_load_internal_cycle() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        let code = 0x0300_0000;
        saved.bus.write16(code, 0x880A);
        saved.bus.write16(code + 2, 0x2307);
        saved.bus.write16(0x0300_0100, 0xABCD);
        saved.cpu.cpsr |= crate::hardware::cpu::CPSR_THUMB;
        saved.cpu.set_pc(code);
        saved.cpu.regs[1] = 0x0300_0100;
        let _ = saved.step_instruction();
        assert!(saved.cpu.pipeline_state().pending_load_internal_cycle);
        let bytes = encode_state(&saved).unwrap();

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &bytes).unwrap();
        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
        let _ = saved.step_instruction();
        let _ = restored.step_instruction();
        assert_eq!(restored.cpu.regs, saved.cpu.regs);
        assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
    }

    #[test]
    fn migrates_version_7_at_an_instruction_boundary() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        let code = 0x0300_0000;
        saved.bus.write32(code, 0xE3A0_0001);
        saved.bus.write32(code + 4, 0xE280_1002);
        saved.bus.write32(code + 8, 0xE281_2003);
        saved.cpu.set_pc(code);
        let _ = saved.step_instruction();
        let state = encode_state(&saved).unwrap();
        let mut v7 = state[..state.len() - VERSION_8_EXECUTION_STATE_SIZE].to_vec();
        v7[8..12].copy_from_slice(&7u32.to_le_bytes());

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &v7).unwrap();

        assert_eq!(restored.cpu.execution_state(), CpuExecutionState::default());
        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
        let _ = saved.step_instruction();
        let _ = restored.step_instruction();
        assert_eq!(restored.cpu.regs, saved.cpu.regs);
        assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
    }

    fn branch_emulator_after_phase_steps(steps: usize) -> Emulator {
        let rom = minimal_rom();
        let mut emu = Emulator::new(&rom, 48_000).unwrap();
        let code = 0x0300_0000;
        emu.bus.write32(code, 0xEA00_0002);
        emu.bus.write32(code + 4, 0xE1A0_0000);
        emu.bus.write32(code + 8, 0xE1A0_0000);
        emu.bus.write32(code + 0x10, 0xE3A0_0007);
        emu.bus.write32(code + 0x14, 0xE1A0_0000);
        emu.bus.write32(code + 0x18, 0xE1A0_0000);
        emu.cpu.set_pc(code);
        for _ in 0..steps {
            let _ = emu.cpu.step_cpu_phase_for_test(&mut emu.bus);
        }
        emu
    }

    fn assert_midphase_continuation(steps: usize, expected_phase: CpuExecutionPhase) {
        let rom = minimal_rom();
        let mut saved = branch_emulator_after_phase_steps(steps);
        assert_eq!(saved.cpu.execution_state().phase, expected_phase);
        let state = encode_state(&saved).unwrap();
        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &state).unwrap();

        assert_eq!(restored.cpu.execution_state(), saved.cpu.execution_state());
        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
        assert_eq!(restored.cpu.regs, saved.cpu.regs);
        assert_eq!(restored.cpu.cpsr, saved.cpu.cpsr);
        assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
        assert_eq!(
            restored.bus.timers.timing_state(),
            saved.bus.timers.timing_state()
        );
        assert_eq!(restored.bus.irq_delay_state(), saved.bus.irq_delay_state());

        let saved_result = saved.step_instruction();
        let restored_result = restored.step_instruction();
        assert_eq!(restored_result, saved_result);
        assert_eq!(restored.cpu.execution_state(), saved.cpu.execution_state());
        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
        assert_eq!(restored.cpu.regs, saved.cpu.regs);
        assert_eq!(restored.cpu.cpsr, saved.cpu.cpsr);
        assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
        assert_eq!(
            restored.bus.timers.timing_state(),
            saved.bus.timers.timing_state()
        );
        assert_eq!(restored.bus.irq_delay_state(), saved.bus.irq_delay_state());
    }

    #[test]
    fn roundtrips_sequential_fetch_and_both_refill_boundaries() {
        assert_midphase_continuation(1, CpuExecutionPhase::SequentialFetch);
        assert_midphase_continuation(2, CpuExecutionPhase::Execute);
        assert_midphase_continuation(3, CpuExecutionPhase::RefillNonSequential);
        assert_midphase_continuation(4, CpuExecutionPhase::RefillSequential);
        assert_midphase_continuation(5, CpuExecutionPhase::Boundary);
    }

    #[test]
    fn roundtrips_both_irq_refill_boundaries() {
        let rom = minimal_rom();
        for completed_phases in [0, 1] {
            let mut saved = Emulator::new(&rom, 48_000).unwrap();
            saved.cpu.cpsr &= !(1 << 7);
            assert!(saved.cpu.try_service_irq(&mut saved.bus, true));
            for _ in 0..completed_phases {
                let _ = saved.cpu.step_cpu_phase_for_test(&mut saved.bus);
            }
            let expected_phase = if completed_phases == 0 {
                CpuExecutionPhase::RefillNonSequential
            } else {
                CpuExecutionPhase::RefillSequential
            };
            assert_eq!(saved.cpu.execution_state().phase, expected_phase);
            let state = encode_state(&saved).unwrap();
            let mut restored = Emulator::new(&rom, 48_000).unwrap();
            decode_state(&mut restored, &state).unwrap();

            assert_eq!(restored.cpu.execution_state(), saved.cpu.execution_state());
            assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
            assert_eq!(restored.cpu.regs, saved.cpu.regs);
            assert_eq!(restored.cpu.cpsr, saved.cpu.cpsr);
            assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
            let saved_result = saved.step_instruction();
            let restored_result = restored.step_instruction();
            assert_eq!(restored_result, saved_result);
            assert_eq!(restored.cpu.execution_state(), saved.cpu.execution_state());
            assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
            assert_eq!(restored.cpu.regs, saved.cpu.regs);
            assert_eq!(restored.cpu.cpsr, saved.cpu.cpsr);
            assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
        }
    }

    fn staged_transfer_emulator(instruction: u32, registers: &[(usize, u32)]) -> Emulator {
        let rom = minimal_rom();
        let mut emu = Emulator::new(&rom, 48_000).unwrap();
        let code = 0x0300_0000;
        emu.bus.write32(code, instruction);
        emu.bus.write32(code + 4, 0xE1A0_0000);
        emu.bus.write32(code + 8, 0xE1A0_0000);
        emu.cpu.set_pc(code);
        for &(register, value) in registers {
            emu.cpu.regs[register] = value;
        }
        emu
    }

    fn assert_staged_transfer_continuation(
        instruction: u32,
        registers: &[(usize, u32)],
        phase_steps: usize,
        expected_phase: CpuExecutionPhase,
    ) {
        let rom = minimal_rom();
        let mut saved = staged_transfer_emulator(instruction, registers);
        saved.bus.write32(0x0200_0000, 0x1122_3344);
        saved.bus.write32(0x0200_0004, 0x5566_7788);
        for _ in 0..phase_steps {
            let _ = saved.cpu.step_cpu_phase_for_test(&mut saved.bus);
        }
        assert_eq!(saved.cpu.execution_state().phase, expected_phase);
        let state = encode_state(&saved).unwrap();
        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &state).unwrap();

        assert_eq!(restored.cpu.execution_state(), saved.cpu.execution_state());
        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
        assert_eq!(restored.cpu.regs, saved.cpu.regs);
        assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
        assert_eq!(restored.bus.ewram, saved.bus.ewram);
        let saved_result = saved.step_instruction();
        let restored_result = restored.step_instruction();
        assert_eq!(restored_result, saved_result);
        assert_eq!(restored.cpu.regs, saved.cpu.regs);
        assert_eq!(restored.cpu.cpsr, saved.cpu.cpsr);
        assert_eq!(restored.cpu.cycles, saved.cpu.cycles);
        assert_eq!(restored.cpu.pipeline_state(), saved.cpu.pipeline_state());
        assert_eq!(restored.bus.ewram, saved.bus.ewram);
        assert_eq!(
            restored.bus.timers.timing_state(),
            saved.bus.timers.timing_state()
        );
        assert_eq!(restored.bus.irq_delay_state(), saved.bus.irq_delay_state());
    }

    #[test]
    fn roundtrips_every_single_transfer_phase() {
        for (steps, phase) in [
            (3, CpuExecutionPhase::DataBus),
            (4, CpuExecutionPhase::LoadInternal),
            (5, CpuExecutionPhase::Writeback),
        ] {
            assert_staged_transfer_continuation(0xE590_1000, &[(0, 0x0200_0000)], steps, phase);
        }
    }

    #[test]
    fn roundtrips_every_two_register_block_transfer_boundary() {
        for (steps, phase) in [
            (3, CpuExecutionPhase::DataBus),
            (4, CpuExecutionPhase::LoadInternal),
            (5, CpuExecutionPhase::DataBus),
            (6, CpuExecutionPhase::LoadInternal),
            (7, CpuExecutionPhase::Writeback),
        ] {
            assert_staged_transfer_continuation(0xE8B2_0005, &[(2, 0x0200_0000)], steps, phase);
        }
    }

    #[test]
    fn completed_block_store_is_not_replayed_after_restore() {
        let rom = minimal_rom();
        let mut saved =
            staged_transfer_emulator(0xE8A2_0005, &[(0, 0x1111_2222), (2, 0x0200_0000)]);
        for _ in 0..4 {
            let _ = saved.cpu.step_cpu_phase_for_test(&mut saved.bus);
        }
        assert_eq!(
            saved.cpu.execution_state().phase,
            CpuExecutionPhase::DataBus
        );
        assert_eq!(saved.cpu.execution_state().bus_address, 0x0200_0004);
        let bytes = encode_state(&saved).unwrap();
        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &bytes).unwrap();

        let (_, events) = restored.step_instruction_with_bus_trace(false, true);
        assert!(events.iter().any(|event| matches!(
            event,
            DebugTraceEvent::Write {
                addr: 0x0200_0004,
                ..
            }
        )));
        assert!(!events.iter().any(|event| matches!(
            event,
            DebugTraceEvent::Write {
                addr: 0x0200_0000,
                ..
            }
        )));
        assert_eq!(restored.bus.read32(0x0200_0000), 0x1111_2222);
        assert_eq!(restored.bus.read32(0x0200_0004), 0x0200_0008);
    }

    #[test]
    fn rejects_invalid_or_noncanonical_execution_state() {
        let rom = minimal_rom();
        let saved = Emulator::new(&rom, 48_000).unwrap();
        let bytes = encode_state(&saved).unwrap();
        let execution_offset = bytes.len() - VERSION_8_EXECUTION_STATE_SIZE;

        let mut invalid_phase = bytes.clone();
        invalid_phase[execution_offset] = 8;
        assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &invalid_phase).is_err());

        let mut active_phase = bytes.clone();
        active_phase[execution_offset] = CpuExecutionPhase::Execute.tag();
        assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &active_phase).is_err());

        let mut orphaned_phase_cycle = bytes.clone();
        orphaned_phase_cycle[execution_offset + 1] = 1;
        assert!(
            decode_state(
                &mut Emulator::new(&rom, 48_000).unwrap(),
                &orphaned_phase_cycle
            )
            .is_err()
        );

        let mut invalid_bus_operation = bytes.clone();
        invalid_bus_operation[execution_offset + 17] = 3;
        assert!(
            decode_state(
                &mut Emulator::new(&rom, 48_000).unwrap(),
                &invalid_bus_operation
            )
            .is_err()
        );

        let mut orphaned_bus_width = bytes;
        orphaned_bus_width[execution_offset + 22] = 4;
        assert!(
            decode_state(
                &mut Emulator::new(&rom, 48_000).unwrap(),
                &orphaned_bus_width
            )
            .is_err()
        );

        let mut invalid_cursor = encode_state(&saved).unwrap();
        invalid_cursor[execution_offset + 59..execution_offset + 63]
            .copy_from_slice(&1025u32.to_le_bytes());
        assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &invalid_cursor).is_err());
    }

    #[test]
    fn rejects_invalid_timer_and_irq_scheduler_state() {
        let rom = minimal_rom();
        let saved = Emulator::new(&rom, 48_000).unwrap();
        let bytes = encode_state(&saved).unwrap();
        let runtime_offset =
            bytes.len() - VERSION_8_EXECUTION_STATE_SIZE - VERSION_7_RUNTIME_STATE_SIZE;

        let mut invalid_accum = bytes.clone();
        invalid_accum[runtime_offset..runtime_offset + 4].copy_from_slice(&0x400u32.to_le_bytes());
        assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &invalid_accum).is_err());

        let mut invalid_start_delay = bytes.clone();
        invalid_start_delay[runtime_offset + 16] = 2;
        assert!(
            decode_state(
                &mut Emulator::new(&rom, 48_000).unwrap(),
                &invalid_start_delay
            )
            .is_err()
        );

        let mut invalid_phase = bytes.clone();
        let phase_offset = runtime_offset + 20;
        invalid_phase[phase_offset..phase_offset + 2].copy_from_slice(&0x400u16.to_le_bytes());
        assert!(decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &invalid_phase).is_err());

        let mut invalid_irq_delay = bytes.clone();
        let irq_present_offset = runtime_offset + 22;
        invalid_irq_delay[irq_present_offset] = 1;
        let irq_delay_offset = runtime_offset + 23;
        invalid_irq_delay[irq_delay_offset..irq_delay_offset + 4]
            .copy_from_slice(&8u32.to_le_bytes());
        assert!(
            decode_state(
                &mut Emulator::new(&rom, 48_000).unwrap(),
                &invalid_irq_delay
            )
            .is_err()
        );

        let mut invalid_wait_mask = encode_state(&saved).unwrap();
        let wait_mask_offset = runtime_offset + 27;
        invalid_wait_mask[wait_mask_offset..wait_mask_offset + 2]
            .copy_from_slice(&0x4000u16.to_le_bytes());
        assert!(
            decode_state(
                &mut Emulator::new(&rom, 48_000).unwrap(),
                &invalid_wait_mask
            )
            .is_err()
        );

        let mut orphaned_wait_mask = encode_state(&saved).unwrap();
        orphaned_wait_mask[wait_mask_offset..wait_mask_offset + 2]
            .copy_from_slice(&8u16.to_le_bytes());
        assert!(
            decode_state(
                &mut Emulator::new(&rom, 48_000).unwrap(),
                &orphaned_wait_mask
            )
            .is_err()
        );

        let mut invalid_pipeline = bytes.clone();
        invalid_pipeline[runtime_offset + 29] = 1;
        invalid_pipeline[runtime_offset + 30..runtime_offset + 34]
            .copy_from_slice(&0x0800_0004u32.to_le_bytes());
        assert!(
            decode_state(&mut Emulator::new(&rom, 48_000).unwrap(), &invalid_pipeline).is_err()
        );

        let mut invalid_pending_load = bytes.clone();
        invalid_pending_load[runtime_offset + 48] = 2;
        assert!(
            decode_state(
                &mut Emulator::new(&rom, 48_000).unwrap(),
                &invalid_pending_load
            )
            .is_err()
        );
    }

    #[test]
    fn migrates_version_6_timer_and_irq_scheduler_state() {
        let rom = minimal_rom();
        let mut saved = Emulator::new(&rom, 48_000).unwrap();
        saved.bus.write16(0x0400_0200, 1 << 3);
        saved.bus.write16(0x0400_0208, 1);
        saved.bus.write16(0x0400_0100, 0xFFFF);
        saved.bus.write16(0x0400_0102, 0x00C1);
        saved.bus.step_cycles(64);
        saved.cpu.cycles = 321;
        let state = encode_state(&saved).unwrap();
        let mut v6 = state
            [..state.len() - VERSION_8_EXECUTION_STATE_SIZE - VERSION_7_RUNTIME_STATE_SIZE]
            .to_vec();
        v6[8..12].copy_from_slice(&6u32.to_le_bytes());

        let mut restored = Emulator::new(&rom, 48_000).unwrap();
        decode_state(&mut restored, &v6).unwrap();

        let timing = restored.bus.timers.timing_state();
        assert_eq!(timing.clock_phase, 321);
        assert_eq!(timing.cycle_accum[0], 1);
        assert_eq!(timing.start_delay_cycles, [0; 4]);
        assert_eq!(restored.bus.irq_delay_state(), Some(7));
    }

    #[test]
    fn public_load_rejects_trailing_data_without_mutation() {
        let mut emu = Emulator::new(&minimal_rom(), 48_000).unwrap();
        emu.set_input(0x03, 0x05);
        emu.step_frame();
        emu.set_apu_sample_generation_enabled(false);
        emu.set_apu_channel_mutes([true, false, true, false, true, false]);
        emu.set_apu_debug_capture_enabled(true);
        seed_host_audio_output(&mut emu);
        let before = emu.encode_state().unwrap();
        let framebuffer = emu.framebuffer().to_vec();
        let mut expected = emu.clone();
        let mut invalid = before.clone();
        invalid.push(0xA5);

        assert!(emu.load_state(&invalid).is_err());
        assert_eq!(emu.encode_state().unwrap(), before);
        assert_eq!(emu.framebuffer(), framebuffer);
        assert_host_audio_output_eq(&emu, &expected);
        let mut actual_audio = Vec::new();
        let mut expected_audio = Vec::new();
        emu.drain_audio_samples_into(&mut actual_audio);
        expected.drain_audio_samples_into(&mut expected_audio);
        assert_eq!(actual_audio, expected_audio);
    }
}
