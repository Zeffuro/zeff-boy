use crate::bus::{AccessKind, BusFault, BusOperation, ByteLanes, Direction, FunctionCode};
use crate::data_tests::{execute, program};
use crate::{EXTEND, Fault, NEGATIVE, SUPERVISOR, StepOutcome, ZERO};

#[test]
fn reset_opcode_pulses_external_devices_without_resetting_cpu_registers() {
    for ccr in 0..32 {
        let (mut cpu, mut bus) = program(&[0x4e70]);
        cpu.registers.d = [0x1234_5678; 8];
        cpu.registers.a = [0x1234_2000; 7];
        cpu.registers.usp = 0x3000;
        cpu.registers.ssp = 0x4000;
        cpu.registers.set_status_register(0x2500 | ccr);
        let mut expected = cpu.registers;
        expected.pc += 2;
        bus.support_reset = true;
        assert_eq!(execute(&mut cpu, &mut bus), 132);
        assert_eq!(cpu.registers, expected);
        assert_eq!(bus.reset_pulses, [124]);
        assert_eq!(bus.reset_edges, [true, false]);
        assert!(!bus.reset_active);
        assert_eq!(bus.transfers.len(), 1);
        assert_eq!(bus.transfers[0].access.kind, AccessKind::Opcode);
    }
}

#[test]
fn unprivileged_reset_traps_before_output_even_without_bus_support() {
    for support in [false, true] {
        let (mut cpu, mut bus) = program(&[0x4e70]);
        cpu.registers.set_status_register(0x051f);
        cpu.registers.ssp = 0x2000;
        cpu.registers.usp = 0x3000;
        bus.word(32, 0);
        bus.word(34, 0x4000);
        bus.support_reset = support;
        assert_eq!(
            cpu.step(&mut bus),
            StepOutcome::Exception {
                opcode: 0x4e70,
                vector: 8,
                nominal_cycles: 34
            }
        );
        assert_eq!(cpu.registers.pc, 0x4000);
        assert_eq!(cpu.registers.status_register(), 0x051f | SUPERVISOR);
        assert!(bus.reset_pulses.is_empty());
        assert!(bus.reset_edges.is_empty());
        assert!(!bus.reset_active);
        assert_eq!(bus.transfers[1].value, 0x1000);
    }
}

#[test]
fn reset_unsupported_and_output_failure_are_typed_without_cpu_commit() {
    for support in [false, true] {
        let (mut cpu, mut bus) = program(&[0x4e70]);
        let original = cpu.clone();
        bus.support_reset = support;
        bus.reset_failure = true;
        let expected = if support {
            BusFault::ResetOutput { source: 0 }
        } else {
            BusFault::UnsupportedOperation {
                operation: BusOperation::ResetOutput,
            }
        };
        assert_eq!(cpu.step(&mut bus), StepOutcome::Fault(Fault::Bus(expected)));
        assert_eq!(cpu, original);
        assert_eq!(bus.reset_pulses.len(), usize::from(support));
        assert_eq!(
            bus.reset_edges,
            if support { vec![true, false] } else { vec![] }
        );
        assert!(!bus.reset_active);
        assert_eq!(bus.transfers.len(), 1);
    }
}

#[test]
fn tas_all_register_bytes_and_initial_flags_use_old_value_and_keep_upper_parts() {
    for reg in 0..8 {
        let (mut cpu, mut bus) = program(&[0x4ac0 | reg]);
        for value in 0..=255_u32 {
            for ccr in 0..32 {
                cpu.registers.pc = 0x1000;
                cpu.registers.d[usize::from(reg)] = 0x1234_5600 | value;
                cpu.registers.set_status_register(0x2500 | ccr);
                bus.transfers.clear();
                assert_eq!(execute(&mut cpu, &mut bus), 4);
                let flags = (ccr & EXTEND)
                    | if value == 0 { ZERO } else { 0 }
                    | if value & 0x80 != 0 { NEGATIVE } else { 0 };
                assert_eq!(cpu.registers.d[usize::from(reg)], 0x1234_5680 | value);
                assert_eq!(cpu.registers.status_register(), 0x2500 | flags);
                assert_eq!(bus.transfers.len(), 1);
                assert!(bus.lock_events.is_empty());
            }
        }
    }
}

