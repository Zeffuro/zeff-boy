use crate::bus::{AccessKind, Direction, FunctionCode};
use crate::data_tests::{execute, program};
use crate::ea::Size;
use crate::test_bus::TestBus;
use crate::{CARRY, Cpu, EXTEND, NEGATIVE, OVERFLOW, StepOutcome, ZERO};

fn oracle(value: u32, width: u32, count: u32, kind: u16, left: bool, x: bool) -> (u32, u16) {
    let modulus = 1_u128 << width;
    let mask = modulus - 1;
    let value = u128::from(value) & mask;
    let sign = value >= modulus / 2;
    let signed = value as i128 - if sign { modulus as i128 } else { 0 };
    let (result, carry, extend, overflow) = if kind <= 1 {
        let result = if left {
            (value * (1_u128 << count)) & mask
        } else if kind == 0 {
            (signed >> count) as u128 & mask
        } else {
            value >> count
        };
        let carry = if count == 0 {
            false
        } else if count > width {
            !left && kind == 0 && sign
        } else {
            value & (1_u128 << if left { width - count } else { count - 1 }) != 0
        };
        let magnitude = signed * (1_i128 << count);
        let overflow = kind == 0
            && left
            && (magnitude < -(modulus as i128 / 2) || magnitude >= modulus as i128 / 2);
        (result, carry, if count == 0 { x } else { carry }, overflow)
    } else {
        let ring_width = width + u32::from(kind == 2);
        let ring_mask = (1_u128 << ring_width) - 1;
        let ring = if kind == 2 {
            value * 2 + u128::from(x)
        } else {
            value
        };
        let rotation = count % ring_width;
        let result = if left {
            ((ring << rotation) | (ring >> (ring_width - rotation))) & ring_mask
        } else {
            ((ring >> rotation) | (ring << (ring_width - rotation))) & ring_mask
        };
        if kind == 2 {
            (result >> 1, result & 1 != 0, result & 1 != 0, false)
        } else {
            let carry = count != 0
                && if left {
                    result & 1 != 0
                } else {
                    result >= modulus / 2
                };
            (result, carry, x, false)
        }
    };
    let flags = if result >= modulus / 2 { NEGATIVE } else { 0 }
        | if result == 0 { ZERO } else { 0 }
        | if carry { CARRY } else { 0 }
        | if extend { EXTEND } else { 0 }
        | if overflow { OVERFLOW } else { 0 };
    (result as u32, flags)
}

fn register_opcode(size: u16, kind: u16, left: bool, count_reg: u16, dest: u16) -> u16 {
    0xe020 | (size << 6) | (kind << 3) | (u16::from(left) << 8) | (count_reg << 9) | dest
}

#[test]
fn exhaustive_byte_shifts_match_independent_integer_and_ring_equations() {
    for kind in 0..4 {
        for left in [false, true] {
            let opcode = register_opcode(0, kind, left, 1, 0);
            let (mut cpu, mut bus) = program(&[opcode]);
            for value in 0..=255 {
                for count in 0..64 {
                    for x in [false, true] {
                        cpu.registers.pc = 0x1000;
                        cpu.registers.d[0] = 0xa5b6_c700 | value;
                        cpu.registers.d[1] = count | 0xabc0;
                        cpu.registers
                            .set_status_register(0x250f | if x { EXTEND } else { 0 });
                        bus.transfers.clear();
                        let (expected, flags) = oracle(value, 8, count, kind, left, x);
                        assert_eq!(execute(&mut cpu, &mut bus), 6 + 2 * count);
                        assert_eq!(cpu.registers.d[0], 0xa5b6_c700 | expected);
                        assert_eq!(cpu.registers.status_register(), 0x2500 | flags);
                        assert_eq!(cpu.registers.pc, 0x1002);
                        assert_eq!(bus.transfers.len(), 1);
                    }
                }
            }
        }
    }
}

