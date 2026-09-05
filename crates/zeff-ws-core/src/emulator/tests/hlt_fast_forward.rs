use super::*;
use zeff_emu_common::address::Address;

fn halted_emulator_with_features(color: bool, rtc: bool, code: &[u8]) -> Emulator {
    let mut rom = rom_with_reset_code(code);
    if color || rtc {
        let footer = rom.len() - 10;
        if color {
            rom[footer + 1] = 1;
        }
        if rtc {
            rom[footer + 7] = 1;
        }
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
    }
    let mut emulator = Emulator::from_rom_data(&rom).unwrap();
    assert_eq!(emulator.step_instruction().unwrap().opcode, 0xEA);
    assert_eq!(emulator.step_instruction().unwrap().opcode, 0xF4);
    assert_eq!(emulator.cpu_state(), CpuState::Halted);
    emulator
}

fn halted_emulator_with_code(color: bool, code: &[u8]) -> Emulator {
    halted_emulator_with_features(color, false, code)
}

fn halted_emulator(color: bool, handler: bool) -> Emulator {
    let code = if handler {
        code_with_handler(&[0xF4], 0x20, &[0xF4])
    } else {
        vec![0xF4]
    };
    halted_emulator_with_code(color, &code)
}

fn restored_pair(source: &Emulator) -> (Emulator, Emulator) {
    let rom = source.cartridge_rom_bytes().to_vec();
    let state = source.encode_state().unwrap();
    let mut fast = Emulator::from_rom_data(&rom).unwrap();
    let mut eager = Emulator::from_rom_data(&rom).unwrap();
    fast.load_state(&state).unwrap();
    eager.load_state(&state).unwrap();
    (fast, eager)
}

fn assert_machine_and_audio_equal(fast: &mut Emulator, eager: &mut Emulator) {
    assert_eq!(fast.cpu_cycles(), eager.cpu_cycles());
    assert_eq!(fast.bus.cycles, eager.bus.cycles);
    assert_eq!(fast.ppu_debug_snapshot(), eager.ppu_debug_snapshot());
    assert_eq!(fast.apu_debug_snapshot(), eager.apu_debug_snapshot());
    assert_eq!(fast.bus.apu.save_state(), eager.bus.apu.save_state());
    assert_eq!(fast.uart_debug_snapshot(), eager.uart_debug_snapshot());
    assert_eq!(
        fast.apu_master_debug_samples_ordered(),
        eager.apu_master_debug_samples_ordered()
    );
    for channel in 0..4 {
        assert_eq!(
            fast.apu_channel_debug_samples_ordered(channel),
            eager.apu_channel_debug_samples_ordered(channel)
        );
    }
    assert_eq!(fast.framebuffer(), eager.framebuffer());
    assert_eq!(fast.encode_state().unwrap(), eager.encode_state().unwrap());
    let mut fast_audio = Vec::new();
    let mut eager_audio = Vec::new();
    fast.drain_audio_samples_into(&mut fast_audio);
    eager.drain_audio_samples_into(&mut eager_audio);
    assert_eq!(fast_audio, eager_audio);
}

fn configure_hypervoice_sound_dma(emulator: &mut Emulator) {
    for offset in 0..256_u32 {
        emulator
            .bus
            .write8(0x1200 + offset, (offset as u8).wrapping_mul(37));
    }
    emulator.io_write8(0x006A, 0x80);
    emulator.io_write8(0x006B, 0x60);
    emulator.bus.io_write16(0x004A, 0x1200);
    emulator.bus.io_write16(0x004C, 0);
    emulator.bus.io_write16(0x004E, 0x0100);
    emulator.bus.io_write16(0x0050, 0);
    emulator.io_write8(0x0052, 0x93);
}

fn configure_timer_interrupt(emulator: &mut Emulator, vblank: bool) {
    let (vector, irq, reload_port, timer_control) = if vblank {
        (5_u16, 0x20, 0x00A6, 0x04)
    } else {
        (7_u16, 0x80, 0x00A4, 0x01)
    };
    emulator.cpu.flags |= 0x0200;
    emulator.bus.write16(u32::from(vector) * 4, 0x0020);
    emulator.bus.write16(u32::from(vector) * 4 + 2, 0xF000);
    emulator.io_write8(0x00B0, 0);
    emulator.io_write8(0x00B2, irq);
    emulator.io_write8(reload_port, 1);
    emulator.io_write8(reload_port + 1, 0);
    emulator.io_write8(0x00A2, timer_control);
}

