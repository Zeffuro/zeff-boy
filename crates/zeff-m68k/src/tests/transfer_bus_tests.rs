use crate::bus::{AccessKind, ByteLanes, Direction, FunctionCode};
use crate::data_tests::{execute, program};
use crate::{Fault, StepOutcome};

#[test]
fn movem_load_reads_one_discard_word_after_the_stream_including_empty_masks() {
    for long in [false, true] {
        for mask in [0_u16, 1, 3, 0xffff] {
            for postincrement in [false, true] {
                let opcode =
                    0x4c90 | if long { 0x40 } else { 0 } | if postincrement { 8 } else { 0 };
                let (mut cpu, mut bus) = program(&[opcode, mask]);
                cpu.registers.a[0] = 0x2000;
                let bytes = if long { 4 } else { 2 };
                let end = 0x2000 + mask.count_ones() * bytes;
                bus.word(end, 0xa55a);
                assert_eq!(
                    execute(&mut cpu, &mut bus),
                    12 + mask.count_ones() * bytes * 2
                );
                let last = bus.transfers.last().unwrap();
                assert_eq!(last.access.kind, AccessKind::Discard);
                assert_eq!(last.access.logical_address, end);
                assert_eq!(last.access.function_code, FunctionCode::SupervisorData);
                assert_eq!(last.access.lanes, ByteLanes::Both);
                assert_eq!(last.direction, Direction::Read);
                assert_eq!(last.value, 0xa55a);
                assert_eq!(
                    bus.transfers.len(),
                    3 + (mask.count_ones() * bytes / 2) as usize
                );
                if postincrement {
                    assert_eq!(cpu.registers.a[0], end);
                }
                assert!(cpu.registers.d.iter().all(|value| *value == 0));
            }
        }
    }
}

#[test]
fn movem_discard_read_uses_operand_space_and_wraps_the_external_address() {
    for supervisor in [false, true] {
        for (opcode, extension, address, program_space) in [
            (0x4cba_u16, vec![0x0010], 0x1014, true),
            (0x4cbb, vec![0x280c], 0x1014, true),
            (0x4cb9, vec![0xabff, 0xfffe], 0xabff_fffe, false),
        ] {
            let mut words = vec![opcode, 1];
            words.extend(extension);
            let (mut cpu, mut bus) = program(&words);
            cpu.registers.d[2] = 4;
            cpu.registers
                .set_status_register(if supervisor { 0x251f } else { 0x051f });
            bus.word(address, 0x1234);
            bus.word(address + 2, 0xabcd);
            execute(&mut cpu, &mut bus);
            let last = bus.transfers.last().unwrap();
            assert_eq!(last.access.kind, AccessKind::Discard);
            assert_eq!(last.access.logical_address, address + 2);
            assert_eq!(last.access.bus_address, (address + 2) & 0x00ff_ffff);
            assert_eq!(
                last.access.function_code,
                match (supervisor, program_space) {
                    (false, false) => FunctionCode::UserData,
                    (false, true) => FunctionCode::UserProgram,
                    (true, false) => FunctionCode::SupervisorData,
                    (true, true) => FunctionCode::SupervisorProgram,
                }
            );
            assert_eq!(cpu.registers.d[0], 0x1234);
        }
    }
}

#[test]
fn movem_discard_fault_is_observable_and_rolls_back_cpu_under_emulator_fault_policy() {
    for mask in [0_u16, 1, 3] {
        let (mut cpu, mut bus) = program(&[0x4cd8, mask]);
        cpu.registers.a[0] = 0x2000;
        bus.seed(0x2000, &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
        let original = cpu.clone();
        let fail = 2 + mask.count_ones() as usize * 2;
        bus.fail_at = Some(fail);
        assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(Fault::Bus(
            crate::bus::BusFault::Transfer { access, direction: Direction::Read, source }
        )) if access.kind == AccessKind::Discard && source == fail));
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), fail + 1);
    }
}

#[test]
fn movem_store_never_reads_a_discard_word() {
    for mask in [0_u16, 1, 0xffff] {
        for long in [false, true] {
            let (mut cpu, mut bus) = program(&[0x4890 | if long { 0x40 } else { 0 }, mask]);
            cpu.registers.a[0] = 0x2000;
            execute(&mut cpu, &mut bus);
            assert!(
                bus.transfers
                    .iter()
                    .all(|t| t.access.kind != AccessKind::Discard)
            );
        }
    }
}

#[test]
fn movem_empty_mask_still_checks_the_discard_word_alignment() {
    for opcode in [0x4c90, 0x4cd0, 0x4c98, 0x4cd8] {
        for address in [0x2001, 0xabff_ffff] {
            let (mut cpu, mut bus) = program(&[opcode, 0]);
            cpu.registers.a[0] = address;
            let original = cpu.clone();
            assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(Fault::Bus(
                crate::bus::BusFault::AddressError { access, direction: Direction::Read }
            )) if access.kind == AccessKind::Discard && access.logical_address == address));
            assert_eq!(cpu, original);
            assert_eq!(bus.transfers.len(), 2);
        }
    }
}
