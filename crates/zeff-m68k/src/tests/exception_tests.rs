use crate::bus::{AccessKind, Direction, FunctionCode};
use crate::data_tests::{execute, program};
use crate::{OVERFLOW, SUPERVISOR, StepOutcome};

#[test]
fn line_a_f_and_reserved_illegal_opcodes_stack_the_opcode_pc() {
    for (opcode, vector) in [
        (0x4afa, 4),
        (0x4afb, 4),
        (0x4afc, 4),
        (0xa000, 10),
        (0xafff, 10),
        (0xf000, 11),
        (0xffff, 11),
    ] {
        for supervisor in [false, true] {
            let (mut cpu, mut bus) = program(&[opcode]);
            let sr = 0x051b | if supervisor { SUPERVISOR } else { 0 };
            cpu.registers.set_status_register(sr);
            cpu.registers.ssp = 0x2000;
            cpu.registers.usp = 0x3000;
            bus.word(u32::from(vector) * 4, 0x1234);
            bus.word(u32::from(vector) * 4 + 2, 0x5000);
            assert_eq!(
                cpu.step(&mut bus),
                StepOutcome::Exception {
                    opcode,
                    vector,
                    nominal_cycles: 34
                }
            );
            assert_eq!(cpu.registers.pc, 0x1234_5000);
            assert_eq!(cpu.registers.ssp, 0x1ffa);
            assert_eq!(cpu.registers.usp, 0x3000);
            assert_eq!(bus.transfers[1].value, 0x1000);
            assert_eq!(bus.transfers[2].value, sr);
            assert_eq!(bus.transfers.len(), 6);
        }
    }
}

#[test]
fn traps_stack_short_frame_in_hardware_order_and_use_supervisor_data_vectors() {
    for trap in 0..16 {
        for supervisor in [false, true] {
            let (mut cpu, mut bus) = program(&[0x4e40 | trap]);
            let sr = 0x051b | if supervisor { SUPERVISOR } else { 0 };
            cpu.registers.set_status_register(sr);
            cpu.registers.ssp = 0x1234_2000;
            cpu.registers.usp = 0x3000;
            let vector = 32 + trap as u8;
            bus.word(u32::from(vector) * 4, 0x1234);
            bus.word(u32::from(vector) * 4 + 2, 0x5000);
            assert_eq!(
                cpu.step(&mut bus),
                StepOutcome::Exception {
                    opcode: 0x4e40 | trap,
                    vector,
                    nominal_cycles: 34
                }
            );
            assert_eq!(cpu.registers.pc, 0x1234_5000);
            assert_eq!(cpu.registers.ssp, 0x1234_1ffa);
            assert_eq!(cpu.registers.usp, 0x3000);
            assert_eq!(cpu.registers.status_register(), sr | SUPERVISOR);
            let transfers = &bus.transfers[1..];
            assert_eq!(
                transfers
                    .iter()
                    .map(|t| (t.direction, t.access.logical_address, t.value))
                    .collect::<Vec<_>>(),
                [
                    (Direction::Write, 0x1234_1ffe, 0x1002),
                    (Direction::Write, 0x1234_1ffa, sr),
                    (Direction::Write, 0x1234_1ffc, 0),
                    (Direction::Read, u32::from(vector) * 4, 0x1234),
                    (Direction::Read, u32::from(vector) * 4 + 2, 0x5000),
                ]
            );
            assert!(
                transfers
                    .iter()
                    .all(|t| t.access.function_code == FunctionCode::SupervisorData)
            );
            assert!(
                transfers[..3]
                    .iter()
                    .all(|t| t.access.kind == AccessKind::Stack)
            );
            assert!(
                transfers[3..]
                    .iter()
                    .all(|t| t.access.kind == AccessKind::Vector)
            );
        }
    }
}

#[test]
fn privilege_checks_precede_extensions_operands_and_ordinary_stack_reads() {
    for opcode in [
        0x4e72, 0x4e73, 0x4e60, 0x4e6f, 0x007c, 0x027c, 0x0a7c, 0x46fc, 0x46f9,
    ] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.set_status_register(0x001f);
        cpu.registers.ssp = 0x2000;
        cpu.registers.usp = 0x3000;
        bus.word(32, 0);
        bus.word(34, 0x4000);
        assert_eq!(
            cpu.step(&mut bus),
            StepOutcome::Exception {
                opcode,
                vector: 8,
                nominal_cycles: 34
            }
        );
        assert_eq!(bus.transfers.len(), 6);
        assert_eq!(
            bus.transfers[0].access.function_code,
            FunctionCode::UserProgram
        );
        assert_eq!(bus.transfers[1].value, 0x1000);
        assert_eq!(bus.transfers[2].value, 0x001f);
        assert_eq!(cpu.registers.usp, 0x3000);
    }
}

