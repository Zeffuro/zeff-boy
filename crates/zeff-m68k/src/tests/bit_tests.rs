use crate::bus::{AccessKind, ByteLanes, Direction, FunctionCode};
use crate::data_tests::{execute, program};
use crate::ea::Size;
use crate::instruction::Context;
use crate::shift_tests::memory_program;
use crate::{StepOutcome, ZERO};

fn modified(value: u32, bit: u32, operation: u16) -> u32 {
    let power = 2_u32.pow(bit);
    let was_set = !(value / power).is_multiple_of(2);
    match operation {
        1 if was_set => value - power,
        1 => value + power,
        2 if was_set => value - power,
        3 if !was_set => value + power,
        _ => value,
    }
}

fn register_cycles(operation: u16, bit: u32, immediate: bool) -> u32 {
    let cycles = match (operation, bit % 32 >= 16) {
        (0, _) | (1 | 3, false) => 6,
        (1 | 3, true) | (2, false) => 8,
        (2, true) => 10,
        _ => unreachable!(),
    };
    cycles + if immediate { 4 } else { 0 }
}

#[test]
fn register_bit_operations_cover_counts_aliases_and_variable_half_timing() {
    for operation in 0..4 {
        for immediate in [false, true] {
            for source in 0..8 {
                for destination in 0..8 {
                    let op = (if immediate {
                        0x0800
                    } else {
                        0x0100 | source << 9
                    }) | operation << 6
                        | destination;
                    let (mut cpu, mut bus) = program(&[op, 0]);
                    for bit in 0..64 {
                        for value in [0, u32::MAX, 0xa5a5_5a5a, 0x8000_0001] {
                            cpu.registers.pc = 0x1000;
                            cpu.registers.d[usize::from(destination)] = value;
                            if immediate {
                                bus.word(0x1002, bit as u16 | 0xab00);
                            } else {
                                cpu.registers.d[usize::from(source)] = bit | 0xabc0;
                            }
                            let before = cpu.registers.d[usize::from(destination)];
                            cpu.registers.set_status_register(0x251f);
                            bus.transfers.clear();
                            assert_eq!(
                                execute(&mut cpu, &mut bus),
                                register_cycles(operation, bit, immediate)
                            );
                            assert_eq!(
                                cpu.registers.d[usize::from(destination)],
                                modified(before, bit % 32, operation)
                            );
                            let zero = (before / 2_u32.pow(bit % 32)).is_multiple_of(2);
                            assert_eq!(
                                cpu.registers.status_register(),
                                0x251b | if zero { ZERO } else { 0 }
                            );
                            assert_eq!(cpu.registers.pc, if immediate { 0x1004 } else { 0x1002 });
                            assert_eq!(bus.transfers.len(), if immediate { 2 } else { 1 });
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn exhaustive_memory_byte_bit_operations_use_modulo_eight_and_initial_z_test() {
    for operation in 0..4 {
        for immediate in [false, true] {
            let opcode = (if immediate { 0x0800 } else { 0x0300 }) | operation << 6 | 0x10;
            let (mut cpu, mut bus) = program(&[opcode, 0]);
            cpu.registers.a[0] = 0x2001;
            for value in 0..=255_u32 {
                for bit in 0..64 {
                    cpu.registers.pc = 0x1000;
                    cpu.registers.d[1] = bit | 0xabc0;
                    cpu.registers.set_status_register(0x251f);
                    bus.word(0x1002, bit as u16 | 0xab00);
                    bus.seed(0x2000, &[0xa5, value as u8, 0x5a]);
                    bus.transfers.clear();
                    let cycles =
                        if operation == 0 { 8 } else { 12 } + if immediate { 4 } else { 0 };
                    assert_eq!(execute(&mut cpu, &mut bus), cycles);
                    assert_eq!(bus.peek(0x2001), modified(value, bit % 8, operation) as u8);
                    assert_eq!((bus.peek(0x2000), bus.peek(0x2002)), (0xa5, 0x5a));
                    let zero = (value / 2_u32.pow(bit % 8)).is_multiple_of(2);
                    assert_eq!(
                        cpu.registers.status_register(),
                        0x251b | if zero { ZERO } else { 0 }
                    );
                    let data: Vec<_> = bus
                        .transfers
                        .iter()
                        .filter(|t| t.access.kind == AccessKind::Data)
                        .collect();
                    assert_eq!(data.len(), if operation == 0 { 1 } else { 2 });
                    for transfer in data {
                        assert_eq!(transfer.access.lanes, ByteLanes::Lower);
                        assert_eq!(transfer.access.logical_address, 0x2001);
                    }
                }
            }
        }
    }
}

#[test]
fn all_memory_alterable_bit_eas_update_address_registers_once() {
    for operation in 0..4 {
        for immediate in [false, true] {
            for mode in 2..=7 {
                for reg in 0..if mode == 7 { 2 } else { 8 } {
                    let opcode = (if immediate { 0x0800 } else { 0x0300 }) | operation << 6;
                    let prefix: &[u16] = if immediate { &[7] } else { &[] };
                    let (mut cpu, mut bus, address, ea, after) =
                        memory_program(opcode, prefix, mode, reg, Size::Byte);
                    cpu.registers.d[1] = 7;
                    cpu.registers.set_status_register(0x251f);
                    let base = if operation == 0 { 4 } else { 8 };
                    assert_eq!(
                        execute(&mut cpu, &mut bus),
                        base + ea + if immediate { 4 } else { 0 }
                    );
                    assert_eq!(bus.peek(address), modified(0x80, 7, operation) as u8);
                    assert_eq!(bus.peek(address + 1), 1);
                    assert_eq!(cpu.registers.status_register(), 0x251b);
                    assert_eq!(
                        cpu.registers.address_register(usize::from(reg)),
                        Some(after)
                    );
                    let data: Vec<_> = bus
                        .transfers
                        .iter()
                        .filter(|t| t.access.kind == AccessKind::Data)
                        .collect();
                    assert_eq!(data.len(), if operation == 0 { 1 } else { 2 });
                    assert_eq!(data[0].direction, Direction::Read);
                    if operation != 0 {
                        assert_eq!(data[1].direction, Direction::Write);
                    }
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
fn btst_accepts_pc_relative_and_only_dynamic_immediate_ea() {
    for immediate in [false, true] {
        for indexed in [false, true] {
            let opcode = if immediate { 0x083a } else { 0x033a } + u16::from(indexed);
            let extension = if indexed { 0x7000 } else { 0x0040 };
            let code = if immediate {
                vec![opcode, 7, extension]
            } else {
                vec![opcode, extension]
            };
            let (mut cpu, mut bus) = program(&code);
            cpu.registers.d[1] = 7;
            cpu.registers.d[7] = 0x40;
            let address = if immediate { 0x1044 } else { 0x1042 };
            bus.seed(address, &[0x80]);
            cpu.registers.set_status_register(0x251f);
            assert_eq!(
                execute(&mut cpu, &mut bus),
                12 + if immediate { 4 } else { 0 } + if indexed { 2 } else { 0 }
            );
            assert_eq!(cpu.registers.status_register(), 0x251b);
            let last = bus.transfers.last().unwrap();
            assert_eq!(last.access.logical_address, address);
            assert_eq!(last.access.function_code, FunctionCode::SupervisorProgram);
            assert_eq!(last.direction, Direction::Read);
        }
    }
    let (mut cpu, mut bus) = program(&[0x033c, 0x00a5]);
    cpu.registers.d[1] = 63;
    cpu.registers.set_status_register(0x251f);
    assert_eq!(execute(&mut cpu, &mut bus), 10);
    assert_eq!(cpu.registers.status_register(), 0x251b);
    assert_eq!(cpu.registers.pc, 0x1004);
    assert_eq!(bus.transfers.len(), 2);
}

#[test]
fn bit_instructions_preserve_every_ccr_combination_except_z() {
    for operation in 0..4 {
        for ccr in 0..32 {
            for value in [0, 1] {
                let (mut cpu, mut bus) = program(&[0x0800 | operation << 6, 0]);
                cpu.registers.d[0] = value;
                cpu.registers.set_status_register(0x2500 | ccr);
                execute(&mut cpu, &mut bus);
                assert_eq!(
                    cpu.registers.status_register(),
                    0x2500 | (ccr & !ZERO) | if value == 0 { ZERO } else { 0 }
                );
            }
        }
    }
}

#[test]
fn bit_memory_faults_roll_back_cpu_and_retain_completed_reads() {
    for operation in 0..4 {
        for immediate in [false, true] {
            for mode in 2..=7 {
                for reg in 0..if mode == 7 { 2 } else { 8 } {
                    let opcode = (if immediate { 0x0800 } else { 0x0300 }) | operation << 6;
                    let prefix: &[u16] = if immediate { &[7] } else { &[] };
                    let (mut cpu, mut bus, _, _, _) =
                        memory_program(opcode, prefix, mode, reg, Size::Byte);
                    cpu.registers.d[1] = 7;
                    let original = cpu.clone();
                    execute(&mut cpu, &mut bus);
                    for failed in 0..bus.transfers.len() {
                        let (mut cpu, mut bus, address, _, _) =
                            memory_program(opcode, prefix, mode, reg, Size::Byte);
                        cpu.registers.d[1] = 7;
                        bus.fail_at = Some(failed);
                        assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
                        assert_eq!(cpu, original);
                        assert_eq!(bus.transfers.len(), failed + 1);
                        assert_eq!((bus.peek(address), bus.peek(address + 1)), (0x80, 1));
                    }
                }
            }
        }
    }
}

#[test]
fn bit_decoder_rejects_illegal_eas_and_leaves_movep_unclaimed() {
    for opcode in [
        0x0808, 0x083c, 0x083d, 0x087a, 0x087b, 0x087c, 0x08ba, 0x08fb, 0x037a, 0x03bb, 0x03fc,
    ] {
        let (mut cpu, mut bus) = program(&[opcode, 7]);
        let original = cpu.clone();
        assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), 1);
    }
    for source in 0..8 {
        for destination in 0..8 {
            for operation in 0..4 {
                let opcode = 0x0108 | source << 9 | operation << 6 | destination;
                let (mut cpu, mut bus) = program(&[opcode]);
                let mut ctx = Context {
                    regs: &mut cpu.registers,
                    bus: &mut bus,
                    pc: 0x1000,
                    opcode,
                    cursor: 0x1002,
                    next_pc: None,
                    exception: None,
                    stopped: false,
                };
                assert_eq!(crate::bit::execute(&mut ctx).unwrap(), None);
                assert!(bus.transfers.is_empty());
            }
        }
    }
}
