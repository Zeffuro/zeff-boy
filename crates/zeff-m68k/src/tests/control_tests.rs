use crate::bus::{AccessKind, Direction, FunctionCode};
use crate::data_tests::{execute, program};
use crate::{StepOutcome, TRACE};

fn conditions(ccr: u16) -> [bool; 16] {
    let c = ccr & 1 != 0;
    let v = ccr & 2 != 0;
    let z = ccr & 4 != 0;
    let n = ccr & 8 != 0;
    [
        true,
        false,
        !c && !z,
        c || z,
        !c,
        c,
        !z,
        z,
        !v,
        v,
        !n,
        n,
        n == v,
        n != v,
        !z && n == v,
        z || n != v,
    ]
}

#[test]
fn all_conditions_drive_branches_dbcc_and_scc_without_changing_flags() {
    for ccr in 0..32 {
        for (code, taken) in conditions(ccr).into_iter().enumerate() {
            let high = (code as u16) << 8;
            for counter in [0, 1, 0xffff] {
                let (mut cpu, mut bus) = program(&[0x50c8 | high, 0xfffc]);
                cpu.registers.d[0] = 0xabcd_0000 | counter;
                cpu.registers.set_status_register(0x2500 | ccr);
                let decremented = (counter as u16).wrapping_sub(1);
                let branches = !taken && decremented != 0xffff;
                let cycles = execute(&mut cpu, &mut bus);
                assert_eq!(
                    cycles,
                    if taken {
                        12
                    } else if branches {
                        10
                    } else {
                        14
                    }
                );
                assert_eq!(cpu.registers.pc, if branches { 0x0ffe } else { 0x1004 });
                assert_eq!(
                    cpu.registers.d[0],
                    0xabcd_0000
                        | if taken {
                            counter
                        } else {
                            u32::from(decremented)
                        }
                );
                assert_eq!(cpu.registers.status_register(), 0x2500 | ccr);
            }
            let (mut cpu, mut bus) = program(&[0x50c0 | high]);
            cpu.registers.d[0] = 0x1234_5678;
            cpu.registers.set_status_register(0x2500 | ccr);
            assert_eq!(execute(&mut cpu, &mut bus), if taken { 6 } else { 4 });
            assert_eq!(
                cpu.registers.d[0],
                0x1234_5600 | if taken { 0xff } else { 0 }
            );
            assert_eq!(cpu.registers.status_register(), 0x2500 | ccr);
            if code == 1 {
                continue;
            }
            for word in [false, true] {
                let (mut cpu, mut bus) =
                    program(&[0x6000 | high | if word { 0 } else { 0xfc }, 0xfffc]);
                cpu.registers.set_status_register(0x2500 | ccr);
                assert_eq!(
                    execute(&mut cpu, &mut bus),
                    if taken {
                        10
                    } else if word {
                        12
                    } else {
                        8
                    }
                );
                assert_eq!(
                    cpu.registers.pc,
                    if taken {
                        0x0ffe
                    } else if word {
                        0x1004
                    } else {
                        0x1002
                    }
                );
                assert_eq!(cpu.registers.status_register(), 0x2500 | ccr);
            }
        }
    }
}

#[test]
fn scc_memory_reads_before_writing_and_updates_a7_once() {
    let (mut cpu, mut bus) = program(&[0x50df]);
    cpu.registers.ssp = 0x2000;
    bus.word(0x2000, 0x1234);
    assert_eq!(execute(&mut cpu, &mut bus), 12);
    assert_eq!(cpu.registers.ssp, 0x2002);
    assert_eq!((bus.peek(0x2000), bus.peek(0x2001)), (0xff, 0x34));
    assert_eq!(
        bus.transfers
            .iter()
            .map(|t| t.direction)
            .collect::<Vec<_>>(),
        [Direction::Read, Direction::Read, Direction::Write]
    );
}