#[test]
fn illegal_saves_opcode_pc_trapv_saves_following_pc_only_when_taken() {
    for (opcode, sr, vector, saved_pc) in [
        (0x4afc, 0x2700, 4, 0x1000),
        (0x4e76, 0x2700 | OVERFLOW, 7, 0x1002),
    ] {
        let (mut cpu, mut bus) = program(&[opcode]);
        cpu.registers.ssp = 0x2000;
        cpu.registers.set_status_register(sr);
        bus.word(u32::from(vector) * 4, 0);
        bus.word(u32::from(vector) * 4 + 2, 0x4000);
        assert_eq!(
            cpu.step(&mut bus),
            StepOutcome::Exception {
                opcode,
                vector,
                nominal_cycles: 34
            }
        );
        assert_eq!(bus.transfers[1].value, saved_pc);
    }
    let (mut cpu, mut bus) = program(&[0x4e76]);
    assert_eq!(execute(&mut cpu, &mut bus), 4);
    assert_eq!(bus.transfers.len(), 1);
}

#[test]
fn rte_reads_sr_then_pc_and_advances_ssp_before_selecting_user_bank() {
    for sr in [0x001f, 0x2510] {
        let (mut cpu, mut bus) = program(&[0x4e73]);
        cpu.registers.ssp = 0x2000;
        cpu.registers.usp = 0x3000;
        bus.word(0x2000, sr);
        bus.word(0x2002, 0x1234);
        bus.word(0x2004, 0x5678);
        assert_eq!(execute(&mut cpu, &mut bus), 20);
        assert_eq!(cpu.registers.pc, 0x1234_5678);
        assert_eq!(cpu.registers.ssp, 0x2006);
        assert_eq!(cpu.registers.usp, 0x3000);
        assert_eq!(
            cpu.registers.stack_pointer(),
            if sr & SUPERVISOR == 0 { 0x3000 } else { 0x2006 }
        );
        assert_eq!(cpu.registers.status_register(), sr);
        assert_eq!(
            bus.transfers[1..]
                .iter()
                .map(|t| t.access.logical_address)
                .collect::<Vec<_>>(),
            [0x2000, 0x2002, 0x2004]
        );
        assert!(
            bus.transfers[1..]
                .iter()
                .all(|t| t.access.function_code == FunctionCode::SupervisorData
                    && t.access.kind == AccessKind::Stack)
        );
    }
}

#[test]
fn exception_and_rte_bus_faults_leave_cpu_unchanged_without_fake_halt() {
    for fail in 1..=5 {
        let (mut cpu, mut bus) = program(&[0x4e40]);
        cpu.registers.ssp = 0x2000;
        bus.word(128, 0x1234);
        bus.word(130, 0x5000);
        let original = cpu.clone();
        bus.fail_at = Some(fail);
        assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), fail + 1);
        if fail > 1 {
            assert_eq!((bus.peek(0x1ffe), bus.peek(0x1fff)), (0x10, 2));
        }
        if fail > 2 {
            assert_eq!((bus.peek(0x1ffa), bus.peek(0x1ffb)), (0x27, 0));
        }
    }
    for fail in 1..=3 {
        let (mut cpu, mut bus) = program(&[0x4e73]);
        cpu.registers.ssp = 0x2000;
        bus.word(0x2000, 0);
        bus.word(0x2002, 0x1234);
        bus.word(0x2004, 0x5678);
        let original = cpu.clone();
        bus.fail_at = Some(fail);
        assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
        assert_eq!(cpu, original);
    }
}

#[test]
fn status_moves_obey_68000_user_sr_access_and_ccr_mask() {
    let (mut cpu, mut bus) = program(&[0x40c0, 0x44fc, 0xffe5, 0x003c, 0xff02]);
    cpu.registers.set_status_register(0x051f);
    cpu.registers.d[0] = 0xabcd_0000;
    assert_eq!(execute(&mut cpu, &mut bus), 6);
    assert_eq!(cpu.registers.d[0], 0xabcd_051f);
    assert_eq!(execute(&mut cpu, &mut bus), 16);
    assert_eq!(cpu.registers.status_register(), 0x0505);
    assert_eq!(execute(&mut cpu, &mut bus), 20);
    assert_eq!(cpu.registers.status_register(), 0x0507);
}

#[test]
fn invalid_status_move_encoding_is_unsupported_before_privilege_handling() {
    for sr in [0, SUPERVISOR] {
        let (mut cpu, mut bus) = program(&[0x46c8]);
        cpu.registers.set_status_register(sr);
        let original = cpu.clone();
        assert_eq!(
            cpu.step(&mut bus),
            StepOutcome::Fault(crate::Fault::UnsupportedInstruction {
                pc: 0x1000,
                opcode: 0x46c8
            })
        );
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), 1);
    }
}
