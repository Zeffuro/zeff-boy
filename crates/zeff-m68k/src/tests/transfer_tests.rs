use crate::bus::{AccessKind, Direction, FunctionCode};
use crate::data_tests::{execute, program};
use crate::{Fault, StepOutcome};

#[test]
fn movem_store_all_masks_preserves_registers_and_ascending_selection() {
    for mask in 0..=u16::MAX {
        let (mut cpu, mut bus) = program(&[0x4890, mask]);
        cpu.registers.a[0] = 0x2000;
        for index in 0..8 {
            cpu.registers.d[index] = 0x1234_8000 + index as u32;
        }
        for index in 1..8 {
            cpu.registers
                .set_address_register(index, 0xabcd_8100 + index as u32);
        }
        let original = cpu.registers;
        let cycles = execute(&mut cpu, &mut bus);
        assert_eq!(cycles, 8 + 4 * mask.count_ones());
        let mut expected = original;
        expected.pc += 4;
        assert_eq!(cpu.registers, expected);
        let writes: Vec<_> = bus
            .transfers
            .iter()
            .filter(|t| t.direction == Direction::Write)
            .collect();
        assert_eq!(writes.len(), mask.count_ones() as usize);
        let mut offset = 0;
        for index in 0..16 {
            if mask & (1 << index) == 0 {
                continue;
            }
            let value = if index < 8 {
                original.d[index]
            } else {
                original.address_register(index - 8).unwrap()
            };
            assert_eq!(
                (writes[offset].access.logical_address, writes[offset].value),
                (0x2000 + offset as u32 * 2, value as u16)
            );
            offset += 1;
        }
    }
}

#[test]
fn movem_predecrement_reverses_mask_and_stores_original_base_low_word_first() {
    for reg in 0..8 {
        for long in [false, true] {
            let opcode = 0x48a0 | reg | if long { 0x40 } else { 0 };
            let mask = 1 << (7 - reg) | 1 << 15;
            let (mut cpu, mut bus) = program(&[opcode, mask]);
            cpu.registers
                .set_address_register(usize::from(reg), 0x1234_2000);
            cpu.registers.d[0] = 0xa1b2_c3d4;
            assert_eq!(execute(&mut cpu, &mut bus), if long { 24 } else { 16 });
            let final_address = if long { 0x1234_1ff8 } else { 0x1234_1ffc };
            assert_eq!(
                cpu.registers.address_register(usize::from(reg)),
                Some(final_address)
            );
            let writes: Vec<_> = bus
                .transfers
                .iter()
                .filter(|t| t.direction == Direction::Write)
                .map(|t| (t.access.logical_address, t.value))
                .collect();
            assert_eq!(
                writes,
                if long {
                    vec![
                        (0x1234_1ffe, 0x2000),
                        (0x1234_1ffc, 0x1234),
                        (0x1234_1ffa, 0xc3d4),
                        (0x1234_1ff8, 0xa1b2),
                    ]
                } else {
                    vec![(0x1234_1ffe, 0x2000), (0x1234_1ffc, 0xc3d4)]
                }
            );
        }
    }
}

#[test]
fn movem_load_word_sign_extends_and_postincrement_wins_over_base_in_list() {
    for reg in 0..8 {
        for long in [false, true] {
            let mask = 1 | 1 << (8 + reg);
            let opcode = 0x4c98 | reg | if long { 0x40 } else { 0 };
            let (mut cpu, mut bus) = program(&[opcode, mask]);
            cpu.registers.set_address_register(usize::from(reg), 0x2000);
            bus.seed(0x2000, &[0x80, 0x01, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc]);
            cpu.registers.set_status_register(0x251f);
            assert_eq!(execute(&mut cpu, &mut bus), if long { 28 } else { 20 });
            assert_eq!(
                cpu.registers.d[0],
                if long { 0x8001_1234 } else { 0xffff_8001 }
            );
            assert_eq!(
                cpu.registers.address_register(usize::from(reg)),
                Some(if long { 0x2008 } else { 0x2004 })
            );
            assert_eq!(cpu.registers.status_register(), 0x251f);
        }
    }
}

#[test]
fn movem_empty_mask_and_invalid_address_modes_obey_subset_policy() {
    for opcode in [0x48a0, 0x4c98] {
        let (mut cpu, mut bus) = program(&[opcode, 0]);
        cpu.registers.a[0] = 0x2000;
        assert_eq!(
            execute(&mut cpu, &mut bus),
            if opcode == 0x48a0 { 8 } else { 12 }
        );
        assert_eq!(cpu.registers.a[0], 0x2000);
        assert!(
            bus.transfers
                .iter()
                .all(|t| t.access.kind != AccessKind::Data)
        );
        assert_eq!(
            bus.transfers
                .iter()
                .filter(|t| t.access.kind == AccessKind::Discard)
                .count(),
            usize::from(opcode == 0x4c98)
        );
    }
    for opcode in [0x4898, 0x48ba, 0x48bc, 0x4ca0, 0x4cbc] {
        let (mut cpu, mut bus) = program(&[opcode, 0xffff]);
        let original = cpu.clone();
        assert_eq!(
            cpu.step(&mut bus),
            StepOutcome::Fault(Fault::UnsupportedInstruction { pc: 0x1000, opcode })
        );
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), 1);
    }
}

