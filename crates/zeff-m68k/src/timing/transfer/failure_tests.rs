use super::tests::{assign, masks, movem_seed, movep_seed, opcode, shapes, transfers};
use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, Event};

fn frozen(cpu: &mut PrefetchCpu, bus: &mut Bus) {
    let state = cpu.clone();
    let events = bus.events.clone();
    assert_eq!(cpu.step_instruction(bus), Err(TimingFault::Frozen));
    assert_eq!(cpu.service_pending_trace(bus), Err(TimingFault::Frozen));
    assert_eq!(cpu.step_boundary(bus), Err(TimingFault::Frozen));
    assert_eq!(*cpu, state);
    assert_eq!(bus.events, events);
    assert_eq!(cpu.clock().0, bus.clock);
}

fn completed_writes(
    initial: &Bus,
    events: &[(BusAccess, Option<u16>)],
    fail: usize,
) -> std::collections::BTreeMap<u32, u16> {
    let mut memory = initial.words.clone();
    for (access, value) in events.iter().take(fail) {
        if let Some(value) = value {
            let old = memory.get(&access.bus_address).copied().unwrap_or_default();
            memory.insert(
                access.bus_address,
                match access.lanes {
                    ByteLanes::Both => *value,
                    ByteLanes::Upper => old & 0xff | value & 0xff00,
                    ByteLanes::Lower => old & 0xff00 | value & 0xff,
                },
            );
        }
    }
    memory
}

#[test]
fn every_movem_failure_retains_complete_loads_bus_words_and_verified_base_commit_stage() {
    for shape in shapes() {
        let (load, mode, base_register) = shape;
        for long in [false, true] {
            for mask in masks() {
                for bank in [0, SUPERVISOR] {
                    let (mut completed, mut full, address, _) =
                        movem_seed(shape, long, mask, bank | TRACE | 0x071f, 0xf810);
                    full.wait = 3;
                    completed.step_instruction(&mut full).unwrap();
                    let events = transfers(&full);
                    let selected: Vec<_> = (0..16).filter(|bit| mask & (1 << bit) != 0).collect();
                    for fail in 0..events.len() {
                        let (mut cpu, mut bus, _, _) =
                            movem_seed(shape, long, mask, bank | TRACE | 0x071f, 0xf810);
                        let mut expected = cpu.registers;
                        let queue = cpu.queued_words();
                        if load {
                            let successful: Vec<_> = events
                                .iter()
                                .take(fail)
                                .filter(|(access, _)| access.kind == AccessKind::Data)
                                .collect();
                            let words = if long { 2 } else { 1 };
                            for (position, chunk) in successful.chunks_exact(words).enumerate() {
                                let high = bus.words[&chunk[0].0.bus_address];
                                let value = if long {
                                    u32::from(high) << 16
                                        | u32::from(bus.words[&chunk[1].0.bus_address])
                                } else {
                                    i32::from(high as i16) as u32
                                };
                                assign(&mut expected, selected[position], value);
                            }
                            if mode == 3
                                && events
                                    .iter()
                                    .take(fail)
                                    .any(|(access, _)| access.kind == AccessKind::Discard)
                            {
                                expected.set_address_register(
                                    base_register,
                                    address.wrapping_add(
                                        selected.len() as u32 * if long { 4 } else { 2 },
                                    ),
                                );
                            }
                        } else if mode == 4 && fail == events.len() - 1 {
                            expected.set_address_register(
                                base_register,
                                address
                                    .wrapping_sub(selected.len() as u32 * if long { 4 } else { 2 }),
                            );
                        }
                        let memory = completed_writes(&bus, &events, fail);
                        bus.wait = 3;
                        bus.fail = Some(fail);
                        assert!(
                            matches!(cpu.step_instruction(&mut bus), Err(TimingFault::Bus { source, .. }) if source == fail)
                        );
                        assert_eq!(
                            cpu.registers,
                            expected,
                            "{:04x}/{mask:04x}/failure{fail}",
                            opcode(shape, long)
                        );
                        assert_eq!(cpu.queued_words(), queue);
                        assert_eq!(bus.words, memory);
                        assert_eq!(bus.transfers, fail + 1);
                        assert!(!cpu.trace_pending());
                        frozen(&mut cpu, &mut bus);
                    }
                }
            }
        }
    }
}

