use super::*;
use crate::debug::WatchType;
use crate::hardware::types::ImeState;
use zeff_emu_common::debug::{BusAccessEvent, TraceWriteKind, TraceWriteWidth};
use zeff_emu_common::time::MasterTicks;

fn test_emulator(bytes: &[u8]) -> Emulator {
    let mut rom = vec![0; 0x8000];
    rom[0x100..0x100 + bytes.len()].copy_from_slice(bytes);
    Emulator::new(&rom, 48_000).unwrap()
}

#[test]
fn access_observer_records_fetch_operands_and_ram_bus_order() {
    let mut emulator = test_emulator(&[
        0x3e, 0x4a, // ld a, $4a
        0xea, 0x00, 0xc0, // ld [$c000], a
        0xfa, 0x00, 0xc0, // ld a, [$c000]
    ]);
    let mut accesses = Vec::new();
    for _ in 0..3 {
        emulator.step_instruction_with_accesses(|event| accesses.push(event));
    }

    assert_eq!(emulator.cpu_a(), 0x4a);
    assert_eq!(emulator.cpu_peek8(0xc000), 0x4a);
    assert_eq!(emulator.cpu_cycles(), 40);
    assert!(!emulator.bus.trace_cpu_accesses);
    assert_eq!(
        accesses,
        vec![
            read(4, 0x100, 0x3e),
            read(8, 0x101, 0x4a),
            read(12, 0x102, 0xea),
            read(16, 0x103, 0x00),
            read(20, 0x104, 0xc0),
            write(24, 0xc000, 0, 0x4a),
            read(28, 0x105, 0xfa),
            read(32, 0x106, 0x00),
            read(36, 0x107, 0xc0),
            read(40, 0xc000, 0x4a),
        ]
    );
}

#[test]
fn access_observer_preserves_pcm_cycles_and_debug_traces() {
    let program = [
        0x3e, 0x80, 0xe0, 0x26, // NR52
        0x3e, 0x77, 0xe0, 0x24, // NR50
        0x3e, 0xff, 0xe0, 0x25, // NR51
        0x3e, 0xf0, 0xe0, 0x12, // NR12
        0x3e, 0x80, 0xe0, 0x14, // NR14
        0x3e, 0x5a, 0xea, 0x00, 0xc0, // ld [$c000], a
        0xfa, 0x00, 0xc0, // ld a, [$c000]
    ];
    let mut plain = test_emulator(&program);
    let mut observed = test_emulator(&program);
    observed.set_instruction_trace_enabled(true);
    observed.add_watchpoint(0xc000, WatchType::Read);

    let mut access_count = 0;
    for _ in 0..400 {
        plain.step_instruction();
        observed.step_instruction_with_accesses(|_| access_count += 1);
        if observed.is_cpu_suspended() {
            break;
        }
    }

    assert!(access_count > 0);
    assert_eq!(
        observed.debug_hit_watchpoint().map(|hit| hit.address),
        Some(0xc000)
    );
    assert!(!observed.instruction_trace().is_empty());
    assert_eq!(plain.cpu_cycles(), observed.cpu_cycles());
    assert_eq!(plain.cpu_pc(), observed.cpu_pc());
    assert_eq!(plain.apu_regs_snapshot(), observed.apu_regs_snapshot());
    assert_eq!(plain.drain_audio_samples(), observed.drain_audio_samples());

    let count_after_observed_step = access_count;
    observed.debug_continue();
    observed.set_instruction_trace_enabled(false);
    observed.step_instruction();
    assert_eq!(access_count, count_after_observed_step);
}

#[test]
fn access_observer_records_interrupt_stack_writes() {
    let mut emulator = test_emulator(&[0]);
    emulator.cpu.ime = ImeState::Enabled;
    emulator.bus.ie = 1;
    emulator.bus.if_reg = 1;
    let mut accesses = Vec::new();

    emulator.step_instruction_with_accesses(|event| accesses.push(event));

    assert_eq!(emulator.cpu_pc(), 0x40);
    assert_eq!(
        accesses,
        vec![write(12, 0xfffd, 0, 0x01), write(16, 0xfffc, 0, 0x00)]
    );
}

