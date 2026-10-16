use crate::test_bus::TestBus;
use crate::{Cpu, EXTEND, NEGATIVE, OVERFLOW, StepOutcome, ZERO};

pub(crate) fn program(words: &[u16]) -> (Cpu, TestBus) {
    let mut cpu = Cpu::new();
    cpu.registers.pc = 0x1000;
    let mut bus = TestBus::default();
    for (index, word) in words.iter().enumerate() {
        bus.word(0x1000 + index as u32 * 2, *word);
    }
    (cpu, bus)
}

pub(crate) fn execute(cpu: &mut Cpu, bus: &mut TestBus) -> u32 {
    match cpu.step(bus) {
        StepOutcome::Executed { nominal_cycles, .. } => nominal_cycles,
        outcome => panic!("unexpected step: {outcome:?}"),
    }
}

#[test]
fn move_sizes_preserve_register_parts_and_movea_sign_extends_without_flags() {
    for (opcode, words, expected, flags) in [
        (0x103c, vec![0x0080], 0x1234_5680, NEGATIVE),
        (0x303c, vec![0x0000], 0x1234_0000, ZERO),
        (0x203c, vec![0x8000, 0x0001], 0x8000_0001, NEGATIVE),
    ] {
        let mut code = vec![opcode];
        code.extend(words);
        let (mut cpu, mut bus) = program(&code);
        cpu.registers.d[0] = 0x1234_5678;
        cpu.registers.set_status_register(0x251f);
        execute(&mut cpu, &mut bus);
        assert_eq!(cpu.registers.d[0], expected);
        assert_eq!(cpu.registers.status_register(), 0x2500 | EXTEND | flags);
    }
    for (opcode, immediate, expected) in [(0x307c, 0x8000, 0xffff_8000), (0x307c, 0x7fff, 0x7fff)] {
        let (mut cpu, mut bus) = program(&[opcode, immediate]);
        cpu.registers.set_status_register(0x251f);
        assert_eq!(execute(&mut cpu, &mut bus), 8);
        assert_eq!(cpu.registers.a[0], expected);
        assert_eq!(cpu.registers.status_register(), 0x251f);
    }
}

#[test]
fn immediate_long_logic_uses_sixteen_clocks_and_compare_uses_fourteen() {
    for family in [0x0080, 0x0280, 0x0a80, 0x0c80] {
        for index in 0..8 {
            let (mut cpu, mut bus) = program(&[family | index, 0xffff, 0x0000]);
            cpu.registers.d[usize::from(index)] = 0x1234_5678;
            cpu.registers.set_status_register(0x251f);
            let original = cpu.registers;
            assert_eq!(
                execute(&mut cpu, &mut bus),
                if family == 0x0c80 { 14 } else { 16 }
            );
            let result = match family {
                0x0080 => 0xffff_5678,
                0x0280 => 0x1234_0000,
                0x0a80 => 0xedcb_5678,
                _ => original.d[usize::from(index)],
            };
            assert_eq!(cpu.registers.d[usize::from(index)], result);
            for other in 0..8 {
                if other != usize::from(index) {
                    assert_eq!(cpu.registers.d[other], original.d[other]);
                }
            }
            assert_eq!(bus.transfers.len(), 3);
            assert_eq!(cpu.registers.pc, 0x1006);
        }
    }
}

#[test]
fn move_source_destination_alias_updates_each_ea_once() {
    let (mut cpu, mut bus) = program(&[0x30d8]);
    cpu.registers.a[0] = 0x2000;
    bus.word(0x2000, 0xa1b2);
    bus.word(0x2002, 0xc3d4);
    assert_eq!(execute(&mut cpu, &mut bus), 12);
    assert_eq!(cpu.registers.a[0], 0x2004);
    assert_eq!((bus.peek(0x2002), bus.peek(0x2003)), (0xa1, 0xb2));
}

#[test]
fn move_destination_predecrement_long_is_low_word_first() {
    let (mut cpu, mut bus) = program(&[0x2100]);
    cpu.registers.a[0] = 0x2004;
    cpu.registers.d[0] = 0x1234_abcd;
    assert_eq!(execute(&mut cpu, &mut bus), 12);
    let writes: Vec<_> = bus
        .transfers
        .iter()
        .filter(|t| t.direction == crate::bus::Direction::Write)
        .map(|t| (t.access.logical_address, t.value))
        .collect();
    assert_eq!(writes, [(0x2002, 0xabcd), (0x2000, 0x1234)]);
}

#[test]
fn failed_move_restores_cpu_but_preserves_completed_bus_writes() {
    let (mut cpu, mut bus) = program(&[0x2100]);
    cpu.registers.a[0] = 0x2004;
    cpu.registers.d[0] = 0x1234_abcd;
    let original = cpu.clone();
    bus.fail_at = Some(2);
    assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
    assert_eq!(cpu, original);
    assert_eq!((bus.peek(0x2002), bus.peek(0x2003)), (0xab, 0xcd));
    assert_eq!((bus.peek(0x2000), bus.peek(0x2001)), (0, 0));
    assert_eq!(bus.transfers.len(), 3);
}