#[test]
fn tas_memory_has_one_locked_byte_pair_for_every_legal_ea_and_function_code() {
    for (mode, reg, extension, address, ea_cycles, final_a0, final_ssp) in [
        (2, 0, vec![], 0x2000, 4, 0x2000, 0x3000),
        (3, 0, vec![], 0x2000, 4, 0x2001, 0x3000),
        (4, 0, vec![], 0x1fff, 6, 0x1fff, 0x3000),
        (5, 0, vec![1], 0x2001, 8, 0x2000, 0x3000),
        (6, 0, vec![0x1001], 0x2005, 10, 0x2000, 0x3000),
        (7, 0, vec![0x8001], 0xffff_8001, 8, 0x2000, 0x3000),
        (7, 1, vec![0xabff, 0xffff], 0xabff_ffff, 12, 0x2000, 0x3000),
        (3, 7, vec![], 0x3000, 4, 0x2000, 0x3002),
        (4, 7, vec![], 0x2ffe, 6, 0x2000, 0x2ffe),
    ] {
        for supervisor in [false, true] {
            let mut words = vec![0x4ac0 | mode << 3 | reg];
            words.extend(&extension);
            let (mut cpu, mut bus) = program(&words);
            cpu.registers.a[0] = 0x2000;
            cpu.registers.d[1] = 4;
            cpu.registers.usp = 0x3000;
            cpu.registers.ssp = 0x3000;
            cpu.registers
                .set_status_register(if supervisor { 0x251f } else { 0x051f });
            let aligned = address & !1;
            bus.word(aligned, if address & 1 == 0 { 0x0055 } else { 0x5500 });
            bus.support_tas = true;
            bus.tas_write = true;
            assert_eq!(execute(&mut cpu, &mut bus), 10 + ea_cycles);
            assert_eq!(cpu.registers.a[0], final_a0);
            assert_eq!(cpu.registers.stack_pointer(), final_ssp);
            assert_eq!(
                cpu.registers.status_register(),
                if supervisor { 0x2514 } else { 0x0514 }
            );
            assert_eq!(bus.peek(address), 0x80);
            assert_eq!(bus.peek(address ^ 1), 0x55);
            let first = 1 + extension.len();
            assert_eq!(bus.lock_events, [(first, true), (first + 2, false)]);
            let pair = &bus.transfers[first..];
            assert_eq!(pair.len(), 2);
            assert_eq!(pair[0].direction, Direction::Read);
            assert_eq!(pair[1].direction, Direction::Write);
            assert!(pair.iter().all(|t| t.access.logical_address == address
                && t.access.function_code
                    == if supervisor {
                        FunctionCode::SupervisorData
                    } else {
                        FunctionCode::UserData
                    }
                && t.access.lanes
                    == if address & 1 == 0 {
                        ByteLanes::Upper
                    } else {
                        ByteLanes::Lower
                    }));
            assert!(!bus.locked);
        }
    }
}

#[test]
fn tas_device_can_ignore_the_write_without_changing_old_value_flags() {
    for value in [0_u8, 1, 0x7f, 0x80, 0xff] {
        let (mut cpu, mut bus) = program(&[0x4ad0]);
        cpu.registers.a[0] = 0x2000;
        cpu.registers.set_status_register(0x251f);
        bus.seed(0x2000, &[value, 0x55]);
        bus.support_tas = true;
        bus.tas_write = false;
        assert_eq!(execute(&mut cpu, &mut bus), 14);
        assert_eq!(bus.peek(0x2000), value);
        assert_eq!(bus.transfers[2].direction, Direction::Write);
        assert_eq!(
            cpu.registers.status_register(),
            0x2510 | if value == 0 { ZERO } else { 0 } | if value >= 0x80 { NEGATIVE } else { 0 }
        );
    }
}

#[test]
fn tas_faults_release_bus_ownership_report_phase_and_do_not_retry() {
    for opcode in [0x4ad0, 0x4ad8, 0x4ae0] {
        for fail in [1, 2] {
            let (mut cpu, mut bus) = program(&[opcode]);
            cpu.registers.a[0] = 0x2001;
            bus.seed(0x2000, &[0, 0]);
            let original = cpu.clone();
            bus.support_tas = true;
            bus.tas_write = true;
            bus.fail_at = Some(fail);
            assert!(
                matches!(cpu.step(&mut bus), StepOutcome::Fault(Fault::Bus(BusFault::Transfer { direction, source, .. }))
                if source == fail && direction == if fail == 1 { Direction::Read } else { Direction::Write })
            );
            assert_eq!(cpu, original);
            assert_eq!(bus.transfers.len(), fail + 1);
            assert_eq!(bus.lock_events, [(1, true), (fail + 1, false)]);
            assert!(!bus.locked);
            assert_eq!(bus.peek(0x2000), 0);
            assert_eq!(bus.peek(0x2001), 0);
        }
    }
}

#[test]
fn unsupported_tas_callback_and_illegal_eas_have_no_operand_transfers() {
    for opcode in [0x4ad0, 0x4ad8, 0x4ae7] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.a[0] = 0x2000;
        cpu.registers.ssp = 0x3000;
        let original = cpu.clone();
        assert_eq!(
            cpu.step(&mut bus),
            StepOutcome::Fault(Fault::Bus(BusFault::UnsupportedOperation {
                operation: BusOperation::TestAndSet
            }))
        );
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), 1);
        assert!(bus.lock_events.is_empty());
    }
    for opcode in [0x4ac8, 0x4acf, 0x4afd, 0x4afe, 0x4aff] {
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
fn tas_callback_error_preserves_completed_device_write_while_cpu_rolls_back() {
    for odd in [false, true] {
        let (mut cpu, mut bus) = program(&[0x4ad8]);
        let address = 0x2000 + u32::from(odd);
        cpu.registers.a[0] = address;
        cpu.registers.set_status_register(0x251f);
        let original = cpu.clone();
        bus.word(0x2000, if odd { 0x5541 } else { 0x4155 });
        bus.support_tas = true;
        bus.tas_write = true;
        bus.tas_error_after_write = true;
        assert!(matches!(
            cpu.step(&mut bus),
            StepOutcome::Fault(Fault::Bus(BusFault::Transfer {
                direction: Direction::Write,
                source: 2,
                ..
            }))
        ));
        assert_eq!(cpu, original);
        assert_eq!(bus.peek(address), 0xc1);
        assert_eq!(bus.peek(address ^ 1), 0x55);
        assert_eq!(bus.lock_events, [(1, true), (3, false)]);
        assert!(!bus.locked);
    }
}
