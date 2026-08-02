use super::flow::brk;
use crate::hardware::bus::Bus;
use crate::hardware::cartridge::Cartridge;
use crate::hardware::cpu::Cpu;
use crate::hardware::cpu::registers::StatusFlags;

fn build_bus_with_program(program: &[u8]) -> Bus {
    let mut rom = vec![0u8; 16 + 0x4000 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;

    let prg_start = 16;
    rom[prg_start..prg_start + program.len()].copy_from_slice(program);

    rom[prg_start + 0x3FFC] = 0x00;
    rom[prg_start + 0x3FFD] = 0x80;

    let cart = Cartridge::load(&rom).expect("test ROM should load");
    Bus::new(cart, 44_100.0)
}

fn setup(program: &[u8]) -> (Cpu, Bus) {
    let mut bus = build_bus_with_program(program);
    let mut cpu = Cpu::new();
    cpu.reset(&mut bus);
    (cpu, bus)
}

#[test]
fn unofficial_nop_immediate_consumes_operand_byte() {
    let mut bus = build_bus_with_program(&[0x80, 0x02, 0xEA]);
    let mut cpu = Cpu::new();
    cpu.reset(&mut bus);

    let cycles = cpu.step(&mut bus);
    assert_eq!(cycles, 2);
    assert_eq!(cpu.last_opcode, 0x80);
    assert_eq!(cpu.pc, 0x8002);

    cpu.step(&mut bus);
    assert_eq!(cpu.last_opcode, 0xEA);
}

#[test]
fn adc_imm_basic() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x10, 0x69, 0x20]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x30);
    assert!(!cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(!cpu.regs.get_flag(StatusFlags::OVERFLOW));
    assert!(!cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(!cpu.regs.get_flag(StatusFlags::NEGATIVE));
}

#[test]
fn adc_carry_out() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0xFF, 0x69, 0x01]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x00);
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(cpu.regs.get_flag(StatusFlags::ZERO));
}

#[test]
fn adc_carry_in() {
    let (mut cpu, mut bus) = setup(&[0x38, 0xA9, 0x10, 0x69, 0x20]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x31);
}

#[test]
fn adc_overflow_positive() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x50, 0x69, 0x50]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0xA0);
    assert!(cpu.regs.get_flag(StatusFlags::OVERFLOW));
    assert!(cpu.regs.get_flag(StatusFlags::NEGATIVE));
}

#[test]
fn adc_overflow_negative() {
    let (mut cpu, mut bus) = setup(&[0x18, 0xA9, 0xD0, 0x69, 0x90]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x60);
    assert!(cpu.regs.get_flag(StatusFlags::OVERFLOW));
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
}

#[test]
fn sbc_basic() {
    let (mut cpu, mut bus) = setup(&[0x38, 0xA9, 0x30, 0xE9, 0x10]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x20);
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(!cpu.regs.get_flag(StatusFlags::OVERFLOW));
}

#[test]
fn sbc_borrow() {
    let (mut cpu, mut bus) = setup(&[0x18, 0xA9, 0x10, 0xE9, 0x20]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0xEF);
    assert!(!cpu.regs.get_flag(StatusFlags::CARRY));
}

#[test]
fn sbc_overflow() {
    let (mut cpu, mut bus) = setup(&[0x38, 0xA9, 0x50, 0xE9, 0xB0]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0xA0);
    assert!(cpu.regs.get_flag(StatusFlags::OVERFLOW));
}

#[test]
fn cmp_equal() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x42, 0xC9, 0x42]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(!cpu.regs.get_flag(StatusFlags::NEGATIVE));
}

#[test]
fn cmp_greater() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x42, 0xC9, 0x20]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(!cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
}

