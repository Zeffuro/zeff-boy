use super::*;
use zeff_m68k::timing::physical::{EntryKind, ExternalResetBoundary, ResetQualification};

fn terminal_state(
    cpu: &mut PrefetchCpu,
    bus: &mut BusDevice,
    cause: CriticalFault,
    failure: Failure,
    kind: EntryKind,
) {
    if failure == Failure::Host {
        assert_eq!(
            cpu.physical_entry_state(),
            PhysicalEntryState::Critical(kind)
        );
        assert_eq!(
            cpu.physical_inspection(),
            if kind == EntryKind::ExternalWarmReset {
                PhysicalInspection::Unavailable
            } else {
                PhysicalInspection::DiagnosticCompleteValues
            }
        );
        guarded(cpu, bus, TimingFault::Frozen);
    } else {
        assert_eq!(
            cpu.physical_entry_state(),
            PhysicalEntryState::DoubleFaultHalted {
                cause,
                timing: if failure == Failure::Boundary {
                    TimingQualification::Qualified
                } else {
                    TimingQualification::Unavailable
                },
            }
        );
        assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
        guarded(cpu, bus, TimingFault::DoubleFaultHalted);
    }
}

#[test]
fn all_11_secondary_callbacks_have_exact_cause_direction_timing_and_consumed_memory() {
    let mut rows = 0;
    for form in forms() {
        for supervisor in [false, true] {
            for at in 0..11 {
                for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                    let mut case = Case::new(form);
                    case.r = registers(supervisor, 31);
                    case.register = 7;
                    let (mut cpu, mut bus) = case.seed();
                    let initial = bus.words.clone();
                    let plan = oracle::plan(case, &initial);
                    let last = plan.len() - 1;
                    let t = plan[last];
                    let c = oracle::capture(t, 2);
                    bus.failure = Some((last, Failure::Boundary));
                    bus.entry_failure = Some((at, failure));
                    let start = case.start + oracle::clocks(case, &bus.elapsed, plan.len(), true);
                    let schedule = oracle::entry_events(t.registers.ssp, c, start);
                    let end = schedule
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| matches!(e, Event::EntryRead(..) | Event::EntryWrite(..)))
                        .nth(at)
                        .unwrap()
                        .0;
                    let mut memory = initial;
                    for event in &schedule[..=end] {
                        if let Event::EntryWrite(a, v, _) = event {
                            memory.insert(a.bus_address, *v);
                        }
                    }
                    let (access, direction) = match schedule[end] {
                        Event::EntryRead(a, _) => (a, Direction::Read),
                        Event::EntryWrite(a, _, _) => (a, Direction::Write),
                        _ => unreachable!(),
                    };
                    let cause = CriticalFault::BusAbort { access, direction };
                    let fault = match failure {
                        Failure::Host => EntryFault::HostFailure {
                            access,
                            direction,
                            source: "host",
                        },
                        Failure::Boundary => EntryFault::DoubleFaultHalted(cause),
                        Failure::Unavailable => EntryFault::TimingUnavailable(cause),
                        Failure::Complete => unreachable!(),
                    };
                    assert_eq!(
                        cpu.step_physical_instruction(&mut bus),
                        Err(InstructionFault::Entry(fault))
                    );
                    assert_eq!(cpu.registers, t.registers);
                    assert_eq!(cpu.queued_words(), None);
                    assert_eq!(bus.words, memory);
                    let mut events = oracle::events(case, &plan, &bus.elapsed, true);
                    events.extend_from_slice(&schedule[..=end]);
                    assert_eq!(bus.events, events);
                    assert_eq!(bus.count, plan.len());
                    assert_eq!(bus.entry_count, at + 1);
                    assert_eq!(
                        cpu.clock().0,
                        start + 4 + (at as u64 + 1) * 4 + if at == 10 { 2 } else { 0 }
                    );
                    assert_eq!(cpu.clock().0, bus.clock);
                    terminal_state(
                        &mut cpu,
                        &mut bus,
                        cause,
                        failure,
                        EntryKind::ImportedGroup0,
                    );
                    rows += 1;
                }
            }
        }
    }
    assert_eq!(rows, 2376);
}

fn reset_boundary(clock: u64, qualified: bool) -> ExternalResetBoundary {
    ExternalResetBoundary {
        sampled_release: CpuClocks(clock),
        qualification: if qualified {
            ResetQualification::QualifiedSafeAssertionAndRelease
        } else {
            ResetQualification::Unqualified
        },
    }
}

fn reset_schedule(start: u64) -> Vec<Event> {
    let mut result = vec![Event::Idle(start, 14)];
    let mut clock = start + 14;
    for (i, address) in [0, 2, 4, 6, 0xabcd_4000, 0xabcd_4002]
        .into_iter()
        .enumerate()
    {
        if i == 5 {
            result.push(Event::Idle(clock, 2));
            clock += 2;
        }
        result.push(Event::EntryRead(
            oracle::access(
                address,
                true,
                true,
                if i < 4 {
                    AccessKind::ResetVector
                } else {
                    AccessKind::Prefetch
                },
            ),
            clock,
        ));
        clock += 4;
    }
    result
}

