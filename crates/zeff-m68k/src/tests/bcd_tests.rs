use crate::bus::{ByteLanes, Direction};
use crate::data_tests::{execute, program};
use crate::{CARRY, EXTEND, StepOutcome, ZERO};

#[test]
fn bcd_correction_and_sticky_zero_cover_valid_and_invalid_digits() {
    for (opcode, source, destination, flags, result, expected_flags) in [
        (0xc300, 1, 0x99, ZERO, 0, CARRY | EXTEND | ZERO),
        (0xc300, 1, 0x99, 0, 0, CARRY | EXTEND),
        (0xc300, 0, 0xff, 0, 0x65, CARRY | EXTEND),
        (0xc300, 0, 0x99, EXTEND, 0, CARRY | EXTEND),
        (0x8300, 0x0a, 0x10, ZERO, 0, ZERO),
        (0x8300, 1, 0, ZERO, 0x99, CARRY | EXTEND | crate::NEGATIVE),
        (0x4800, 1, 0, ZERO, 0x99, CARRY | EXTEND | crate::NEGATIVE),
    ] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.d[0] = 0xabcd_1200 | source;
        cpu.registers.d[1] = 0x5678_3400 | destination;
        cpu.registers.set_status_register(0x2500 | flags);
        assert_eq!(execute(&mut cpu, &mut bus), 6);
        let index = if opcode == 0x4800 { 0 } else { 1 };
        let high = if index == 0 { 0xabcd_1200 } else { 0x5678_3400 };
        assert_eq!(cpu.registers.d[index], high | result);
        assert_eq!(cpu.registers.status_register(), 0x2500 | expected_flags);
    }
}

#[test]
fn every_predecrement_pair_obeys_byte_lane_a7_step_and_source_destination_order() {
    for source in 0..8 {
        for destination in 0..8 {
            for subtract in [false, true] {
                let opcode = if subtract { 0x8108 } else { 0xc108 } | destination << 9 | source;
                let (mut cpu, mut bus) = program(&[opcode]);
                for register in 0..8 {
                    cpu.registers
                        .set_address_register(register, 0x2000 + register as u32 * 0x100);
                }
                let source_step = if source == 7 { 2 } else { 1 };
                let destination_step = if destination == 7 { 2 } else { 1 };
                let source_address = 0x2000 + u32::from(source) * 0x100 - source_step;
                let destination_address = 0x2000 + u32::from(destination) * 0x100
                    - destination_step
                    - if source == destination {
                        source_step
                    } else {
                        0
                    };
                bus.seed(source_address, &[1]);
                bus.seed(destination_address, &[0x20]);
                assert_eq!(execute(&mut cpu, &mut bus), 18);
                assert_eq!(
                    cpu.registers.address_register(usize::from(source)),
                    Some(
                        source_address
                            - if source == destination {
                                destination_step
                            } else {
                                0
                            }
                    )
                );
                assert_eq!(
                    cpu.registers.address_register(usize::from(destination)),
                    Some(destination_address)
                );
                assert_eq!(
                    bus.peek(destination_address),
                    if subtract { 0x19 } else { 0x21 }
                );
                let transfers = &bus.transfers[1..];
                assert_eq!(
                    transfers
                        .iter()
                        .map(|t| (t.direction, t.access.logical_address))
                        .collect::<Vec<_>>(),
                    [
                        (Direction::Read, source_address),
                        (Direction::Read, destination_address),
                        (Direction::Write, destination_address)
                    ]
                );
                assert!(transfers.iter().all(|t| t.access.lanes
                    == if t.access.logical_address & 1 == 0 {
                        ByteLanes::Upper
                    } else {
                        ByteLanes::Lower
                    }));
            }
        }
    }
}

#[test]
fn nbcd_memory_cycles_and_faults_preserve_the_explicit_emulator_contract() {
    for (opcode, extension, address, cycles) in [
        (0x4810, vec![], 0x2000, 12),
        (0x4818, vec![], 0x2000, 12),
        (0x4820, vec![], 0x1fff, 14),
        (0x4828, vec![4], 0x2004, 16),
        (0x4830, vec![4], 0x2004, 18),
        (0x4838, vec![0x2000], 0x2000, 16),
        (0x4839, vec![0, 0x2000], 0x2000, 20),
    ] {
        let mut words = vec![opcode];
        words.extend(extension);
        let (mut cpu, mut bus) = program(&words);
        cpu.registers.a[0] = 0x2000;
        bus.seed(address, &[1]);
        assert_eq!(execute(&mut cpu, &mut bus), cycles);
        assert_eq!(bus.peek(address), 0x99);
    }
    for fail in 1..=3 {
        let (mut cpu, mut bus) = program(&[0xcf0f]);
        cpu.registers.ssp = 0x2004;
        bus.seed(0x2002, &[1]);
        bus.seed(0x2000, &[2]);
        let original = cpu.clone();
        bus.fail_at = Some(fail);
        assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
        assert_eq!(cpu, original);
        assert_eq!(bus.peek(0x2000), 2);
        assert_eq!(bus.transfers.len(), fail + 1);
    }
}
