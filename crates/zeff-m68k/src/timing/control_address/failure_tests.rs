use super::tests::{seed, shapes};
use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::Event;

#[test]
fn every_control_transfer_failure_preserves_staged_an_sp_and_completed_high_stack_word() {
    for base in [0x41c0, 0x4840, 0x4e80, 0x4ec0] {
        for (mode, register) in shapes() {
            for destination in 0..if base == 0x41c0 { 8 } else { 1 } {
                let opcode = base | destination << 9 | mode << 3 | register;
                for bank in [0, SUPERVISOR] {
                    let (mut completed, mut full, _) = seed(opcode, bank | TRACE | 0x1f, 0xf802);
                    let initial = completed.registers;
                    full.wait = 3;
                    completed.step_instruction(&mut full).unwrap();
                    let transfers: Vec<_> = full
                        .events
                        .iter()
                        .filter(|event| !matches!(event, Event::Idle(..)))
                        .collect();
                    let first_write = transfers
                        .iter()
                        .position(|event| matches!(event, Event::Write(..)));
                    for fail in 0..full.transfers {
                        let (mut timed, mut bus, _) = seed(opcode, bank | TRACE | 0x1f, 0xf802);
                        let queue = timed.queued_words();
                        let mut expected = initial;
                        let mut memory = bus.words.clone();
                        if base == 0x41c0 {
                            let commit =
                                usize::from(mode == 6 || mode == 7 && matches!(register, 1 | 3));
                            if fail >= commit {
                                expected.set_address_register(
                                    usize::from(destination),
                                    completed
                                        .registers
                                        .address_register(usize::from(destination))
                                        .unwrap(),
                                );
                            }
                        }
                        if first_write.is_some_and(|index| fail >= index) {
                            expected.set_address_register(7, initial.stack_pointer() - 4);
                        }
                        for event in transfers.iter().take(fail) {
                            if let Event::Write(a, value, _) = event {
                                memory.insert(a.bus_address, *value);
                            }
                        }
                        bus.wait = 3;
                        bus.fail = Some(fail);
                        assert!(
                            matches!(timed.step_instruction(&mut bus),Err(TimingFault::Bus{source,..}) if source==fail)
                        );
                        assert_eq!(timed.registers, expected, "op={opcode:04x} fail={fail}");
                        assert_eq!(timed.queued_words(), queue);
                        assert_eq!(bus.words, memory);
                        assert_eq!(timed.clock().0, bus.clock);
                        assert!(!timed.trace_pending());
                        let events = bus.events.clone();
                        assert_eq!(timed.step_boundary(&mut bus), Err(TimingFault::Frozen));
                        assert_eq!(bus.events, events);
                    }
                }
            }
        }
    }
}

#[test]
fn lea_absolute_long_failed_low_extension_keeps_old_an_and_failed_refills_keep_full_address() {
    for bank in [0, SUPERVISOR] {
        for destination in 0..8 {
            for fail in 0..3 {
                let (mut timed, mut bus, _) = seed(0x41f9 | destination << 9, bank, 0xf802);
                let initial = timed
                    .registers
                    .address_register(usize::from(destination))
                    .unwrap();
                bus.fail = Some(fail);
                assert!(matches!(
                    timed.step_instruction(&mut bus),
                    Err(TimingFault::Bus { .. })
                ));
                assert_eq!(
                    timed.registers.address_register(usize::from(destination)),
                    Some(if fail == 0 { initial } else { 0xabcd_3010 })
                );
            }
        }
    }
}

#[test]
fn invalid_control_eas_and_odd_targets_or_stack_reject_at_the_selected_stage() {
    for base in [0x41c0, 0x4840, 0x4e80, 0x4ec0] {
        for ea in 0..64 {
            let mode = ea >> 3;
            let register = ea & 7;
            if crate::instruction::control_address(mode, register) || base == 0x4840 && mode == 0 {
                continue;
            }
            let opcode = base | ea;
            let (mut timed, mut bus, _) = seed(base | 0x10, 0, 0x8702);
            timed.pipeline.as_mut().unwrap().words[0] = opcode;
            let initial = timed.clone();
            assert_eq!(
                timed.step_instruction(&mut bus),
                Err(TimingFault::UnsupportedInstruction {
                    pc: timed.registers.pc,
                    opcode
                })
            );
            assert_eq!(timed, initial);
            assert!(bus.events.is_empty());
        }
    }
    for opcode in [0x41d0, 0x4850, 0x4e90, 0x4ed0] {
        let (mut timed, mut bus, _) = seed(opcode, 0, 0x8702);
        timed.registers.set_address_register(0, 0x3001);
        if opcode == 0x41d0 || opcode == 0x4850 {
            assert!(timed.step_instruction(&mut bus).is_ok());
        } else {
            assert_eq!(
                timed.step_instruction(&mut bus),
                Err(TimingFault::AddressError { address: 0x3001 })
            );
            assert_eq!(bus.transfers, 0);
            assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
        }
    }
    for opcode in [0x4850, 0x4e90] {
        let (mut timed, mut bus, _) = seed(opcode, 0, 0x8702);
        timed.registers.usp = 0x4001;
        assert_eq!(
            timed.step_instruction(&mut bus),
            Err(TimingFault::AddressError { address: 0x3ffd })
        );
        assert_eq!(timed.registers.usp, 0x3ffd);
        assert_eq!(bus.transfers, 1);
    }
}

#[test]
fn indexed_a7_and_external_wrap_use_immutable_inputs_and_logical_addresses() {
    for bank in [0, SUPERVISOR] {
        for opcode in [0x4eb7, 0x4877, 0x4ff7] {
            for ignored in 0..8 {
                let extension = 0xf802 | ignored << 8;
                let (mut timed, mut bus, _) = seed(opcode, bank, extension);
                timed.registers.set_address_register(7, 0xffff_fffe);
                let expected = 0xffff_fffeu32.wrapping_add(0xffff_fffe).wrapping_add(2);
                let pc = timed.registers.pc;
                bus.words.insert(expected & BUS_ADDRESS_MASK, 0x7201);
                bus.words
                    .insert(expected.wrapping_add(2) & BUS_ADDRESS_MASK, 0x7402);
                timed.step_instruction(&mut bus).unwrap();
                if opcode == 0x4ff7 {
                    assert_eq!(timed.registers.address_register(7), Some(expected));
                } else {
                    assert_eq!(timed.registers.stack_pointer(), 0xffff_fffa);
                    assert_eq!(
                        bus.words[&0x00ff_fffa],
                        if opcode == 0x4877 {
                            (expected >> 16) as u16
                        } else {
                            (pc >> 16) as u16
                        }
                    );
                }
                if opcode == 0x4eb7 {
                    assert_eq!(timed.registers.pc, expected);
                }
            }
        }
    }
}