#[test]
fn qualified_reset_after_address_arithmetic_doublehalt_has_all_six_exact_terminal_read_causes() {
    for (operation, supervisor) in
        [Operation::Add, Operation::Subtract]
            .into_iter()
            .flat_map(|operation| {
                [false, true]
                    .into_iter()
                    .map(move |supervisor| (operation, supervisor))
            })
    {
        for at in 0..6 {
            for failure in [
                Failure::Host,
                Failure::Boundary,
                Failure::Unavailable,
                Failure::Complete,
            ] {
                let mut case = Case::new(Form {
                    operation,
                    size: Size::Long,
                    ea: Ea::Post,
                });
                case.r = registers(supervisor, 31);
                case.register = 7;
                let (mut cpu, mut bus) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                let diagnostic = plan.last().unwrap().registers;
                bus.failure = Some((2, Failure::Boundary));
                bus.entry_failure = Some((0, Failure::Boundary));
                assert!(matches!(
                    cpu.step_physical_instruction(&mut bus),
                    Err(InstructionFault::Entry(EntryFault::DoubleFaultHalted(_)))
                ));
                let before = cpu.clone();
                let events = bus.events.clone();
                assert!(
                    cpu.external_warm_reset(&mut bus, reset_boundary(before.clock().0, false))
                        .is_err()
                );
                assert_eq!(cpu, before);
                assert_eq!(bus.events, events);
                let release = cpu.clock().0 + 17;
                bus.clock = release;
                bus.events.clear();
                bus.entry_count = 0;
                bus.entry_failure = Some((at, failure));
                for (address, value) in [
                    (0, 0x1020),
                    (2, 0x3001),
                    (4, 0xabcd),
                    (6, 0x4000),
                    (0xabcd_4000, 0x7201),
                    (0xabcd_4002, 0x4e71),
                ] {
                    bus.word(address, value);
                }
                let memory = bus.words.clone();
                let schedule = reset_schedule(release);
                let result = cpu.external_warm_reset(&mut bus, reset_boundary(release, true));
                if failure == Failure::Complete {
                    assert_eq!(result.unwrap().clocks, 40);
                    let mut r = diagnostic;
                    r.ssp = 0x1020_3001;
                    r.pc = 0xabcd_4000;
                    r.set_status_register(0x2700 | diagnostic.status_register() & 31);
                    assert_eq!(cpu.registers, r);
                    assert_eq!(cpu.queued_words(), Some([0x7201, 0x4e71]));
                    assert_eq!(bus.events, schedule);
                    assert_eq!(bus.entry_count, 6);
                    assert_eq!(cpu.clock().0, release + 40);
                } else {
                    let end = schedule
                        .iter()
                        .enumerate()
                        .filter(|(_, e)| matches!(e, Event::EntryRead(..)))
                        .nth(at)
                        .unwrap()
                        .0;
                    let access = match schedule[end] {
                        Event::EntryRead(a, _) => a,
                        _ => unreachable!(),
                    };
                    let cause = CriticalFault::BusAbort {
                        access,
                        direction: Direction::Read,
                    };
                    assert_eq!(
                        result,
                        Err(match failure {
                            Failure::Host => EntryFault::HostFailure {
                                access,
                                direction: Direction::Read,
                                source: "host"
                            },
                            Failure::Boundary => EntryFault::DoubleFaultHalted(cause),
                            Failure::Unavailable => EntryFault::TimingUnavailable(cause),
                            Failure::Complete => unreachable!(),
                        })
                    );
                    assert_eq!(cpu.registers, diagnostic);
                    assert_eq!(cpu.queued_words(), None);
                    assert_eq!(bus.events, schedule[..=end]);
                    assert_eq!(bus.entry_count, at + 1);
                    assert_eq!(
                        cpu.clock().0,
                        release + 14 + (at as u64 + 1) * 4 + if at == 5 { 2 } else { 0 }
                    );
                    terminal_state(
                        &mut cpu,
                        &mut bus,
                        cause,
                        failure,
                        EntryKind::ExternalWarmReset,
                    );
                }
                assert_eq!(bus.words, memory);
                assert_eq!(cpu.clock().0, bus.clock);
            }
        }
    }
}

#[test]
fn host_frozen_address_arithmetic_secondary_entry_rejects_even_qualified_reset_without_side_effects()
 {
    let case = Case::new(Form {
        operation: Operation::Add,
        size: Size::Word,
        ea: Ea::Pre,
    });
    let (mut cpu, mut bus) = case.seed();
    bus.failure = Some((0, Failure::Boundary));
    bus.entry_failure = Some((0, Failure::Host));
    assert!(matches!(
        cpu.step_physical_instruction(&mut bus),
        Err(InstructionFault::Entry(EntryFault::HostFailure { .. }))
    ));
    let before = cpu.clone();
    let memory = bus.words.clone();
    let events = bus.events.clone();
    assert_eq!(
        cpu.external_warm_reset(&mut bus, reset_boundary(before.clock().0, true)),
        Err(EntryFault::HostFrozen)
    );
    assert_eq!(cpu, before);
    assert_eq!(bus.words, memory);
    assert_eq!(bus.events, events);
    assert_eq!(bus.clock, before.clock().0);
}