fn configure_active_apu(emulator: &mut Emulator, phase: u32, seed: u8) {
    for offset in 0..64_u32 {
        emulator
            .bus
            .write8(offset, (offset as u8).wrapping_mul(29).wrapping_add(seed));
    }
    for channel in 0..4_u16 {
        let period = 0x0320_u16 + channel * 0x0137 + u16::from(seed);
        emulator.io_write8(0x0080 + channel * 2, period as u8);
        emulator.io_write8(0x0081 + channel * 2, (period >> 8) as u8);
        emulator.io_write8(0x0088 + channel, 0xF1_u8.wrapping_sub(seed + channel as u8));
    }
    emulator.io_write8(0x008C, seed.wrapping_mul(3).wrapping_add(1));
    emulator.io_write8(0x008D, seed & 0x03);
    emulator.io_write8(0x008E, 0x10 | (seed & 0x07));
    emulator.io_write8(0x0092, seed.wrapping_mul(17));
    emulator.io_write8(0x0093, 0x21_u8.wrapping_add(seed));
    emulator.io_write8(0x0090, 0xCF);
    emulator.io_write8(0x0091, 0x0F);
    emulator.bus.step_cycles(phase);
    emulator.cpu.cycles += u64::from(phase);
}

#[test]
fn hlt_fast_forward_matches_eager_sound_dma_audio_and_continuation() {
    let mut fast = halted_emulator(true, false);
    configure_hypervoice_sound_dma(&mut fast);
    let mut eager = fast.clone();

    for _ in 0..3 {
        fast.step_frame();
        eager.eager_hlt_step_frame();
        assert_machine_and_audio_equal(&mut fast, &mut eager);
    }

    assert!(fast.hlt_fast_forward_calls > 0);
    assert_eq!(eager.hlt_fast_forward_calls, 0);
}

#[test]
fn hlt_fast_forward_matches_hblank_and_vblank_timer_irq_wake() {
    for vblank in [false, true] {
        let mut fast = halted_emulator(false, true);
        configure_timer_interrupt(&mut fast, vblank);
        let initial_sp = fast.cpu.regs[4];
        let mut eager = fast.clone();

        fast.step_frame();
        eager.eager_hlt_step_frame();

        assert_machine_and_audio_equal(&mut fast, &mut eager);
        assert_eq!(fast.cpu.regs[4], initial_sp.wrapping_sub(6));
        assert!(fast.hlt_fast_forward_calls > 0);
    }
}

#[test]
fn hlt_fast_forward_preserves_uart_completion() {
    let mut fast = halted_emulator(false, false);
    fast.io_write8(0x00B3, 0xC0);
    fast.io_write8(0x00B1, 0xA6);
    let completed_cycle = fast.cpu_cycles() + 800;
    let mut eager = fast.clone();

    fast.step_frame();
    eager.eager_hlt_step_frame();
    assert_machine_and_audio_equal(&mut fast, &mut eager);

    let fast_event = fast.take_wonder_swan_link_tx_event().unwrap();
    let eager_event = eager.take_wonder_swan_link_tx_event().unwrap();
    assert_eq!(fast_event, eager_event);
    assert_eq!(fast_event.byte, 0xA6);
    assert_eq!(fast_event.completed_cycle, completed_cycle);
    assert_eq!(fast.encode_state().unwrap(), eager.encode_state().unwrap());
}

#[test]
fn hlt_fast_forward_falls_back_for_every_observer_kind() {
    let base = halted_emulator(false, false);
    let mut cases = Vec::new();

    let mut breakpoint = base.clone();
    breakpoint.add_breakpoint(Address::from(0x12345_u32));
    cases.push(breakpoint);

    let mut watchpoint = base.clone();
    watchpoint.add_watchpoint(Address::from(0x12345_u32), WatchType::Read);
    cases.push(watchpoint);

    let mut event = base.clone();
    event.set_event_breakpoint(DebugEvent::Interrupt, true);
    cases.push(event);

    let mut trace = base.clone();
    trace.set_instruction_trace_enabled(true);
    cases.push(trace);

    let mut opcode = base.clone();
    opcode.set_opcode_log_enabled(true);
    cases.push(opcode);

    let mut break_on_next = base.clone();
    break_on_next.debug.break_on_next = true;
    cases.push(break_on_next);

    let mut bus_trace = base.clone();
    bus_trace.bus.debug_trace_mode = crate::hardware::bus::DebugTraceMode::IoOnly;
    cases.push(bus_trace);

    for mut emulator in cases {
        emulator.step_frame();
        assert_eq!(emulator.hlt_fast_forward_calls, 0);
    }
}

#[test]
fn public_halted_instruction_step_remains_one_cycle() {
    let mut emulator = halted_emulator(false, false);
    let cycles_before = emulator.cpu_cycles();

    assert!(emulator.step_instruction().is_none());

    assert_eq!(emulator.cpu_cycles(), cycles_before + 1);
    assert_eq!(emulator.hlt_fast_forward_calls, 0);
}

