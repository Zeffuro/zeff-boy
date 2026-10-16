use super::tests::{branch_seed, conditions, scc_seed};
use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::Event;

#[test]
fn bsr_each_failed_write_or_target_fetch_keeps_active_sp_and_prior_high_word() {
    for opcode in [0x6120, 0x6100] {
        for bank in [0, SUPERVISOR] {
            for fail in 0..4 {
                let (mut timed, mut bus) = branch_seed(opcode, 0xffe0, bank | TRACE | 0x1f);
                let queue = timed.queued_words();
                let mut expected = timed.registers;
                let pc = expected.pc;
                let sp = expected.stack_pointer();
                expected.set_address_register(7, sp - 4);
                let mut memory = bus.words.clone();
                if fail > 0 {
                    memory.insert((sp - 4) & BUS_ADDRESS_MASK, (pc >> 16) as u16);
                }
                if fail > 1 {
                    memory.insert(
                        (sp - 2) & BUS_ADDRESS_MASK,
                        (pc + if opcode == 0x6100 { 4 } else { 2 }) as u16,
                    );
                }
                bus.fail = Some(fail);
                bus.wait = 3;
                assert!(
                    matches!(timed.step_instruction(&mut bus),Err(TimingFault::Bus{source,..}) if source==fail)
                );
                assert_eq!(timed.registers, expected);
                assert_eq!(timed.queued_words(), queue);
                assert_eq!(bus.words, memory);
                assert_eq!(timed.clock().0, bus.clock);
                let events = bus.events.clone();
                assert_eq!(timed.step_boundary(&mut bus), Err(TimingFault::Frozen));
                assert_eq!(bus.events, events);
            }
        }
    }
}

#[test]
fn dbcc_taken_expired_and_true_failures_commit_counter_only_before_the_final_fetch() {
    for (opcode, counter, transfers) in [
        (0x51c8, 0, 3),
        (0x51c8, 1, 2),
        (0x51c8, 0xffff, 2),
        (0x50c8, 0, 2),
    ] {
        for fail in 0..transfers {
            let (mut timed, mut bus) = branch_seed(opcode, 0x20, TRACE | 0x1f);
            timed.registers.d[0] = 0x1234_0000 | counter;
            let initial = timed.clone();
            bus.fail = Some(fail);
            bus.wait = 1;
            assert!(
                matches!(timed.step_instruction(&mut bus),Err(TimingFault::Bus{source,..}) if source==fail)
            );
            let expected = if opcode == 0x51c8 && fail == transfers - 1 {
                0x1234_0000 | counter.wrapping_sub(1) & 0xffff
            } else {
                initial.registers.d[0]
            };
            assert_eq!(timed.registers.d[0], expected);
            assert_eq!(timed.registers.pc, initial.registers.pc);
            assert_eq!(
                timed.registers.status_register(),
                initial.registers.status_register()
            );
            assert_eq!(timed.queued_words(), initial.queued_words());
            let events = bus.events.clone();
            assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
            assert_eq!(bus.events, events);
        }
    }
}

#[test]
fn scc_each_transfer_failure_preserves_flags_original_ea_and_register_stage() {
    for code in [0, 1] {
        for mode in [0, 2, 3, 4, 5, 6, 7] {
            for register in 0..if mode == 7 { 2 } else { 8 } {
                for bank in [0, SUPERVISOR] {
                    for odd in [false, true] {
                        let opcode = 0x50c0 | code << 8 | mode << 3 | register;
                        let (mut completed, mut full, _) =
                            scc_seed(opcode, bank | TRACE | 0x1f, odd);
                        completed.step_instruction(&mut full).unwrap();
                        for fail in 0..full.transfers {
                            let (mut timed, mut bus, _) =
                                scc_seed(opcode, bank | TRACE | 0x1f, odd);
                            let initial = timed.clone();
                            let memory = bus.words.clone();
                            bus.fail = Some(fail);
                            bus.wait = 3;
                            assert!(matches!(
                                timed.step_instruction(&mut bus),
                                Err(TimingFault::Bus { .. })
                            ));
                            let mut expected = initial.registers;
                            if mode == 0 && !conditions(0x1f)[usize::from(code)] {
                                expected.d[usize::from(register)] &= !255;
                            }
                            if matches!(mode, 3 | 4) {
                                let old = expected.address_register(usize::from(register)).unwrap();
                                let delta = if register == 7 { 2 } else { 1 };
                                expected.set_address_register(
                                    usize::from(register),
                                    if mode == 3 { old + delta } else { old - delta },
                                );
                            }
                            assert_eq!(timed.registers, expected);
                            assert_eq!(timed.queued_words(), initial.queued_words());
                            assert_eq!(bus.words, memory);
                            assert_eq!(timed.clock().0, bus.clock);
                            let events = bus.events.clone();
                            assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
                            assert_eq!(bus.events, events);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn expired_dbcc_still_checks_odd_target_and_bsr_odd_target_retains_stack_writes() {
    let (mut timed, mut bus) = branch_seed(0x51c8, 1, 0x1f);
    timed.registers.d[0] = 0x1234_0000;
    let initial = timed.clone();
    assert_eq!(
        timed.step_instruction(&mut bus),
        Err(TimingFault::AddressError {
            address: initial.registers.pc + 3
        })
    );
    assert_eq!(timed.registers, initial.registers);
    assert_eq!(bus.transfers, 0);
    assert!(matches!(bus.events[0], Event::Idle(_, 2)));
    for opcode in [0x61ff, 0x6100] {
        let (mut timed, mut bus) = branch_seed(opcode, 1, 0x1f);
        let pc = timed.registers.pc;
        let sp = timed.registers.usp;
        let target = pc.wrapping_add(if opcode == 0x61ff { 1 } else { 3 });
        assert_eq!(
            timed.step_instruction(&mut bus),
            Err(TimingFault::AddressError { address: target })
        );
        assert_eq!(timed.registers.usp, sp - 4);
        assert_eq!(bus.transfers, 2);
        assert_eq!(bus.words[&(sp - 4)], (pc >> 16) as u16);
        assert_eq!(
            bus.words[&(sp - 2)],
            (pc + if opcode == 0x61ff { 2 } else { 4 }) as u16
        );
    }
}

#[test]
fn invalid_scc_fields_reject_before_callbacks_and_trace_saves_completed_counter() {
    for ea in [0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f] {
        let (mut timed, mut bus) = branch_seed(0x50c0 | ea, 0, 0x1f);
        let initial = timed.clone();
        assert_eq!(
            timed.step_instruction(&mut bus),
            Err(TimingFault::UnsupportedInstruction {
                pc: initial.registers.pc,
                opcode: 0x50c0 | ea
            })
        );
        assert_eq!(timed, initial);
        assert!(bus.events.is_empty());
    }
    let (mut timed, mut bus) = branch_seed(0x51c8, 0x20, TRACE | 0x1f);
    timed.registers.d[0] = 0xabcd_0000;
    bus.handler(9);
    timed.step_instruction(&mut bus).unwrap();
    let completed = timed.registers;
    assert!(timed.trace_pending());
    assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), Some(34));
    assert_eq!(timed.registers.d, completed.d);
    assert_eq!(bus.words[&(completed.ssp - 2)], completed.pc as u16);
    assert_eq!(bus.words[&(completed.ssp - 6)], completed.status_register());
}