#[test]
fn cmp_less() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x20, 0xC9, 0x42]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(!cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(!cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(cpu.regs.get_flag(StatusFlags::NEGATIVE));
}

#[test]
fn cpx_basic() {
    let (mut cpu, mut bus) = setup(&[0xA2, 0x10, 0xE0, 0x10]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
}

#[test]
fn cpy_basic() {
    let (mut cpu, mut bus) = setup(&[0xA0, 0xFF, 0xC0, 0x00]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(!cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(cpu.regs.get_flag(StatusFlags::NEGATIVE));
}

#[test]
fn branch_not_taken_2_cycles() {
    let (mut cpu, mut bus) = setup(&[0x38, 0x90, 0x02, 0xEA]);
    cpu.step(&mut bus);
    let cycles = cpu.step(&mut bus);
    assert_eq!(cycles, 2);
}

#[test]
fn branch_taken_same_page_3_cycles() {
    let (mut cpu, mut bus) = setup(&[0x18, 0x90, 0x02, 0xEA, 0xEA]);
    cpu.step(&mut bus);
    let cycles = cpu.step(&mut bus);
    assert_eq!(cycles, 3);
    assert!(
        cpu.last_step_branch_taken_same_page,
        "taken same-page branches have a distinct interrupt poll point"
    );
}

#[test]
fn lda_abs_x_no_page_cross() {
    let (mut cpu, mut bus) = setup(&[0xA2, 0x01, 0xBD, 0x10, 0x80]);
    cpu.step(&mut bus);
    let cycles = cpu.step(&mut bus);
    assert_eq!(cycles, 4);
}

#[test]
fn lda_abs_x_page_cross() {
    let (mut cpu, mut bus) = setup(&[0xA2, 0xFF, 0xBD, 0xFF, 0x80]);
    cpu.step(&mut bus);
    let cycles = cpu.step(&mut bus);
    assert_eq!(cycles, 5);
}

#[test]
fn las_abs_y_loads_a_x_sp_and_page_crosses() {
    let (mut cpu, mut bus) = setup(&[0xBB, 0xF0, 0x00]);
    cpu.regs.y = 0x10;
    cpu.sp = 0x3C;
    bus.ram[0x0100] = 0xF3;

    let cycles = cpu.step(&mut bus);

    assert_eq!(cycles, 5);
    assert_eq!(cpu.regs.a, 0x30);
    assert_eq!(cpu.regs.x, 0x30);
    assert_eq!(cpu.sp, 0x30);
    assert!(!cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(!cpu.regs.get_flag(StatusFlags::NEGATIVE));
}

#[test]
fn jmp_indirect_page_boundary_bug() {
    let mut program = vec![0u8; 0x4000];
    program[0] = 0x6C;
    program[1] = 0xFF;
    program[2] = 0x80;
    program[0xFF] = 0x10;
    program[0] = 0xEA;
    program[3] = 0x6C;
    program[4] = 0xFF;
    program[5] = 0x80;
    program[0xFF] = 0x20;
    program[0x3FFC] = 0x03;
    program[0x3FFD] = 0x80;

    let mut rom = vec![0u8; 16 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let mut full_rom = Vec::new();
    full_rom.extend_from_slice(&rom[..16]);
    full_rom.extend_from_slice(&program);
    full_rom.extend_from_slice(&rom[16..16 + 0x2000]);

    let cart = Cartridge::load(&full_rom).expect("test ROM");
    let mut bus = Bus::new(cart, 44_100.0);
    let mut cpu = Cpu::new();
    cpu.reset(&mut bus);

    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0xEA20);
}

#[test]
fn zero_page_x_wraps() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x42, 0x85, 0x03, 0xA2, 0x04, 0xB5, 0xFF]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x42);
}

#[test]
fn brk_normal_uses_irq_vector() {
    let mut program = vec![0xEA; 0x4000];
    program[0] = 0x00;
    program[0x3FFE] = 0x00;
    program[0x3FFF] = 0x81;
    program[0x3FFA] = 0x00;
    program[0x3FFB] = 0x82;
    program[0x3FFC] = 0x00;
    program[0x3FFD] = 0x80;

    let mut rom = vec![0u8; 16 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let mut full = Vec::new();
    full.extend_from_slice(&rom[..16]);
    full.extend_from_slice(&program);
    full.extend_from_slice(&rom[16..16 + 0x2000]);

    let cart = Cartridge::load(&full).expect("test ROM");
    let mut bus = Bus::new(cart, 44_100.0);
    let mut cpu = Cpu::new();
    cpu.reset(&mut bus);

    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x8100, "BRK without NMI should use IRQ vector");
}

#[test]
fn brk_nmi_hijack_via_step_services_nmi_first() {
    let mut program = vec![0xEA; 0x4000];
    program[0] = 0x00;
    program[0x3FFE] = 0x00;
    program[0x3FFF] = 0x81;
    program[0x3FFA] = 0x00;
    program[0x3FFB] = 0x82;
    program[0x3FFC] = 0x00;
    program[0x3FFD] = 0x80;

    let mut rom = vec![0u8; 16 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let mut full = Vec::new();
    full.extend_from_slice(&rom[..16]);
    full.extend_from_slice(&program);
    full.extend_from_slice(&rom[16..16 + 0x2000]);

    let cart = Cartridge::load(&full).expect("test ROM");
    let mut bus = Bus::new(cart, 44_100.0);
    let mut cpu = Cpu::new();
    cpu.reset(&mut bus);

    cpu.nmi_pending = true;
    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x8200, "NMI should be serviced before BRK executes");
    assert!(!cpu.nmi_pending, "NMI should be consumed");
}

#[test]
fn brk_hijack_via_direct_call() {
    let mut program = vec![0xEA; 0x4000];
    program[0x3FFE] = 0x00;
    program[0x3FFF] = 0x81;
    program[0x3FFA] = 0x00;
    program[0x3FFB] = 0x82;
    program[0x3FFC] = 0x00;
    program[0x3FFD] = 0x80;

    let mut rom = vec![0u8; 16 + 0x2000];
    rom[0..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1;
    rom[5] = 1;
    let mut full = Vec::new();
    full.extend_from_slice(&rom[..16]);
    full.extend_from_slice(&program);
    full.extend_from_slice(&rom[16..16 + 0x2000]);

    let cart = Cartridge::load(&full).expect("test ROM");
    let mut bus = Bus::new(cart, 44_100.0);
    let mut cpu = Cpu::new();
    cpu.reset(&mut bus);
    cpu.nmi_pending = true;
    brk(&mut cpu, &mut bus);

    assert_eq!(
        cpu.pc, 0x8200,
        "BRK with NMI pending should hijack to NMI vector"
    );
    assert!(!cpu.nmi_pending, "NMI should be consumed");
}

#[test]
fn cli_delays_pending_irq_for_one_instruction() {
    let (mut cpu, mut bus) = setup(&[0x58, 0xEA, 0xEA]);
    cpu.irq_line = true;

    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x8001);
    assert!(!cpu.regs.get_flag(StatusFlags::INTERRUPT));

    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x8002, "instruction after CLI should run");

    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x0000, "pending IRQ should be serviced next");
}