#[test]
fn pending_brk_after_shadow_matches_eager_after_state_roundtrip() {
    let mut source = halted_emulator(false, true);
    source.bus.write16(4, 0x0020);
    source.bus.write16(6, 0xF000);
    source.cpu.flags |= 0x0100;
    source.cpu.brk_shadow = 1;
    source.bus.ppu.set_timing_state(158, 254, false);
    let initial_sp = source.cpu.regs[4];
    let (mut fast, mut eager) = restored_pair(&source);
    assert_eq!(fast.cpu_state(), CpuState::Halted);
    assert_ne!(fast.cpu.flags & 0x0100, 0);
    assert_eq!(fast.cpu.brk_shadow, 1);
    assert_eq!(fast.ppu_debug_snapshot().vcount, 158);
    assert_eq!(fast.ppu_debug_snapshot().line_cycles, 254);

    fast.step_frame();
    eager.eager_hlt_step_frame();

    assert_machine_and_audio_equal(&mut fast, &mut eager);
    assert_eq!(fast.cpu.regs[4], initial_sp.wrapping_sub(6));
    assert_eq!(eager.cpu.regs[4], initial_sp.wrapping_sub(6));
    assert_eq!(fast.hlt_fast_forward_calls, 0);
}

#[test]
fn interrupt_shadow_matures_before_hblank_service() {
    let code = code_with_handler(&[0xF4], 0x20, &[0x40, 0xEB, 0xFD]);
    let mut fast = halted_emulator_with_code(false, &code);
    configure_timer_interrupt(&mut fast, false);
    fast.cpu.interrupt_shadow = 1;
    fast.bus.ppu.set_timing_state(10, 255, false);
    assert_eq!(fast.io_peek8(0x00A4), 1);
    assert_eq!(fast.io_peek8(0x00A8), 1);
    let initial_sp = fast.cpu.regs[4];
    let mut eager = fast.clone();

    fast.step_frame();
    eager.eager_hlt_step_frame();

    assert_machine_and_audio_equal(&mut fast, &mut eager);
    assert_eq!(fast.cpu.regs[4], initial_sp.wrapping_sub(6));
    assert_eq!(eager.cpu.regs[4], initial_sp.wrapping_sub(6));
    assert_eq!(fast.hlt_fast_forward_calls, 0);
}

#[test]
fn hblank_wakes_halt_without_if_or_interrupt_service() {
    let mut fast = halted_emulator_with_code(false, &[0xF4, 0x40, 0xEB, 0xFD]);
    configure_timer_interrupt(&mut fast, false);
    fast.cpu.flags &= !0x0200;
    fast.cpu.interrupt_shadow = 0;
    fast.bus.ppu.set_timing_state(10, 255, false);
    assert_eq!(fast.io_peek8(0x00A4), 1);
    assert_eq!(fast.io_peek8(0x00A8), 1);
    let initial_sp = fast.cpu.regs[4];
    let initial_ax = fast.cpu.regs[0];
    let mut eager = fast.clone();

    fast.step_frame();
    eager.eager_hlt_step_frame();

    assert_machine_and_audio_equal(&mut fast, &mut eager);
    assert_eq!(fast.cpu.regs[4], initial_sp);
    assert_ne!(fast.cpu.regs[0], initial_ax);
    assert_eq!(fast.hlt_fast_forward_calls, 0);
}

#[test]
fn final_frame_boundary_does_not_overshoot_uart_completion() {
    let mut fast = halted_emulator(false, false);
    fast.io_write8(0x00B3, 0xC0);
    fast.io_write8(0x00B1, 0xA6);
    fast.bus.step_cycles(798);
    fast.cpu.cycles += 798;
    fast.bus.ppu.set_timing_state(158, 253, false);
    assert_eq!(fast.io_peek8(0x00B2), 0);
    assert_eq!(fast.uart_debug_snapshot().tx_cycles_remaining, 2);
    let initial_cpu_cycles = fast.cpu_cycles();
    let initial_bus_cycles = fast.bus.cycles;
    let completed_cycle = initial_bus_cycles + 2;
    let mut eager = fast.clone();

    fast.step_frame();
    eager.eager_hlt_step_frame();

    assert_machine_and_audio_equal(&mut fast, &mut eager);
    assert_eq!(fast.cpu_cycles(), initial_cpu_cycles + 3);
    assert_eq!(fast.bus.cycles, initial_bus_cycles + 3);
    assert_eq!(fast.ppu_debug_snapshot().vcount, 0);
    assert_eq!(fast.ppu_debug_snapshot().line_cycles, 0);
    assert!(fast.frame_ready());
    assert_eq!(fast.hlt_fast_forward_calls, 1);

    let fast_event = fast.take_wonder_swan_link_tx_event().unwrap();
    let eager_event = eager.take_wonder_swan_link_tx_event().unwrap();
    assert_eq!(fast_event, eager_event);
    assert_eq!(fast_event.byte, 0xA6);
    assert_eq!(fast_event.completed_cycle, completed_cycle);
    assert_eq!(fast.encode_state().unwrap(), eager.encode_state().unwrap());
}