#[test]
fn bsr_jsr_rts_push_high_first_and_pop_the_return_address() {
    for (words, return_pc, target, cycles) in [
        (vec![0x6104], 0x1002, 0x1006, 18),
        (vec![0x6100, 4], 0x1004, 0x1006, 18),
        (vec![0x4eb9, 0x0000, 0x3000], 0x1006, 0x3000, 20),
    ] {
        let (mut cpu, mut bus) = program(&words);
        cpu.registers.set_status_register(0x001f);
        cpu.registers.usp = 0x4000;
        bus.word(target, 0x4e75);
        assert_eq!(execute(&mut cpu, &mut bus), cycles);
        assert_eq!(cpu.registers.pc, target);
        assert_eq!(cpu.registers.usp, 0x3ffc);
        let writes: Vec<_> = bus
            .transfers
            .iter()
            .filter(|t| t.direction == Direction::Write)
            .collect();
        assert_eq!(
            writes
                .iter()
                .map(|t| (t.access.logical_address, t.value))
                .collect::<Vec<_>>(),
            [(0x3ffc, 0), (0x3ffe, return_pc as u16)]
        );
        assert!(
            writes
                .iter()
                .all(|t| t.access.function_code == FunctionCode::UserData
                    && t.access.kind == AccessKind::Stack)
        );
        bus.transfers.clear();
        assert_eq!(execute(&mut cpu, &mut bus), 16);
        assert_eq!(cpu.registers.pc, return_pc);
        assert_eq!(cpu.registers.usp, 0x4000);
        assert_eq!(cpu.registers.status_register(), 0x001f);
        assert_eq!(
            bus.transfers
                .iter()
                .map(|t| t.access.logical_address)
                .collect::<Vec<_>>(),
            [target, 0x3ffc, 0x3ffe]
        );
    }
}

#[test]
fn link_unlink_a7_alias_uses_original_sp_then_popped_value() {
    let (mut cpu, mut bus) = program(&[0x4e57, 0xfff8, 0x4e5f]);
    cpu.registers.ssp = 0x20_4000;
    assert_eq!(execute(&mut cpu, &mut bus), 16);
    assert_eq!(cpu.registers.ssp, 0x20_3ff4);
    assert_eq!(
        u32::from_be_bytes([
            bus.peek(0x20_3ffc),
            bus.peek(0x20_3ffd),
            bus.peek(0x20_3ffe),
            bus.peek(0x20_3fff)
        ]),
        0x20_4000
    );
    cpu.registers.ssp = 0x20_3ffc;
    assert_eq!(execute(&mut cpu, &mut bus), 12);
    assert_eq!(cpu.registers.ssp, 0x20_4000);
}

#[test]
fn control_address_modes_use_address_without_operand_read() {
    for (mode, reg, extension, target, cost) in [
        (2, 0, vec![], 0x2000, 8),
        (5, 0, vec![0xfffc], 0x1ffc, 10),
        (6, 0, vec![0x10fc], 0x2004, 14),
        (7, 0, vec![0x8000], 0xffff_8000, 10),
        (7, 1, vec![0x1234, 0x5678], 0x1234_5678, 12),
        (7, 2, vec![0xfffc], 0x0ffe, 10),
        (7, 3, vec![0x10fc], 0x1006, 14),
    ] {
        for call in [false, true] {
            let mut words = vec![if call { 0x4e80 } else { 0x4ec0 } | mode << 3 | reg];
            words.extend(&extension);
            let (mut cpu, mut bus) = program(&words);
            cpu.registers.a[0] = 0x2000;
            cpu.registers.d[1] = 8;
            cpu.registers.ssp = 0x4000;
            assert_eq!(execute(&mut cpu, &mut bus), cost + if call { 8 } else { 0 });
            assert_eq!(cpu.registers.pc, target);
            assert!(
                bus.transfers
                    .iter()
                    .all(|t| t.access.kind != AccessKind::Data)
            );
        }
    }
}

