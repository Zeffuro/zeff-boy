use super::tests::{check_clock, seed, transfers};
use super::*;
use crate::SUPERVISOR;

fn frozen(cpu: &mut PrefetchCpu, bus: &mut crate::timing::test_bus::Bus) {
    let registers = cpu.registers;
    let count = bus.events.len();
    let clock = cpu.clock();
    assert_eq!(cpu.step_instruction(bus), Err(TimingFault::Frozen));
    assert_eq!(cpu.registers, registers);
    assert_eq!(cpu.clock(), clock);
    assert_eq!(bus.events.len(), count);
    assert!(!cpu.trace_pending());
    check_clock(bus);
}

#[test]
fn every_link_and_unlk_failure_preserves_verified_register_and_write_stages() {
    for supervisor in [0, SUPERVISOR] {
        for register in 0..8 {
            for family in [0x4e50, 0x4e58] {
                let count = if family == 0x4e50 { 4 } else { 3 };
                for fail in 0..count {
                    let (mut cpu, mut bus) =
                        seed(family | register, supervisor | TRACE | 0x1f, 0xfff8, 3);
                    let initial = cpu.registers;
                    let sp = initial.stack_pointer();
                    let old = initial.address_register(usize::from(register)).unwrap();
                    bus.fail = Some(fail);
                    let Err(TimingFault::Bus { source, .. }) = cpu.step_instruction(&mut bus)
                    else {
                        panic!("missing failure")
                    };
                    assert_eq!(source, fail);
                    let mut expected = initial;
                    if family == 0x4e50 {
                        if fail >= 1 {
                            expected.set_address_register(usize::from(register), sp - 4);
                        }
                        if fail >= 3 {
                            expected.set_address_register(7, sp - 12);
                        }
                        if fail >= 2 {
                            assert_eq!(
                                bus.words[&((sp - 4) & BUS_ADDRESS_MASK)],
                                (old >> 16) as u16
                            );
                        } else {
                            assert!(!bus.words.contains_key(&((sp - 4) & BUS_ADDRESS_MASK)));
                        }
                        if fail >= 3 {
                            assert_eq!(bus.words[&((sp - 2) & BUS_ADDRESS_MASK)], old as u16);
                        } else {
                            assert!(!bus.words.contains_key(&((sp - 2) & BUS_ADDRESS_MASK)));
                        }
                    } else if fail >= 2 {
                        expected.set_address_register(7, old + 4);
                        expected.set_address_register(usize::from(register), 0x5678_4000);
                    }
                    assert_eq!(cpu.registers, expected);
                    assert_eq!(cpu.queued_words(), Some([family | register, 0xfff8]));
                    assert_eq!(bus.transfers, fail + 1);
                    frozen(&mut cpu, &mut bus);
                }
            }
        }
    }
}

#[test]
fn every_rtr_failure_preserves_ccr_and_sp_commit_before_low_pc_read() {
    for supervisor in [0, SUPERVISOR] {
        for ccr in 0..32 {
            for fail in 0..5 {
                let (mut cpu, mut bus) = seed(0x4e77, supervisor | TRACE | 0x071f, 0xffe0 | ccr, 1);
                let initial = cpu.registers;
                bus.fail = Some(fail);
                assert!(
                    matches!(cpu.step_instruction(&mut bus), Err(TimingFault::Bus { source, .. }) if source == fail)
                );
                let mut expected = initial;
                if fail >= 2 {
                    expected.set_address_register(7, initial.stack_pointer() + 6);
                    expected.set_status_register((initial.status_register() & !0x1f) | ccr);
                }
                assert_eq!(cpu.registers, expected);
                assert_eq!(cpu.queued_words(), Some([0x4e77, 0xffe0 | ccr]));
                assert_eq!(bus.transfers, fail + 1);
                frozen(&mut cpu, &mut bus);
            }
        }
    }
}