#[test]
fn hlt_fast_forward_matches_eager_mixed_hardware_phase_matrix() {
    let code = code_with_handler(
        &[0xF4, 0x40, 0xEB, 0xFC],
        0x40,
        &[0xB0, 0xFF, 0xE6, 0xB6, 0xCF],
    );
    let cases = [
        (0, 17, 0, 1, 0x98, 767, 3, 1, 2, 1, 20, 31),
        (139, 250, 23, 127, 0x99, 257, 5, 4, 3, 2, 142, 257),
        (143, 129, 4_095, 511, 0x9A, 191, 7, 2, 4, 3, 150, 513),
        (155, 200, 8_191, 798, 0x9B, 73, 11, 10, 5, 4, 157, 700),
    ];

    for (case_index, case) in cases.into_iter().enumerate() {
        let (
            vcount,
            line_cycles,
            apu_phase,
            uart_elapsed,
            dma_control,
            dma_phase,
            hblank_reload,
            hblank_count,
            vblank_reload,
            vblank_count,
            line_compare,
            rtc_cycles_to_rollover,
        ) = case;
        let mut source = halted_emulator_with_features(true, true, &code);
        configure_active_apu(&mut source, apu_phase, case_index as u8 + 1);
        source.io_write8(0x00B3, 0xC0);
        source.io_write8(0x00B1, 0xA0 | case_index as u8);
        source.bus.step_cycles(uart_elapsed);
        source.cpu.cycles += u64::from(uart_elapsed);
        configure_hypervoice_sound_dma(&mut source);
        source.io_write8(0x0052, dma_control);
        let (reload_source, reload_length, _) = source.bus.sound_dma_save_values();
        source
            .bus
            .load_sound_dma_save_values(reload_source, reload_length, dma_phase);
        source.bus.ppu.set_timing_state(vcount, line_cycles, false);
        source
            .bus
            .load_rtc_save_state(crate::hardware::bus::RtcSaveState {
                command: 0x15,
                payload: [0x24, 0x12, 0x31, 0x02, 0x23, 0x59, 0x59],
                payload_index: case_index as u8,
                payload_len: 7,
                ready_delay_reads: (case_index % 3) as u8,
                invalid_command: false,
                subsecond_cycles: crate::hardware::constants::CPU_CLOCK_HZ - rtc_cycles_to_rollover,
            })
            .unwrap();
        source.io_write8(0x00B0, 0);
        for vector in [0_u16, 4, 5, 6, 7] {
            source.bus.write16(u32::from(vector) * 4, 0x0040);
            source.bus.write16(u32::from(vector) * 4 + 2, 0xF000);
        }
        source.bus.io_write16(0x00A4, hblank_reload);
        source.bus.io_write16(0x00A6, vblank_reload);
        source.bus.io_write16(0x00A8, hblank_count);
        source.bus.io_write16(0x00AA, vblank_count);
        source.io_write8(0x00A2, 0x0F);
        source.io_write8(0x0003, line_compare);
        source.io_write8(0x00B2, 0xF0 | u8::from(case_index == 0));
        source.cpu.flags |= 0x0200;
        assert_eq!(source.io_peek8(0x00B4), 0, "case {case_index}");
        let expected_tx_cycle = source.bus.cycles + u64::from(800 - uart_elapsed);
        let (mut fast, mut eager) = restored_pair(&source);
        let mut apu_state = fast.bus.apu.save_state();
        apu_state.sample_cycle_accumulator =
            crate::hardware::constants::CPU_CLOCK_HZ / 4 * (case_index as u32 + 1) - 1;
        fast.bus.apu.load_state(apu_state);
        eager.bus.apu.load_state(apu_state);

        fast.step_frame();
        eager.eager_hlt_step_frame();

        assert_machine_and_audio_equal(&mut fast, &mut eager);
        assert_eq!(
            fast.bus.rtc_save_state().payload,
            [0x25, 0x01, 0x01, 0x03, 0x00, 0x00, 0x00],
            "case {case_index}"
        );
        assert!(fast.hlt_fast_forward_calls > 0, "case {case_index}");
        assert_eq!(eager.hlt_fast_forward_calls, 0, "case {case_index}");
        let fast_tx = fast.take_wonder_swan_link_tx_event().unwrap();
        let eager_tx = eager.take_wonder_swan_link_tx_event().unwrap();
        assert_eq!(fast_tx, eager_tx, "case {case_index}");
        assert_eq!(
            fast_tx.completed_cycle, expected_tx_cycle,
            "case {case_index}"
        );
        assert_eq!(fast.encode_state().unwrap(), eager.encode_state().unwrap());
    }
}