#[test]
fn movem_failure_preserves_completed_memory_but_rolls_back_registers() {
    for fail in 2..=5 {
        let (mut cpu, mut bus) = program(&[0x48e0, 0xc000]);
        cpu.registers.a[0] = 0x2008;
        cpu.registers.d[0] = 0x1234_5678;
        cpu.registers.d[1] = 0xabcd_ef01;
        let original = cpu.clone();
        bus.fail_at = Some(fail);
        assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), fail + 1);
        if fail > 2 {
            assert_eq!((bus.peek(0x2006), bus.peek(0x2007)), (0xef, 1));
        }
        if fail > 3 {
            assert_eq!((bus.peek(0x2004), bus.peek(0x2005)), (0xab, 0xcd));
        }
        if fail > 4 {
            assert_eq!((bus.peek(0x2002), bus.peek(0x2003)), (0x56, 0x78));
        }
    }
}

#[test]
fn movep_word_long_reads_and_writes_sparse_bytes_on_either_lane() {
    for reg in 0..8 {
        for address_reg in 0..8 {
            for long in [false, true] {
                for store in [false, true] {
                    for odd in [false, true] {
                        let opcode = 0x0108
                            | reg << 9
                            | address_reg
                            | if long { 0x40 } else { 0 }
                            | if store { 0x80 } else { 0 };
                        let (mut cpu, mut bus) = program(&[opcode, 0xfffc]);
                        let base = 0x2000 + u32::from(odd);
                        cpu.registers
                            .set_address_register(usize::from(address_reg), base + 4);
                        cpu.registers.d[usize::from(reg)] = 0x1234_abcd;
                        cpu.registers.set_status_register(0x251f);
                        bus.seed(base, &[0x11, 0x99, 0x22, 0x88, 0x33, 0x77, 0x44, 0x66]);
                        assert_eq!(execute(&mut cpu, &mut bus), if long { 24 } else { 16 });
                        assert_eq!(cpu.registers.status_register(), 0x251f);
                        assert_eq!(
                            cpu.registers.address_register(usize::from(address_reg)),
                            Some(base + 4)
                        );
                        if store {
                            let bytes = if long {
                                vec![0x12, 0x34, 0xab, 0xcd]
                            } else {
                                vec![0xab, 0xcd]
                            };
                            for (index, byte) in bytes.into_iter().enumerate() {
                                assert_eq!(bus.peek(base + index as u32 * 2), byte);
                            }
                            assert_eq!(bus.peek(base + 1), 0x99);
                        } else {
                            assert_eq!(
                                cpu.registers.d[usize::from(reg)],
                                if long { 0x1122_3344 } else { 0x1234_1122 }
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn movem_pc_relative_uses_extension_after_mask_and_program_space() {
    for (opcode, extension, address, cycles) in
        [(0x4cba, 0x0010, 0x1014, 20), (0x4cbb, 0x280c, 0x1014, 22)]
    {
        for supervisor in [false, true] {
            let (mut cpu, mut bus) = program(&[opcode, 1, extension]);
            cpu.registers.d[2] = 4;
            cpu.registers
                .set_status_register(if supervisor { 0x251f } else { 0x051f });
            bus.word(address, 0x8001);
            assert_eq!(execute(&mut cpu, &mut bus), cycles);
            assert_eq!(cpu.registers.pc, 0x1006);
            assert_eq!(cpu.registers.d[0], 0xffff_8001);
            let operand = bus
                .transfers
                .iter()
                .find(|t| t.access.kind == AccessKind::Data)
                .unwrap();
            assert_eq!(operand.access.logical_address, address);
            assert_eq!(
                operand.access.function_code,
                if supervisor {
                    FunctionCode::SupervisorProgram
                } else {
                    FunctionCode::UserProgram
                }
            );
        }
    }
}

#[test]
fn movep_faults_keep_completed_sparse_writes_and_restore_cpu() {
    for store in [false, true] {
        for fail in 1..=5 {
            let (mut cpu, mut bus) = program(&[if store { 0x01c8 } else { 0x0148 }, 1]);
            cpu.registers.a[0] = 0x2000;
            cpu.registers.d[0] = 0x1234_abcd;
            bus.seed(0x2000, &[0x55; 8]);
            let original = cpu.clone();
            bus.fail_at = Some(fail);
            assert!(matches!(
                cpu.step(&mut bus),
                StepOutcome::Fault(Fault::Bus(_))
            ));
            assert_eq!(cpu, original);
            assert_eq!(bus.transfers.len(), fail + 1);
            for position in 0..4 {
                let expected = if store && 2 + position < fail {
                    [0x12, 0x34, 0xab, 0xcd][position]
                } else {
                    0x55
                };
                assert_eq!(bus.peek(0x2001 + position as u32 * 2), expected);
                assert_eq!(bus.peek(0x2000 + position as u32 * 2), 0x55);
            }
        }
    }
}