#[test]
fn sei_delay_allows_irq_after_cli_sei_pair() {
    let (mut cpu, mut bus) = setup(&[0x58, 0x78, 0xEA]);
    cpu.irq_line = true;

    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x8001);

    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x8002);
    assert!(cpu.regs.get_flag(StatusFlags::INTERRUPT));

    cpu.step(&mut bus);
    assert_eq!(
        cpu.pc, 0x0000,
        "SEI inhibition is delayed by one instruction"
    );
}

#[test]
fn delayed_irq_poll_runs_one_more_instruction() {
    let (mut cpu, mut bus) = setup(&[0xEA, 0xEA, 0xEA]);
    cpu.regs.set_flag(StatusFlags::INTERRUPT, false);
    cpu.irq_line = true;
    cpu.delay_irq_poll_once();

    cpu.step(&mut bus);
    assert_eq!(
        cpu.pc, 0x8001,
        "poll delay should let one instruction execute"
    );

    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x0000, "IRQ should be serviced after the delay");
}

#[test]
fn delayed_nmi_poll_runs_one_more_instruction() {
    let (mut cpu, mut bus) = setup(&[0xEA, 0xEA, 0xEA]);
    cpu.nmi_pending = true;
    cpu.delay_nmi_poll_once();

    cpu.step(&mut bus);
    assert_eq!(
        cpu.pc, 0x8001,
        "poll delay should let one instruction execute before NMI"
    );

    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x0000, "NMI should be serviced after the delay");
}

#[test]
fn lax_zp_loads_a_and_x() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x42, 0x85, 0x10, 0xA7, 0x10]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x42);
    assert_eq!(cpu.regs.x, 0x42);
}

#[test]
fn atx_immediate_loads_operand_into_a_and_x() {
    let (mut cpu, mut bus) = setup(&[0xAB, 0x80]);
    cpu.regs.a = 0x3C;
    cpu.regs.x = 0x00;

    cpu.step(&mut bus);

    assert_eq!(cpu.regs.a, 0x80);
    assert_eq!(cpu.regs.x, 0x80);
    assert_eq!(cpu.pc, 0x8002);
    assert!(!cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(cpu.regs.get_flag(StatusFlags::NEGATIVE));
}

#[test]
fn sax_zp_stores_a_and_x() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0xFF, 0xA2, 0x0F, 0x87, 0x10, 0xA9, 0x00, 0xA5, 0x10]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x0F);
}

