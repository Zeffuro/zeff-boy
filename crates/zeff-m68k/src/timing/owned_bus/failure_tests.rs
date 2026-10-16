use super::spy::*;
use super::*;
use crate::SUPERVISOR;

fn frozen(cpu: &mut PrefetchCpu, spy: &mut Spy) {
    assert_eq!(spy.owner, Owner::None);
    assert!(!cpu.trace_pending());
    let state = cpu.clone();
    let events = spy.events.clone();
    assert_eq!(cpu.step_instruction(spy), Err(TimingFault::Frozen));
    assert_eq!(cpu.step_boundary(spy), Err(TimingFault::Frozen));
    assert_eq!(cpu.service_pending_trace(spy), Err(TimingFault::Frozen));
    assert_eq!(*cpu, state);
    assert_eq!(spy.events, events);
    assert_eq!(cpu.clock().0, spy.bus.clock);
}

#[test]
fn actual_default_hooks_fail_closed_without_emulated_pulse_or_unlocked_rmw_fallback() {
    let (mut cpu, spy) = reset_seed(SUPERVISOR | TRACE);
    let original = cpu.registers;
    let mut bus = spy.bus;
    assert_eq!(
        cpu.step_instruction(&mut bus),
        Err(TimingFault::ResetOutputUnsupported)
    );
    assert_eq!(cpu.registers, original);
    assert_eq!(cpu.clock().0, 4);
    assert_eq!(bus.transfers, 0);
    for shape in shapes() {
        let (mut cpu, spy, _, _) = seed(shape, SUPERVISOR | TRACE, 0x80, 0, 0x1010);
        let mut bus = spy.bus;
        let old = bus.words.clone();
        assert!(matches!(
            cpu.step_instruction(&mut bus),
            Err(TimingFault::RmwOwnershipUnsupported { .. })
        ));
        assert_eq!(bus.words, old);
        assert!(bus.events.iter().all(|event| !matches!(event, crate::timing::test_bus::Event::Read(access, _) if access.kind == AccessKind::Data)));
        let count = bus.events.len();
        assert_eq!(cpu.step_instruction(&mut bus), Err(TimingFault::Frozen));
        assert_eq!(bus.events.len(), count);
    }
}

#[test]
fn begin_unsupported_and_error_leave_unowned_with_no_cpu_end_or_retry() {
    for begin in [Begin::Unsupported, Begin::Error] {
        let (mut cpu, mut spy) = reset_seed(SUPERVISOR | TRACE | 0x1f);
        let original = cpu.registers;
        spy.reset = begin;
        let fault = cpu.step_instruction(&mut spy);
        assert_eq!(
            fault,
            Err(if begin == Begin::Unsupported {
                TimingFault::ResetOutputUnsupported
            } else {
                TimingFault::ResetOutput { source: 900 }
            })
        );
        assert_eq!(cpu.registers, original);
        assert_eq!(cpu.clock().0, 4);
        assert_eq!(spy.bus.transfers, 0);
        assert!(
            !spy.events
                .iter()
                .any(|event| matches!(event, Event::EndReset(..)))
        );
        assert_eq!(
            spy.events
                .iter()
                .filter(|event| matches!(event, Event::Cleanup(Owner::Reset, 4)))
                .count(),
            usize::from(begin == Begin::Error)
        );
        frozen(&mut cpu, &mut spy);
        for shape in shapes() {
            for bank in [0, SUPERVISOR] {
                let (mut cpu, mut spy, _, _) = seed(shape, bank | TRACE | 0x1f, 0x80, 1, 0xf810);
                let mut expected = cpu.registers;
                if matches!(shape.0, 3 | 4) {
                    let step = if shape.1 == 7 { 2 } else { 1 };
                    let base = expected.address_register(shape.1).unwrap();
                    expected.set_address_register(
                        shape.1,
                        if shape.0 == 3 {
                            base + step
                        } else {
                            base - step
                        },
                    );
                }
                spy.rmw = begin;
                let fault = cpu.step_instruction(&mut spy);
                assert!(
                    matches!(fault, Err(TimingFault::RmwOwnershipUnsupported { .. }))
                        && begin == Begin::Unsupported
                        || matches!(fault, Err(TimingFault::RmwOwnership { source: 901, .. }))
                            && begin == Begin::Error
                );
                assert_eq!(cpu.registers, expected);
                assert!(!spy.events.iter().any(|event| matches!(
                    event,
                    Event::EndRmw(..) | Event::Read(_, _, Owner::Rmw) | Event::Write(..)
                )));
                assert_eq!(
                    spy.events
                        .iter()
                        .filter(|event| matches!(event, Event::Cleanup(Owner::Rmw, _)))
                        .count(),
                    usize::from(begin == Begin::Error)
                );
                frozen(&mut cpu, &mut spy);
            }
        }
    }
}