#[test]
fn register_counts_cover_boundaries_and_all_sizes_and_register_pairs() {
    for (size, width) in [(0, 8), (1, 16), (2, 32)] {
        for kind in 0..4 {
            for left in [false, true] {
                for source in 0..8 {
                    for destination in 0..8 {
                        let op = register_opcode(size, kind, left, source, destination);
                        let (mut cpu, mut bus) = program(&[op]);
                        for count in [0, 1, width - 1, width, width + 1, 63] {
                            for value in [0, 1, 0x7fff, 0x8000, 0x8000_0000, u32::MAX] {
                                for x in [false, true] {
                                    cpu.registers.pc = 0x1000;
                                    cpu.registers.d[usize::from(destination)] = value;
                                    cpu.registers.d[usize::from(source)] = count;
                                    let old = cpu.registers.d[usize::from(destination)];
                                    cpu.registers.set_status_register(
                                        0x251f & if x { u16::MAX } else { !EXTEND },
                                    );
                                    bus.transfers.clear();
                                    let (expected, flags) =
                                        oracle(old, width, count, kind, left, x);
                                    assert_eq!(
                                        execute(&mut cpu, &mut bus),
                                        if size == 2 { 8 } else { 6 } + 2 * count
                                    );
                                    let mask = (u64::from(u32::MAX) >> (32 - width)) as u32;
                                    assert_eq!(
                                        cpu.registers.d[usize::from(destination)],
                                        old & !mask | expected
                                    );
                                    assert_eq!(cpu.registers.status_register(), 0x2500 | flags);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn immediate_shift_counts_include_encoded_eight_for_all_registers() {
    for size in 0..3 {
        let width = 8 << size;
        for kind in 0..4 {
            for left in [false, true] {
                for destination in 0..8 {
                    for encoded in 0..8 {
                        let op = register_opcode(size, kind, left, encoded, destination) & !0x20;
                        let (mut cpu, mut bus) = program(&[op]);
                        cpu.registers.d[usize::from(destination)] = 0x89ab_cdef;
                        cpu.registers.set_status_register(0x251f);
                        let count = if encoded == 0 { 8 } else { u32::from(encoded) };
                        let (value, flags) = oracle(0x89ab_cdef, width, count, kind, left, true);
                        let mask = (u64::from(u32::MAX) >> (32 - width)) as u32;
                        assert_eq!(
                            execute(&mut cpu, &mut bus),
                            if size == 2 { 8 } else { 6 } + 2 * count
                        );
                        assert_eq!(
                            cpu.registers.d[usize::from(destination)],
                            0x89ab_cdef & !mask | value
                        );
                        assert_eq!(cpu.registers.status_register(), 0x2500 | flags);
                    }
                }
            }
        }
    }
}

pub(crate) fn memory_program(
    opcode: u16,
    prefix: &[u16],
    mode: u16,
    reg: u16,
    size: Size,
) -> (Cpu, TestBus, u32, u32, u32) {
    let extension: &[u16] = match (mode, reg) {
        (5, _) => &[6],
        (6, _) => &[0x7002],
        (7, 0) => &[0x2000],
        (7, 1) => &[0, 0x2000],
        _ => &[],
    };
    let mut code = vec![opcode | mode << 3 | reg];
    code.extend_from_slice(prefix);
    code.extend_from_slice(extension);
    let (mut cpu, mut bus) = program(&code);
    let step = if reg == 7 && size == Size::Byte {
        2
    } else {
        size.bytes()
    };
    cpu.registers.set_address_register(usize::from(reg), 0x2000);
    cpu.registers.d[7] = 4;
    let address = match mode {
        4 => 0x2000 - step,
        5 | 6 => 0x2006,
        _ => 0x2000,
    };
    let cycles = match (mode, reg) {
        (2 | 3, _) => 4,
        (4, _) => 6,
        (5, _) | (7, 0) => 8,
        (6, _) => 10,
        (7, 1) => 12,
        _ => panic!("illegal test EA"),
    };
    let after = match mode {
        3 => 0x2000 + step,
        4 => address,
        _ => 0x2000,
    };
    bus.word(address, 0x8001);
    (cpu, bus, address, cycles, after)
}

#[test]
fn memory_shift_forms_cover_every_legal_ea_and_address_register() {
    for kind in 0..4 {
        for left in [false, true] {
            for mode in 2..=7 {
                for reg in 0..if mode == 7 { 2 } else { 8 } {
                    let opcode = 0xe0c0 | kind << 9 | u16::from(left) << 8;
                    let (mut cpu, mut bus, address, ea, after) =
                        memory_program(opcode, &[], mode, reg, Size::Word);
                    cpu.registers.set_status_register(0x251f);
                    let (expected, flags) = oracle(0x8001, 16, 1, kind, left, true);
                    assert_eq!(execute(&mut cpu, &mut bus), 8 + ea);
                    assert_eq!(
                        u16::from_be_bytes([bus.peek(address), bus.peek(address + 1)]),
                        expected as u16
                    );
                    assert_eq!(cpu.registers.status_register(), 0x2500 | flags);
                    assert_eq!(
                        cpu.registers.address_register(usize::from(reg)),
                        Some(after)
                    );
                    let data: Vec<_> = bus
                        .transfers
                        .iter()
                        .filter(|t| t.access.kind == AccessKind::Data)
                        .collect();
                    assert_eq!(data.len(), 2);
                    assert_eq!(data[0].direction, Direction::Read);
                    assert_eq!(data[1].direction, Direction::Write);
                    for transfer in data {
                        assert_eq!(transfer.access.logical_address, address);
                        assert_eq!(transfer.access.function_code, FunctionCode::SupervisorData);
                    }
                }
            }
        }
    }
}

#[test]
fn memory_shift_faults_roll_back_cpu_at_each_demand_transfer() {
    for mode in 2..=7 {
        for reg in 0..if mode == 7 { 2 } else { 8 } {
            let (mut cpu, mut bus, _, _, _) = memory_program(0xe1c0, &[], mode, reg, Size::Word);
            execute(&mut cpu, &mut bus);
            for failed in 0..bus.transfers.len() {
                let (mut cpu, mut bus, address, _, _) =
                    memory_program(0xe1c0, &[], mode, reg, Size::Word);
                let original = cpu.clone();
                bus.fail_at = Some(failed);
                assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
                assert_eq!(cpu, original);
                assert_eq!(bus.transfers.len(), failed + 1);
                assert_eq!((bus.peek(address), bus.peek(address + 1)), (0x80, 1));
            }
        }
    }
    let (mut cpu, mut bus) = program(&[0xe1d0]);
    cpu.registers.a[0] = 0x2001;
    let original = cpu.clone();
    assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
    assert_eq!(cpu, original);
    assert_eq!(bus.transfers.len(), 1);
}

#[test]
fn memory_shift_rejects_register_pc_immediate_and_later_cpu_encodings() {
    for opcode in [
        0xe0c0, 0xe0c8, 0xe0fa, 0xe0fb, 0xe0fc, 0xe0fd, 0xe8d0, 0xefd0,
    ] {
        let (mut cpu, mut bus) = program(&[opcode, 0x1234]);
        let original = cpu.clone();
        assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), 1);
    }
}