#[test]
fn access_observer_does_not_fabricate_suspended_or_halted_reads() {
    let mut emulator = test_emulator(&[0x76, 0]);
    let mut accesses = Vec::new();
    emulator.step_instruction_with_accesses(|event| accesses.push(event));
    assert_eq!(accesses, [read(4, 0x100, 0x76)]);
    accesses.clear();
    emulator.step_instruction_with_accesses(|event| accesses.push(event));
    assert!(accesses.is_empty());
    assert!(!emulator.bus.trace_cpu_accesses);
    emulator.debug_suspend();
    let before = emulator.cpu_cycles();
    emulator.step_instruction_with_accesses(|event| accesses.push(event));
    assert!(accesses.is_empty());
    assert_eq!(emulator.cpu_cycles(), before);
}

#[test]
fn access_observer_records_writes_blocked_by_dma_and_oam_lock() {
    let mut dma = test_emulator(&[0]);
    dma.cpu.pc = 0xff80;
    dma.cpu.regs.a = 0x42;
    for (offset, byte) in [0xea, 0x00, 0xc1].into_iter().enumerate() {
        dma.bus.write_byte(0xff80 + offset as u16, byte);
    }
    dma.bus.write_byte(0xff46, 0xc0);
    dma.bus.step_oam_dma(4);
    let mut accesses = Vec::new();
    dma.step_instruction_with_accesses(|event| accesses.push(event));
    assert_eq!(dma.cpu_peek8(0xc100), 0);
    assert_eq!(accesses.last(), Some(&blocked_write(16, 0xc100, 0x42)));

    let mut oam = test_emulator(&[0x3e, 0x42, 0xea, 0x00, 0xfe]);
    oam.cpu.pc = 0x200;
    for _ in 0..20_000 {
        if oam.cpu_peek8(0xff41) & 3 == 3 {
            break;
        }
        oam.step_instruction();
    }
    assert_eq!(oam.cpu_peek8(0xff41) & 3, 3);
    oam.cpu.pc = 0x100;
    let before = oam.cpu_cycles();
    assert!(!oam.bus.cpu_oam_write_accessible());
    oam.step_instruction();
    accesses.clear();
    oam.step_instruction_with_accesses(|event| accesses.push(event));
    assert_eq!(
        accesses.last(),
        Some(&blocked_write(before + 24, 0xfe00, 0x42))
    );
}

fn blocked_write(at: u64, addr: u16, value: u8) -> BusAccessEvent {
    BusAccessEvent::Write {
        at: Some(MasterTicks::new(at)),
        space: TraceWriteKind::Memory,
        addr: u32::from(addr),
        old_value: 0xff,
        written_value: u32::from(value),
        new_value: 0xff,
        width: TraceWriteWidth::Byte,
        mapped_addr: None,
    }
}

fn read(at: u64, addr: u16, value: u8) -> BusAccessEvent {
    BusAccessEvent::Read {
        at: Some(MasterTicks::new(at)),
        space: TraceWriteKind::Memory,
        addr: u32::from(addr),
        value: u32::from(value),
        width: TraceWriteWidth::Byte,
        mapped_addr: None,
    }
}

fn write(at: u64, addr: u16, old_value: u8, value: u8) -> BusAccessEvent {
    BusAccessEvent::Write {
        at: Some(MasterTicks::new(at)),
        space: TraceWriteKind::Memory,
        addr: u32::from(addr),
        old_value: u32::from(old_value),
        written_value: u32::from(value),
        new_value: u32::from(value),
        width: TraceWriteWidth::Byte,
        mapped_addr: None,
    }
}