#[test]
fn every_tas_transfer_failure_releases_and_retains_only_completed_ea_flags_and_writes() {
    for shape in shapes() {
        for bank in [0, SUPERVISOR] {
            for parity in [0, 1] {
                for old in [0, 0x80] {
                    for ignore in [false, true] {
                        let (mut complete, mut full, _, _) =
                            seed(shape, bank | TRACE | 0x1f, old, parity, 0xf810);
                        full.bus.wait = 3;
                        full.ignore_write = ignore;
                        complete.step_instruction(&mut full).unwrap();
                        let read = full.bus.events.iter().filter(|event| !matches!(event, crate::timing::test_bus::Event::Idle(..))).position(|event| matches!(event, crate::timing::test_bus::Event::Read(access, _) if access.kind == AccessKind::Data)).unwrap();
                        for fail in 0..full.bus.transfers {
                            let (mut cpu, mut spy, address, _) =
                                seed(shape, bank | TRACE | 0x1f, old, parity, 0xf810);
                            let initial = cpu.registers;
                            let queue = cpu.queued_words();
                            let mut expected = initial;
                            if matches!(shape.0, 3 | 4) {
                                let base = initial.address_register(shape.1).unwrap();
                                let step = if shape.1 == 7 { 2 } else { 1 };
                                expected.set_address_register(
                                    shape.1,
                                    if shape.0 == 3 {
                                        base + step
                                    } else {
                                        base - step
                                    },
                                );
                            }
                            if fail > read {
                                expected.set_status_register(flags(initial.status_register(), old));
                            }
                            let mut memory = spy.bus.words.clone();
                            if !ignore && fail > read + 1 {
                                memory.insert(
                                    address & BUS_ADDRESS_MASK & !1,
                                    full.bus.words[&(address & BUS_ADDRESS_MASK & !1)],
                                );
                            }
                            spy.bus.wait = 3;
                            spy.bus.fail = Some(fail);
                            spy.ignore_write = ignore;
                            assert!(
                                matches!(cpu.step_instruction(&mut spy), Err(TimingFault::Bus { source, .. }) if source == fail)
                            );
                            assert_eq!(cpu.registers, expected, "{shape:?}/{fail}");
                            assert_eq!(spy.bus.words, memory);
                            assert_eq!(cpu.queued_words(), queue);
                            assert!(!cpu.trace_pending());
                            let acquired = spy
                                .events
                                .iter()
                                .any(|event| matches!(event, Event::BeginRmw(..)));
                            assert_eq!(
                                spy.events
                                    .iter()
                                    .filter(|event| matches!(event, Event::EndRmw(..)))
                                    .count(),
                                usize::from(acquired)
                            );
                            assert_eq!(spy.bus.transfers, fail + 1);
                            frozen(&mut cpu, &mut spy);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn reset_tail_fault_retains_completed_pulse_and_privilege_frame_errors_never_acquire() {
    let (mut cpu, mut spy) = reset_seed(SUPERVISOR | TRACE | 0x1f);
    let initial = cpu.registers;
    let queue = cpu.queued_words();
    spy.bus.wait = 3;
    spy.bus.fail = Some(0);
    spy.pulse_writes.push((0x340104, 0xcafe));
    assert!(matches!(
        cpu.step_instruction(&mut spy),
        Err(TimingFault::Bus { source: 0, .. })
    ));
    assert_eq!(cpu.registers, initial);
    assert_eq!(cpu.queued_words(), queue);
    assert_eq!(spy.bus.words[&0x340104], 0xcafe);
    assert_eq!(cpu.clock().0, 135);
    assert_eq!(spy.events[3], Event::EndReset(128));
    assert!(matches!(spy.events[4], Event::Read(_, 128, Owner::None)));
    frozen(&mut cpu, &mut spy);
    for fail in 0..7 {
        let (mut cpu, mut spy) = reset_seed(TRACE | 0x1f);
        let initial = cpu.registers;
        spy.bus.fail = Some(fail);
        assert!(
            matches!(cpu.step_instruction(&mut spy), Err(TimingFault::Bus { source, .. }) if source == fail)
        );
        assert_eq!(cpu.registers, initial);
        assert!(
            !spy.events
                .iter()
                .any(|event| matches!(event, Event::BeginReset(..) | Event::EndReset(..)))
        );
        frozen(&mut cpu, &mut spy);
    }
}

#[test]
fn pulse_clock_capacity_is_checked_before_acquisition_and_release_precedes_tail_overflow() {
    for remaining in [1, 127, 128] {
        let (mut cpu, mut spy) = reset_seed(SUPERVISOR | TRACE);
        let original = cpu.registers;
        cpu.clock = CpuClocks(u64::MAX - remaining);
        spy.bus.clock = cpu.clock.0;
        assert_eq!(
            cpu.step_instruction(&mut spy),
            Err(TimingFault::ClockOverflow)
        );
        assert_eq!(cpu.registers, original);
        assert_eq!(spy.bus.transfers, 0);
        if remaining == 128 {
            assert_eq!(spy.events[1], Event::BeginReset(u64::MAX - 124));
            assert_eq!(spy.events[3], Event::EndReset(u64::MAX));
            assert_eq!(cpu.clock().0, u64::MAX);
        } else {
            assert!(
                !spy.events
                    .iter()
                    .any(|event| matches!(event, Event::BeginReset(..)))
            );
        }
        frozen(&mut cpu, &mut spy);
    }
}

#[test]
fn rmw_clock_overflow_releases_before_read_after_read_or_after_completed_write() {
    let unit = 4 + u64::from(u32::MAX);
    for (remaining, transfers, changed_flags, changed_memory) in [
        (10, 0, false, false),
        (unit, 1, true, false),
        (unit * 2 + 2, 2, true, true),
    ] {
        let (mut cpu, mut spy, address, _) = seed((2, 0), SUPERVISOR | TRACE | 0x1f, 0, 0, 0x1010);
        let initial = cpu.registers;
        let before = spy.bus.words[&(address & BUS_ADDRESS_MASK)];
        cpu.clock = CpuClocks(u64::MAX - remaining);
        spy.bus.clock = cpu.clock.0;
        spy.bus.wait = u32::MAX;
        assert_eq!(
            cpu.step_instruction(&mut spy),
            Err(TimingFault::ClockOverflow)
        );
        let mut expected = initial;
        if changed_flags {
            expected.set_status_register(flags(initial.status_register(), 0));
        }
        assert_eq!(cpu.registers, expected);
        assert_eq!(spy.bus.transfers, transfers);
        assert_eq!(
            spy.bus.words[&(address & BUS_ADDRESS_MASK)],
            if changed_memory { 0x8055 } else { before }
        );
        assert_eq!(
            spy.events
                .iter()
                .filter(|event| matches!(event, Event::EndRmw(..)))
                .count(),
            1
        );
        assert!(matches!(spy.events.last(), Some(Event::EndRmw(time)) if *time == cpu.clock().0));
        frozen(&mut cpu, &mut spy);
    }
    let (mut cpu, mut spy, _, _) = seed((4, 7), SUPERVISOR | TRACE, 0, 0, 0x1010);
    let initial = cpu.registers;
    cpu.clock = CpuClocks(u64::MAX - 1);
    spy.bus.clock = cpu.clock.0;
    assert_eq!(
        cpu.step_instruction(&mut spy),
        Err(TimingFault::ClockOverflow)
    );
    assert_eq!(cpu.registers, initial);
    assert!(spy.events.is_empty());
    frozen(&mut cpu, &mut spy);
}

#[test]
fn invalid_tas_is_precallback_and_accepted_illegal_encodings_keep_vector4_precedence() {
    for opcode in (0x4ac8..=0x4acf).chain(0x4afd..=0x4aff) {
        for bank in [0, SUPERVISOR] {
            let (mut cpu, mut spy) = reset_seed(bank | TRACE);
            cpu.pipeline.as_mut().unwrap().words[0] = opcode;
            let initial = cpu.clone();
            assert_eq!(
                cpu.step_instruction(&mut spy),
                Err(TimingFault::UnsupportedInstruction {
                    pc: cpu.registers.pc,
                    opcode
                })
            );
            assert_eq!(cpu, initial);
            assert!(spy.events.is_empty());
        }
    }
    for opcode in 0x4afa..=0x4afc {
        for bank in [0, SUPERVISOR] {
            let (mut cpu, mut spy) = reset_seed(bank | TRACE);
            cpu.pipeline.as_mut().unwrap().words[0] = opcode;
            assert_eq!(cpu.step_instruction(&mut spy).unwrap().exception, Some(4));
            assert!(!cpu.trace_pending());
            assert!(
                !spy.events
                    .iter()
                    .any(|event| matches!(event, Event::BeginRmw(..) | Event::EndRmw(..)))
            );
        }
    }
}

#[test]
fn tas_dn_tail_failure_retains_completed_byte_and_flags_and_never_requests_ownership() {
    for old in [0, 0x80] {
        let (mut cpu, mut spy) = reset_seed(SUPERVISOR | TRACE | 0x1f);
        cpu.pipeline.as_mut().unwrap().words[0] = 0x4ac7;
        cpu.registers.d[7] = 0x1234_5600 | old;
        let mut expected = cpu.registers;
        expected.d[7] |= 0x80;
        expected.set_status_register(flags(expected.status_register(), old as u8));
        spy.bus.fail = Some(0);
        assert!(matches!(
            cpu.step_instruction(&mut spy),
            Err(TimingFault::Bus { source: 0, .. })
        ));
        assert_eq!(cpu.registers, expected);
        assert_eq!(spy.events.len(), 1);
        frozen(&mut cpu, &mut spy);
    }
}