#[test]
fn every_movep_failure_preserves_sparse_completed_lanes_and_commits_only_full_input() {
    for destination in 0..8 {
        for address_register in 0..8 {
            for long in [false, true] {
                for store in [false, true] {
                    for parity in [0, 1] {
                        for bank in [0, SUPERVISOR] {
                            let opcode = 0x0108
                                | destination << 9
                                | address_register
                                | if long { 0x40 } else { 0 }
                                | if store { 0x80 } else { 0 };
                            let (mut completed, mut full, _) =
                                movep_seed(opcode, bank | TRACE | 0x1f, 0xfffe, parity);
                            full.wait = 3;
                            completed.step_instruction(&mut full).unwrap();
                            let events = transfers(&full);
                            for fail in 0..events.len() {
                                let (mut cpu, mut bus, _) =
                                    movep_seed(opcode, bank | TRACE | 0x1f, 0xfffe, parity);
                                let mut expected = cpu.registers;
                                let queue = cpu.queued_words();
                                if !store && fail == events.len() - 1 {
                                    expected.d[usize::from(destination)] =
                                        completed.registers.d[usize::from(destination)];
                                }
                                let memory = completed_writes(&bus, &events, fail);
                                bus.wait = 3;
                                bus.fail = Some(fail);
                                assert!(
                                    matches!(cpu.step_instruction(&mut bus), Err(TimingFault::Bus { source, .. }) if source == fail)
                                );
                                assert_eq!(
                                    cpu.registers, expected,
                                    "{opcode:04x}/{parity}/failure{fail}"
                                );
                                assert_eq!(cpu.queued_words(), queue);
                                assert_eq!(bus.words, memory);
                                assert_eq!(bus.transfers, fail + 1);
                                assert!(!cpu.trace_pending());
                                frozen(&mut cpu, &mut bus);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn empty_movem_load_reads_one_word_discard_and_empty_store_never_reads_operand() {
    for long in [false, true] {
        for bank in [0, SUPERVISOR] {
            for mode in [2, 3] {
                let (mut cpu, mut bus, address, _) =
                    movem_seed((true, mode, 7), long, 0, bank, 0x1810);
                let initial = cpu.registers;
                cpu.step_instruction(&mut bus).unwrap();
                let events = transfers(&bus);
                assert_eq!(events.len(), 3);
                assert_eq!(events[1].0.logical_address, address);
                assert_eq!(events[1].0.kind, AccessKind::Discard);
                assert_eq!(events[1].0.lanes, ByteLanes::Both);
                assert_eq!(cpu.registers.stack_pointer(), initial.stack_pointer());
                let (mut cpu, mut bus, _, _) = movem_seed((true, mode, 7), long, 0, bank, 0x1810);
                cpu.registers.set_address_register(7, 0x3001);
                let initial = cpu.registers;
                assert_eq!(
                    cpu.step_instruction(&mut bus),
                    Err(TimingFault::AddressError { address: 0x3001 })
                );
                assert_eq!(cpu.registers, initial);
                assert_eq!(bus.transfers, 1);
                frozen(&mut cpu, &mut bus);
            }
            for mode in [2, 4] {
                let (mut cpu, mut bus, _, _) = movem_seed((false, mode, 7), long, 0, bank, 0x1810);
                cpu.registers.set_address_register(7, 0x3001);
                cpu.step_instruction(&mut bus).unwrap();
                assert_eq!(bus.transfers, 2);
                assert_eq!(cpu.registers.stack_pointer(), 0x3001);
                assert!(
                    transfers(&bus)
                        .iter()
                        .all(|(access, _)| access.kind == AccessKind::Prefetch)
                );
            }
        }
    }
}

#[test]
fn postincrement_base_override_waits_for_discard_and_predecrement_base_for_all_writes() {
    for bank in [0, SUPERVISOR] {
        for long in [false, true] {
            let width = if long { 4 } else { 2 };
            let (mut cpu, mut bus, address, _) =
                movem_seed((true, 3, 7), long, 0x8000, bank | TRACE, 0x1810);
            let words = if long { 2 } else { 1 };
            bus.fail = Some(words + 1);
            assert!(
                matches!(cpu.step_instruction(&mut bus), Err(TimingFault::Bus { access, .. }) if access.kind == AccessKind::Discard)
            );
            let high = bus.words[&(address & BUS_ADDRESS_MASK)];
            let loaded = if long {
                u32::from(high) << 16 | u32::from(bus.words[&((address + 2) & BUS_ADDRESS_MASK)])
            } else {
                i32::from(high as i16) as u32
            };
            assert_eq!(cpu.registers.stack_pointer(), loaded);
            let (mut cpu, mut bus, address, _) =
                movem_seed((true, 3, 7), long, 0x8000, bank, 0x1810);
            bus.fail = Some(words + 2);
            assert!(
                matches!(cpu.step_instruction(&mut bus), Err(TimingFault::Bus { access, .. }) if access.kind == AccessKind::Prefetch)
            );
            assert_eq!(cpu.registers.stack_pointer(), address + width);
            let (mut cpu, mut bus, address, _) = movem_seed((false, 4, 7), long, 1, bank, 0x1810);
            let initial = cpu.registers.stack_pointer();
            bus.fail = Some(1);
            assert!(matches!(
                cpu.step_instruction(&mut bus),
                Err(TimingFault::Bus { .. })
            ));
            assert_eq!(cpu.registers.stack_pointer(), initial);
            let (mut cpu, mut bus, _, _) = movem_seed((false, 4, 7), long, 1, bank, 0x1810);
            bus.fail = Some(words + 1);
            assert!(matches!(
                cpu.step_instruction(&mut bus),
                Err(TimingFault::Bus { .. })
            ));
            assert_eq!(cpu.registers.stack_pointer(), address - width);
        }
    }
}

#[test]
fn long_load_host_errors_leave_no_partial_register_even_after_high_input_word_or_bytes() {
    for bank in [0, SUPERVISOR] {
        let (mut cpu, mut bus, address, _) = movem_seed((true, 2, 0), true, 1, bank, 0x1810);
        let old = cpu.registers.d[0];
        bus.fail = Some(2);
        assert!(matches!(
            cpu.step_instruction(&mut bus),
            Err(TimingFault::Bus { .. })
        ));
        assert_eq!(cpu.registers.d[0], old);
        assert_eq!(transfers(&bus)[1].0.logical_address, address);
        for fail in [2, 3, 4] {
            let (mut cpu, mut bus, _) = movep_seed(0x0148, bank, 0, 0);
            let old = cpu.registers.d[0];
            bus.fail = Some(fail);
            assert!(matches!(
                cpu.step_instruction(&mut bus),
                Err(TimingFault::Bus { .. })
            ));
            assert_eq!(cpu.registers.d[0], old);
            frozen(&mut cpu, &mut bus);
        }
    }
}

#[test]
fn malformed_movem_modes_reject_before_mask_callbacks_and_do_not_steal_ext() {
    for load in [false, true] {
        for long in [false, true] {
            for bank in [0, SUPERVISOR] {
                for (mode, register) in [
                    (1, 0),
                    (1, 7),
                    (if load { 4 } else { 3 }, 7),
                    (7, 4),
                    (7, 5),
                    (7, 6),
                    (7, 7),
                ]
                .into_iter()
                .chain(if load { vec![] } else { vec![(7, 2), (7, 3)] })
                {
                    let opcode = opcode((load, mode, register), long);
                    let mut cpu = crate::timing::test_bus::cpu(bank | TRACE);
                    cpu.pipeline.as_mut().unwrap().words = [opcode, 0xffff];
                    let initial = cpu.clone();
                    let mut bus = Bus::default();
                    assert_eq!(
                        cpu.step_instruction(&mut bus),
                        Err(TimingFault::UnsupportedInstruction {
                            pc: cpu.registers.pc,
                            opcode
                        })
                    );
                    assert_eq!(cpu, initial);
                    assert!(bus.events.is_empty());
                }
            }
        }
    }
    for opcode in [0x4880, 0x48c0] {
        let mut cpu = crate::timing::test_bus::cpu(SUPERVISOR);
        cpu.pipeline.as_mut().unwrap().words = [opcode, 0x7201];
        cpu.registers.d[0] = 0x1234_80ff;
        let mut bus = Bus::default();
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(
            cpu.registers.d[0],
            if opcode == 0x4880 {
                0x1234_ffff
            } else {
                0xffff_80ff
            }
        );
        assert_eq!(bus.transfers, 1);
    }
}

#[test]
fn odd_nonempty_data_accesses_freeze_after_extensions_without_premature_base_commit() {
    for load in [false, true] {
        for long in [false, true] {
            for bank in [0, SUPERVISOR] {
                let mode = if load { 3 } else { 4 };
                let (mut cpu, mut bus, _, _) =
                    movem_seed((load, mode, 7), long, 1, bank | TRACE, 0x1810);
                cpu.registers.set_address_register(7, 0x3001);
                let initial = cpu.registers;
                let address = if load { 0x3001 } else { 0x2fff };
                assert_eq!(
                    cpu.step_instruction(&mut bus),
                    Err(TimingFault::AddressError { address })
                );
                assert_eq!(cpu.registers, initial);
                assert_eq!(bus.transfers, 1);
                assert!(
                    !bus.events
                        .iter()
                        .any(|event| matches!(event, Event::Idle(..)))
                );
                frozen(&mut cpu, &mut bus);
            }
        }
    }
}