#[test]
fn shy_abs_x_page_cross_writes_wrapped_address_with_base_high_mask() {
    let (mut cpu, mut bus) = setup(&[0x9C, 0xFE, 0x02]);
    cpu.regs.x = 0x02;
    cpu.regs.y = 0xFF;
    bus.ram[0x0200] = 0;
    bus.ram[0x0300] = 0;
    bus.debug_trace_enabled = true;
    bus.debug_trace_events.clear();

    cpu.step(&mut bus);

    let accesses: Vec<_> = bus
        .debug_trace_events
        .iter()
        .filter_map(|event| match event {
            crate::hardware::bus::DebugTraceEvent::Read { addr, .. } if *addr == 0x0200 => {
                Some(("read", *addr, 0))
            }
            crate::hardware::bus::DebugTraceEvent::Write {
                addr, new_value, ..
            } if *addr == 0x0200 || *addr == 0x0300 => Some(("write", *addr, *new_value)),
            _ => None,
        })
        .collect();

    assert_eq!(accesses, vec![("read", 0x0200, 0), ("write", 0x0200, 0x03)]);
    assert_eq!(bus.ram[0x0200], 0x03);
    assert_eq!(bus.ram[0x0300], 0x00);
}

#[test]
fn shx_abs_y_page_cross_writes_wrapped_address_with_base_high_mask() {
    let (mut cpu, mut bus) = setup(&[0x9E, 0xFE, 0x02]);
    cpu.regs.x = 0xFF;
    cpu.regs.y = 0x02;
    bus.ram[0x0200] = 0;
    bus.ram[0x0300] = 0;
    bus.debug_trace_enabled = true;
    bus.debug_trace_events.clear();

    cpu.step(&mut bus);

    let accesses: Vec<_> = bus
        .debug_trace_events
        .iter()
        .filter_map(|event| match event {
            crate::hardware::bus::DebugTraceEvent::Read { addr, .. } if *addr == 0x0200 => {
                Some(("read", *addr, 0))
            }
            crate::hardware::bus::DebugTraceEvent::Write {
                addr, new_value, ..
            } if *addr == 0x0200 || *addr == 0x0300 => Some(("write", *addr, *new_value)),
            _ => None,
        })
        .collect();

    assert_eq!(accesses, vec![("read", 0x0200, 0), ("write", 0x0200, 0x03)]);
    assert_eq!(bus.ram[0x0200], 0x03);
    assert_eq!(bus.ram[0x0300], 0x00);
}

#[test]
fn dcp_zp_decrements_and_compares() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x43, 0x85, 0x10, 0xA9, 0x42, 0xC7, 0x10]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
}

#[test]
fn isb_zp_increments_and_subtracts() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x09, 0x85, 0x10, 0x38, 0xA9, 0x20, 0xE7, 0x10]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x16);
}

#[test]
fn php_plp_preserves_flags() {
    let (mut cpu, mut bus) = setup(&[0x38, 0x78, 0x08, 0x18, 0x58, 0x28]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(cpu.regs.get_flag(StatusFlags::INTERRUPT));
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(!cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(!cpu.regs.get_flag(StatusFlags::INTERRUPT));
    cpu.step(&mut bus);
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(cpu.regs.get_flag(StatusFlags::INTERRUPT));
}

#[test]
fn pha_pla_preserves_accumulator() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x42, 0x48, 0xA9, 0x00, 0x68]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x00);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x42);
}

#[test]
fn tax_tay_transfer() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x42, 0xAA, 0xA8]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.x, 0x42);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.y, 0x42);
}

#[test]
fn jsr_rts_round_trip() {
    let (mut cpu, mut bus) = setup(&[0x20, 0x05, 0x80, 0xEA, 0xEA, 0xA9, 0x42, 0x60]);
    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x8005);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x42);
    cpu.step(&mut bus);
    assert_eq!(cpu.pc, 0x8003);
}

#[test]
fn bit_test_flags() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0xC0, 0x85, 0x10, 0xA9, 0xFF, 0x24, 0x10]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(!cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(cpu.regs.get_flag(StatusFlags::OVERFLOW));
    assert!(cpu.regs.get_flag(StatusFlags::NEGATIVE));
}

#[test]
fn bit_test_zero_result() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0xC0, 0x85, 0x10, 0xA9, 0x00, 0x24, 0x10]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert!(cpu.regs.get_flag(StatusFlags::ZERO));
    assert!(cpu.regs.get_flag(StatusFlags::OVERFLOW));
    assert!(cpu.regs.get_flag(StatusFlags::NEGATIVE));
}

#[test]
fn asl_accumulator() {
    let (mut cpu, mut bus) = setup(&[0xA9, 0x81, 0x0A]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x02);
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
}