#[test]
fn rtr_restores_only_ccr_and_uses_the_current_stack_bank() {
    for supervisor in [false, true] {
        for ccr in 0..32 {
            let (mut cpu, mut bus) = program(&[0x4e77]);
            let sr = if supervisor { 0x251f } else { 0x051f };
            cpu.registers.set_status_register(sr);
            cpu.registers.ssp = 0x2000;
            cpu.registers.usp = 0x3000;
            let stack = cpu.registers.stack_pointer();
            bus.word(stack, 0xffe0 | ccr);
            bus.word(stack + 2, 0x1234);
            bus.word(stack + 4, 0x5678);
            assert_eq!(execute(&mut cpu, &mut bus), 20);
            assert_eq!(cpu.registers.pc, 0x1234_5678);
            assert_eq!(cpu.registers.status_register(), (sr & !0x1f) | ccr);
            assert_eq!(cpu.registers.ssp, if supervisor { 0x2006 } else { 0x2000 });
            assert_eq!(cpu.registers.usp, if supervisor { 0x3000 } else { 0x3006 });
            assert_eq!(
                bus.transfers[1..]
                    .iter()
                    .map(|t| t.access.logical_address)
                    .collect::<Vec<_>>(),
                [stack, stack + 2, stack + 4]
            );
            assert!(
                bus.transfers[1..]
                    .iter()
                    .all(|t| t.access.kind == AccessKind::Stack
                        && t.access.function_code
                            == if supervisor {
                                FunctionCode::SupervisorData
                            } else {
                                FunctionCode::UserData
                            })
            );
        }
    }
}

#[test]
fn rtr_read_faults_preserve_both_stacks_and_status() {
    for fail in 1..=3 {
        let (mut cpu, mut bus) = program(&[0x4e77]);
        cpu.registers.set_status_register(0x051f);
        cpu.registers.ssp = 0x2000;
        cpu.registers.usp = 0x3000;
        bus.seed(0x3000, &[0x27, 0, 0x12, 0x34, 0x56, 0x78]);
        let original = cpu.clone();
        bus.fail_at = Some(fail);
        assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), fail + 1);
    }
}

#[test]
fn stop_uses_old_privilege_new_bank_and_has_no_further_bus_work() {
    let (mut cpu, mut bus) = program(&[0x4e72, 0x0005]);
    cpu.registers.ssp = 0x2000;
    cpu.registers.usp = 0x3000;
    assert_eq!(execute(&mut cpu, &mut bus), 4);
    assert!(cpu.stopped);
    assert_eq!(cpu.registers.stack_pointer(), 0x3000);
    let count = bus.transfers.len();
    assert_eq!(cpu.step(&mut bus), StepOutcome::Stopped);
    assert_eq!(bus.transfers.len(), count);
    bus.word(0, 0);
    bus.word(2, 0x4000);
    bus.word(4, 0);
    bus.word(6, 0x1000);
    cpu.reset(&mut bus).unwrap();
    assert!(!cpu.stopped);
    assert_eq!(cpu.registers.ssp, 0x4000);
    let (mut cpu, mut bus) = program(&[0x4e72, 0x2000 | TRACE]);
    let original = cpu.clone();
    assert_eq!(
        cpu.step(&mut bus),
        StepOutcome::Fault(crate::Fault::TraceUnsupported)
    );
    assert_eq!(cpu, original);
}

#[test]
fn subroutine_faults_restore_cpu_and_keep_completed_stack_writes() {
    for fail in 1..=2 {
        let (mut cpu, mut bus) = program(&[0x6104]);
        cpu.registers.pc = 0x1234_1000;
        bus.word(0x1234_1000, 0x6104);
        cpu.registers.ssp = 0x1234_4000;
        let original = cpu.clone();
        bus.fail_at = Some(fail);
        assert!(matches!(cpu.step(&mut bus), StepOutcome::Fault(_)));
        assert_eq!(cpu, original);
        assert_eq!(bus.transfers.len(), fail + 1);
        assert_eq!(bus.peek(0x343ffc), if fail == 2 { 0x12 } else { 0 });
        assert_eq!(bus.peek(0x343ffd), if fail == 2 { 0x34 } else { 0 });
        assert_eq!(bus.peek(0x343ffe), 0);
    }
}