#[test]
fn immediate_alu_families_and_compare_preserve_storage_contracts() {
    for (opcode, old, immediate, expected, sr) in [
        (0x0000, 0x40, 0x81, 0xc1, 0x2718),
        (0x0200, 0xc1, 0x7f, 0x41, 0x2710),
        (0x0400, 0x00, 0x01, 0xff, 0x2719),
        (0x0600, 0x7f, 0x01, 0x80, 0x270a),
        (0x0a00, 0x80, 0x80, 0x00, 0x2714),
        (0x0c00, 0x80, 0x01, 0x80, 0x2712),
    ] {
        let (mut cpu, mut bus) = program(&[opcode, immediate]);
        cpu.registers.d[0] = 0xabcd_0000 | old;
        cpu.registers.set_status_register(0x271f);
        assert_eq!(execute(&mut cpu, &mut bus), 8);
        assert_eq!(cpu.registers.d[0], 0xabcd_0000 | expected);
        assert_eq!(cpu.registers.status_register(), sr);
    }
}

#[test]
fn quick_address_arithmetic_uses_full_width_and_preserves_flags() {
    for opcode in [0x5048, 0x5088, 0x5148, 0x5188] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.a[0] = 0xffff_fffc;
        cpu.registers.set_status_register(0x251f);
        assert_eq!(execute(&mut cpu, &mut bus), 8);
        assert_eq!(
            cpu.registers.a[0],
            if opcode & 0x100 == 0 { 4 } else { 0xffff_fff4 }
        );
        assert_eq!(cpu.registers.status_register(), 0x251f);
    }
}

#[test]
fn register_binary_alu_and_address_variants_obey_flags() {
    for (opcode, left, right, expected, flags) in [
        (0xd001, 0x7f, 1, 0x80, NEGATIVE | OVERFLOW),
        (0x9001, 0x80, 1, 0x7f, OVERFLOW),
        (0xb001, 0x80, 1, 0x80, EXTEND | OVERFLOW),
        (0x8001, 0x40, 0x80, 0xc0, EXTEND | NEGATIVE),
        (0xc001, 0x80, 0x7f, 0, EXTEND | ZERO),
        (0xb300, 0x80, 0x80, 0, EXTEND | ZERO),
    ] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.d[0] = 0xabcd_0000 | left;
        cpu.registers.d[1] = right;
        cpu.registers.set_status_register(0x271f);
        execute(&mut cpu, &mut bus);
        assert_eq!(cpu.registers.d[0], 0xabcd_0000 | expected);
        assert_eq!(cpu.registers.status_register(), 0x2700 | flags);
    }
    let (mut cpu, mut bus) = program(&[0xd0fc, 0xffff, 0xb0fc, 0xffff]);
    cpu.registers.a[0] = 0;
    cpu.registers.set_status_register(0x271f);
    execute(&mut cpu, &mut bus);
    assert_eq!(cpu.registers.a[0], u32::MAX);
    assert_eq!(cpu.registers.status_register(), 0x271f);
    execute(&mut cpu, &mut bus);
    assert_eq!(cpu.registers.status_register(), 0x2714);
}

#[test]
fn unary_operations_and_extensions_preserve_register_upper_parts() {
    for (opcode, initial, expected) in [
        (0x4200, 0xabcd_ffff, 0xabcd_ff00),
        (0x4400, 0xabcd_ff01, 0xabcd_ffff),
        (0x4600, 0xabcd_ff80, 0xabcd_ff7f),
        (0x4a00, 0xabcd_ff80, 0xabcd_ff80),
        (0x4880, 0xabcd_0080, 0xabcd_ff80),
        (0x48c0, 0xabcd_8000, 0xffff_8000),
        (0x4840, 0x1234_abcd, 0xabcd_1234),
    ] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.d[0] = initial;
        execute(&mut cpu, &mut bus);
        assert_eq!(cpu.registers.d[0], expected);
    }
}

#[test]
fn cmpm_alias_reads_sequential_operands_and_preserves_x() {
    for (opcode, bytes, step, cycles, flags) in [
        (0xbf0f, vec![0x7f, 0x12, 0x80, 0x34], 2, 12, OVERFLOW),
        (0xbf4f, vec![0x7f, 0xff, 0x80, 0x00], 2, 12, OVERFLOW),
        (
            0xbf8f,
            vec![0x7f, 0xff, 0xff, 0xff, 0x80, 0, 0, 0],
            4,
            20,
            OVERFLOW,
        ),
    ] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.ssp = 0x2000;
        cpu.registers.set_status_register(0x271f);
        bus.seed(0x2000, &bytes);
        let initial = bus.memory.clone();
        assert_eq!(execute(&mut cpu, &mut bus), cycles);
        assert_eq!(cpu.registers.ssp, 0x2000 + 2 * step);
        assert_eq!(cpu.registers.status_register(), 0x2700 | EXTEND | flags);
        assert_eq!(bus.memory, initial);
        assert!(
            bus.transfers
                .iter()
                .all(|t| t.direction == crate::bus::Direction::Read)
        );
    }
}