#[test]
fn ror_accumulator_with_carry() {
    let (mut cpu, mut bus) = setup(&[0x38, 0xA9, 0x01, 0x6A]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.a, 0x80);
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(cpu.regs.get_flag(StatusFlags::NEGATIVE));
}

#[test]
fn official_rmw_writes_original_then_modified_value() {
    let (mut cpu, mut bus) = setup(&[0x0E, 0x00, 0x02]);
    bus.ram[0x0200] = 0x41;
    bus.debug_trace_enabled = true;
    bus.debug_trace_events.clear();

    cpu.step(&mut bus);

    let writes: Vec<_> = bus
        .debug_trace_events
        .iter()
        .filter_map(|event| match event {
            crate::hardware::bus::DebugTraceEvent::Write {
                addr, new_value, ..
            } if *addr == 0x0200 => Some(*new_value),
            _ => None,
        })
        .collect();
    assert_eq!(writes, vec![0x41, 0x82]);
    assert_eq!(bus.ram[0x0200], 0x82);
}

#[test]
fn unofficial_rmw_writes_original_then_modified_value() {
    let (mut cpu, mut bus) = setup(&[0xCF, 0x00, 0x02]);
    cpu.regs.a = 0x40;
    bus.ram[0x0200] = 0x41;
    bus.debug_trace_enabled = true;
    bus.debug_trace_events.clear();

    cpu.step(&mut bus);

    let writes: Vec<_> = bus
        .debug_trace_events
        .iter()
        .filter_map(|event| match event {
            crate::hardware::bus::DebugTraceEvent::Write {
                addr, new_value, ..
            } if *addr == 0x0200 => Some(*new_value),
            _ => None,
        })
        .collect();
    assert_eq!(writes, vec![0x41, 0x40]);
    assert_eq!(bus.ram[0x0200], 0x40);
    assert!(cpu.regs.get_flag(StatusFlags::CARRY));
    assert!(cpu.regs.get_flag(StatusFlags::ZERO));
}

#[test]
fn absolute_x_read_page_cross_performs_wrapped_dummy_read() {
    let (mut cpu, mut bus) = setup(&[0xA2, 0x01, 0xBD, 0xFF, 0x02]);
    bus.ram[0x0200] = 0x11;
    bus.ram[0x0300] = 0xA5;
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.x, 1);

    bus.debug_trace_enabled = true;
    bus.debug_trace_events.clear();
    cpu.step(&mut bus);

    let reads: Vec<_> = bus
        .debug_trace_events
        .iter()
        .filter_map(|event| match event {
            crate::hardware::bus::DebugTraceEvent::Read { addr, .. }
                if *addr == 0x0200 || *addr == 0x0300 =>
            {
                Some(*addr)
            }
            _ => None,
        })
        .collect();
    assert_eq!(reads, vec![0x0200, 0x0300]);
    assert_eq!(cpu.regs.a, 0xA5);
}

#[test]
fn absolute_x_store_performs_wrapped_dummy_read_before_write() {
    let (mut cpu, mut bus) = setup(&[0xA2, 0x01, 0xA9, 0x7E, 0x9D, 0xFF, 0x02]);
    bus.ram[0x0200] = 0;
    cpu.step(&mut bus);
    cpu.step(&mut bus);

    bus.debug_trace_enabled = true;
    bus.debug_trace_events.clear();
    cpu.step(&mut bus);

    let accesses: Vec<_> = bus
        .debug_trace_events
        .iter()
        .filter_map(|event| match event {
            crate::hardware::bus::DebugTraceEvent::Read { addr, .. } if *addr == 0x0200 => {
                Some(("read", *addr, 0))
            }
            crate::hardware::bus::DebugTraceEvent::Write {
                addr, new_value, ..
            } if *addr == 0x0300 => Some(("write", *addr, *new_value)),
            _ => None,
        })
        .collect();
    assert_eq!(accesses, vec![("read", 0x0200, 0), ("write", 0x0300, 0x7E)]);
    assert_eq!(bus.ram[0x0300], 0x7E);
}

#[test]
fn inx_overflow_to_zero() {
    let (mut cpu, mut bus) = setup(&[0xA2, 0xFF, 0xE8]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.x, 0x00);
    assert!(cpu.regs.get_flag(StatusFlags::ZERO));
}

#[test]
fn dey_underflow() {
    let (mut cpu, mut bus) = setup(&[0xA0, 0x00, 0x88]);
    cpu.step(&mut bus);
    cpu.step(&mut bus);
    assert_eq!(cpu.regs.y, 0xFF);
    assert!(cpu.regs.get_flag(StatusFlags::NEGATIVE));
}