#[test]
fn trapv_prefetch_and_every_exception_failure_keep_entry_registers_and_completed_frame_words() {
    for supervisor in [0, SUPERVISOR] {
        for fail in 0..8 {
            let (mut cpu, mut bus) = seed(0x4e76, supervisor | TRACE | 0x051f, 0x7201, 3);
            let initial = cpu.registers;
            bus.fail = Some(fail);
            assert!(
                matches!(cpu.step_instruction(&mut bus), Err(TimingFault::Bus { source, .. }) if source == fail)
            );
            assert_eq!(cpu.registers, initial);
            assert_eq!(cpu.queued_words(), Some([0x4e76, 0x7201]));
            assert_eq!(
                transfers(&bus)[0].0.function_code,
                FunctionCode::SupervisorProgram
            );
            for (index, address, value) in [
                (1, 0xffe, 0x0102),
                (2, 0xffa, initial.status_register()),
                (3, 0xffc, 0x1234),
            ] {
                if fail > index {
                    assert_eq!(bus.words[&address], value);
                } else {
                    assert!(!bus.words.contains_key(&address));
                }
            }
            assert_eq!(bus.transfers, fail + 1);
            frozen(&mut cpu, &mut bus);
        }
    }
}

#[test]
fn usp_failure_retains_transfer_and_user_privilege_never_touches_usp() {
    for opcode in 0x4e60..=0x4e6f {
        let (mut cpu, mut bus) = seed(opcode, SUPERVISOR | TRACE | 0x1f, 0x7201, 0);
        let mut expected = cpu.registers;
        let register = usize::from(opcode & 7);
        if opcode & 8 == 0 {
            expected.usp = expected.address_register(register).unwrap();
        } else {
            expected.set_address_register(register, expected.usp);
        }
        bus.fail = Some(0);
        assert!(matches!(
            cpu.step_instruction(&mut bus),
            Err(TimingFault::Bus { source: 0, .. })
        ));
        assert_eq!(cpu.registers, expected);
        frozen(&mut cpu, &mut bus);
        for fail in 0..7 {
            let (mut cpu, mut bus) = seed(opcode, TRACE | 0x1f, 0x7201, 0);
            let initial = cpu.registers;
            bus.fail = Some(fail);
            assert!(
                matches!(cpu.step_instruction(&mut bus), Err(TimingFault::Bus { source, .. }) if source == fail)
            );
            assert_eq!(cpu.registers, initial);
            assert!(
                transfers(&bus)
                    .iter()
                    .all(|(access, _)| access.logical_address != initial.pc + 4)
            );
            frozen(&mut cpu, &mut bus);
        }
    }
}

#[test]
fn odd_frames_and_targets_freeze_at_their_real_transfer_boundary() {
    for supervisor in [0, SUPERVISOR] {
        let (mut cpu, mut bus) = seed(0x4e57, supervisor, 0, 0);
        cpu.registers.set_address_register(7, 0x3001);
        assert_eq!(
            cpu.step_instruction(&mut bus),
            Err(TimingFault::AddressError { address: 0x2ffd })
        );
        assert_eq!(cpu.registers.stack_pointer(), 0x2ffd);
        assert_eq!(bus.transfers, 1);
        frozen(&mut cpu, &mut bus);
        let (mut cpu, mut bus) = seed(0x4e58, supervisor, 0, 0);
        cpu.registers.set_address_register(0, 0x3001);
        assert_eq!(
            cpu.step_instruction(&mut bus),
            Err(TimingFault::AddressError { address: 0x3001 })
        );
        assert_eq!(bus.transfers, 0);
        frozen(&mut cpu, &mut bus);
        let (mut cpu, mut bus) = seed(0x4e77, supervisor | 0x1f, 0xffe0, 0);
        let sp = cpu.registers.stack_pointer();
        bus.words.insert(sp + 4, 0x4001);
        assert_eq!(
            cpu.step_instruction(&mut bus),
            Err(TimingFault::AddressError {
                address: 0x1234_4001
            })
        );
        assert_eq!(cpu.registers.stack_pointer(), sp + 6);
        assert_eq!(cpu.registers.status_register(), supervisor);
        assert_eq!(bus.transfers, 3);
        frozen(&mut cpu, &mut bus);
    }
}
