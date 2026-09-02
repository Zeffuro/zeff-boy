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

mod tas_state;

pub use tas_state::{
    CurrentNativeGbaTasStateInspection, CurrentNativeGbaTasStateProjection, GbaTasKeypadState,
    GbaTasStartup, TAS_DETERMINISM_ABI_ID, TAS_STATE_FORMAT_COMPATIBILITY_ID,
    inspect_current_native_gba_tas_state, restore_current_native_gba_tas_state,
};

const MAGIC: &[u8; 8] = b"ZBGBAST\0";
const VERSION: u32 = 10;
const MAX_BACKUP_SIZE: usize = 0x20_000;
const MAX_FIFO_SIZE: usize = 32;
#[cfg(test)]
const VERSION_7_RUNTIME_STATE_SIZE: usize = 49;
#[cfg(test)]
const VERSION_8_EXECUTION_STATE_SIZE: usize = 71;
#[cfg(test)]
const VERSION_9_ROM_HASH_SIZE: usize = 32;
#[cfg(test)]
const VERSION_10_BACKUP_EXECUTION_STATE_SIZE: usize =
    crate::hardware::cartridge::BACKUP_EXECUTION_STATE_SIZE;

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
    emu.bus.cartridge.write_backup_execution_state(&mut w);
    w.write_bytes(&emu.rom_hash);

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

    let keyinput = r.read_u16()?;
    let keycnt = r.read_u16()?;
    let pressed = !keyinput & 0x03FF;
    emu.bus.keypad.set_host_input(
        ((pressed & 0x000F) as u8) | (((pressed >> 8) as u8 & 0x03) << 4),
        (pressed >> 4) as u8 & 0x0F,
    );
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

    if version >= 10 {
        emu.bus.cartridge.read_backup_execution_state(&mut r)?;
    } else {
        emu.bus.cartridge.reset_backup_execution_state();
    }
    if version >= 9 {
        let mut state_rom_hash = [0; 32];
        r.read_exact(&mut state_rom_hash)?;
        ensure!(
            state_rom_hash == emu.rom_hash,
            "GBA save state ROM identity does not match the loaded emulator"
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
mod backup_state_tests;
#[cfg(test)]
mod tests;
