use super::{DEFAULT_SAMPLE_RATE, Emulator};
use crate::hardware::bus::DebugTraceEvent;
use crate::hardware::constants::{APU_STATUS, FRAME_STEP_4};
use crate::hardware::cpu::StatusFlags;
use zeff_emu_common::save_ram::SaveRamKind;

fn build_test_rom_with_program(program: &[u8]) -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let prg = 16;
    rom[prg..prg + program.len()].copy_from_slice(program);
    rom[prg + 0x3FFC] = 0x00;
    rom[prg + 0x3FFD] = 0x80;
    rom
}

fn build_test_rom() -> Vec<u8> {
    build_test_rom_with_program(&[0xEA])
}

fn build_vs_system_test_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 0x8000 + 0x4000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 2;
    rom[5] = 2;
    rom[6] = 0x30;
    rom[7] = 0x60;
    let prg = 16;
    rom[prg] = 0xEA;
    rom[prg + 0x7FFC] = 0x00;
    rom[prg + 0x7FFD] = 0x80;
    rom
}

#[test]
fn new_uses_power_on_reset_without_stack_adjust() {
    let emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

    assert_eq!(emu.cpu.pc, 0x8000);
    assert_eq!(emu.cpu.sp, 0xFD);
    assert_eq!(emu.cpu.regs.a, 0);
    assert_eq!(emu.cpu.regs.x, 0);
    assert_eq!(emu.cpu.regs.y, 0);
    assert_eq!(emu.cpu.regs.p.bits(), 0x24);
}

#[test]
fn public_api_parity_wrappers_load_step_and_roundtrip_state() {
    let rom = build_test_rom_with_program(&[0x4C, 0x00, 0x80]);
    let mut emu = Emulator::from_rom_data(&rom).expect("test ROM");

    assert_eq!(emu.framebuffer_dimensions(), (256, 240));
    assert_eq!(emu.framebuffer().len(), 256 * 240 * 4);
    assert_eq!(emu.frame_count(), 0);
    assert_eq!(emu.save_ram_kind(), SaveRamKind::none());
    assert_eq!(emu.system_ram().len(), 0x800);
    assert_eq!(emu.video_ram_snapshot().len(), 0x2000);
    assert!(emu.iter_breakpoints().next().is_none());

    emu.add_breakpoint(emu.cpu_pc());
    assert_eq!(
        emu.iter_breakpoints().collect::<Vec<_>>(),
        vec![emu.cpu_pc()]
    );
    assert_eq!(emu.debug_hit_breakpoint(), None);
    emu.remove_breakpoint(emu.cpu_pc());

    emu.add_watchpoint(0x0000, crate::debug::WatchType::Write);
    assert_eq!(emu.debug_watchpoints().len(), 1);
    emu.cpu_write8(0x0000, 0x5A);
    assert_eq!(emu.cpu_peek8(0x0000), 0x5A);
    assert_eq!(
        emu.debug_hit_watchpoint().map(|hit| hit.new_value),
        Some(0x5A)
    );
    emu.debug_continue();

    emu.set_input(0x01, 0x01);
    emu.step_frame();

    assert!(emu.frame_count() > 0);

    let mut audio = Vec::new();
    emu.drain_audio_samples_into(&mut audio);

    let state = emu
        .encode_state()
        .expect("NES emulator should encode state");
    emu.load_state(&state)
        .expect("NES emulator should load state");
}

#[test]
fn reset_preserves_cpu_registers_and_decrements_stack() {
    let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");
    emu.cpu.regs.a = 0x34;
    emu.cpu.regs.x = 0x56;
    emu.cpu.regs.y = 0x78;
    emu.cpu.regs.p = StatusFlags::from_bits_truncate(0xFB);
    emu.cpu.sp = 0x12;
    emu.bus.ram[0x110] = 0xBC;
    emu.bus.ram[0x111] = 0x9A;
    emu.bus.ram[0x112] = 0xFB;

    emu.reset();

    assert_eq!(emu.cpu.pc, 0x8000);
    assert_eq!(emu.cpu.regs.a, 0x34);
    assert_eq!(emu.cpu.regs.x, 0x56);
    assert_eq!(emu.cpu.regs.y, 0x78);
    assert_eq!(emu.cpu.regs.p.bits(), 0xFF);
    assert_eq!(emu.cpu.sp, 0x0F);
    assert_eq!(emu.bus.ram[0x110], 0xBC);
    assert_eq!(emu.bus.ram[0x111], 0x9A);
    assert_eq!(emu.bus.ram[0x112], 0xFB);
}

#[test]
fn mapper_99_zapper_uses_vs_serial_protocol_on_4016() {
    let mut emu =
        Emulator::new(&build_vs_system_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");
    emu.set_zapper_state(true, true, true, None);

    emu.cpu_write(0x4016, 1);
    emu.cpu_write(0x4016, 0);

    let port_1_bits: Vec<u8> = (0..8)
        .map(|_| emu.bus_mut().cpu_read(0x4016) & 0x01)
        .collect();

    assert_eq!(port_1_bits, [0, 0, 0, 0, 1, 0, 1, 1]);
}

#[test]
fn mapper_99_select_exposes_one_vs_coin_pulse() {
    let mut emu =
        Emulator::new(&build_vs_system_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

    emu.set_input_p1(0x04);
    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x24, 0x20);

    emu.set_input_p1(0);
    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x04, 0);
    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x20, 0x20);

    for _ in 0..4 {
        emu.bus.finish_vs_system_input_frame();
    }
    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x20, 0);
}

#[test]
fn nrom_select_does_not_expose_vs_credit_bits() {
    let mut emu = Emulator::new(&build_test_rom(), DEFAULT_SAMPLE_RATE).expect("test ROM");

    emu.set_input_p1(0x04);

    assert_eq!(emu.bus_mut().cpu_read(0x4016) & 0x24, 0);
}

#[test]
fn indexed_store_dummy_read_can_ack_frame_irq_edge() {
    let rom = build_test_rom_with_program(&[
        0xA2, 0x15, // LDX #$15
        0xA9, 0x00, // LDA #$00
        0x9D, 0x00, 0x40, // STA $4000,X
        0xEA, // NOP
    ]);
    let mut emu = Emulator::new(&rom, DEFAULT_SAMPLE_RATE).expect("test ROM");

    emu.step_instruction();
    emu.step_instruction();

    emu.bus.apu.five_step_mode = false;
    emu.bus.apu.irq_inhibit = false;
    emu.bus.apu.frame_irq = false;
    emu.bus.apu.frame_cycle = FRAME_STEP_4 - 3;
    emu.bus.apu.frame_reset_delay = 0;

    let (_, _, _, events) = emu.step_instruction_with_bus_trace();

    let status_read = events.iter().find_map(|event| match event {
        DebugTraceEvent::Read { addr, value, .. } if *addr == APU_STATUS => Some(*value),
        _ => None,
    });

    assert_eq!(status_read.map(|value| value & 0x40), Some(0x40));
    assert!(!emu.bus.apu.irq_pending());
    assert!(!emu.cpu.irq_line);
}
