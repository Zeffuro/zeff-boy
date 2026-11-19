use super::*;
use zeff_m68k::timing::physical::{
    EntryInputError, EntryKind, ExternalResetBoundary, ResetQualification,
};

fn boundary(clock: u64, qualified: bool) -> ExternalResetBoundary {
    ExternalResetBoundary {
        sampled_release: CpuClocks(clock),
        qualification: if qualified {
            ResetQualification::QualifiedSafeAssertionAndRelease
        } else {
            ResetQualification::Unqualified
        },
    }
}
fn schedule(start: u64) -> Vec<Event> {
    let mut events = vec![Event::Idle(start, 14)];
    let mut clock = start + 14;
    for (i, address) in [0, 2, 4, 6, 0xabcd_4000, 0xabcd_4002]
        .into_iter()
        .enumerate()
    {
        if i == 5 {
            events.push(Event::Idle(clock, 2));
            clock += 2;
        }
        events.push(Event::EntryRead(
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
    events
}
fn check(case: Case, state: u8, failure: Option<(usize, Failure)>) -> bool {
    let (mut cpu, mut bus) = case.seed();
    let memory = bus.words.clone();
    let plan = oracle::plan(case, &memory);
    let t = *plan.last().unwrap();
    let elapsed = oracle::clocks(case, &bus.elapsed, plan.len(), true);
    bus.failure = Some((
        plan.len() - 1,
        if state == 0 {
            Failure::Host
        } else if state == 1 {
            Failure::Unavailable
        } else {
            Failure::Boundary
        },
    ));
    if state >= 2 {
        bus.entry_failure = Some((
            0,
            if state == 2 {
                Failure::Boundary
            } else {
                Failure::Unavailable
            },
        ));
    }
    let result = cpu.step_physical_instruction(&mut bus);
    let mut good = cpu.registers == t.registers && cpu.queued_words().is_none();
    if state == 0 {
        good &= result
            == Err(InstructionFault::Entry(EntryFault::HostFailure {
                access: t.access,
                direction: Direction::Read,
                source: "host",
            }));
    } else if state == 1 {
        good &= result
            == Err(InstructionFault::TransitionTimingUnavailable {
                phase: t.phase,
                cause: CriticalFault::BusAbort {
                    access: t.access,
                    direction: Direction::Read,
                },
            });
    } else {
        let p = entry::predict(
            case,
            t,
            2,
            case.start + elapsed,
            memory,
            (bus.entry_failure, None),
        );
        good &= result == p.result && bus.words == p.words && cpu.physical_entry_state() == p.state;
    }
    let before = cpu.clone();
    let device = bus.clone();
    let words = bus.words.clone();
    let events = bus.events.clone();
    let counts = (bus.count, bus.entry_count);
    let clock = cpu.clock().0;
    let unqualified = cpu.external_warm_reset(&mut bus, boundary(clock, false));
    good &= unqualified
        == Err(if state == 0 {
            EntryFault::HostFrozen
        } else {
            EntryFault::Input(EntryInputError::UnqualifiedReset)
        })
        && cpu == before
        && bus == device
        && bus.words == words
        && bus.events == events
        && bus.clock == clock
        && (bus.count, bus.entry_count) == counts;
    if state == 0 {
        return good
            && cpu.external_warm_reset(&mut bus, boundary(clock, true))
                == Err(EntryFault::HostFrozen)
            && cpu == before
            && bus == device
            && bus.words == words
            && bus.events == events
            && bus.clock == clock
            && (bus.count, bus.entry_count) == counts;
    }
    let start = clock + 17;
    bus.clock = start;
    bus.events.clear();
    bus.entry_count = 0;
    bus.entry_failure = failure;
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
    let words = bus.words.clone();
    let events = schedule(start);
    let result = cpu.external_warm_reset(&mut bus, boundary(start, true));
    if let Some((at, kind)) = failure {
        let end = events
            .iter()
            .enumerate()
            .filter(|(_, e)| matches!(e, Event::EntryRead(..)))
            .nth(at)
            .unwrap()
            .0;
        let access = if let Event::EntryRead(a, _) = events[end] {
            a
        } else {
            unreachable!()
        };
        let cause = CriticalFault::BusAbort {
            access,
            direction: Direction::Read,
        };
        let expected = match kind {
            Failure::Host => EntryFault::HostFailure {
                access,
                direction: Direction::Read,
                source: "host",
            },
            Failure::Boundary => EntryFault::DoubleFaultHalted(cause),
            _ => EntryFault::TimingUnavailable(cause),
        };
        let state = if kind == Failure::Host {
            PhysicalEntryState::Critical(EntryKind::ExternalWarmReset)
        } else {
            PhysicalEntryState::DoubleFaultHalted {
                cause,
                timing: if kind == Failure::Boundary {
                    TimingQualification::Qualified
                } else {
                    TimingQualification::Unavailable
                },
            }
        };
        let clocks = start + 14 + (at as u64 + 1) * 4 + if at == 5 { 2 } else { 0 };
        good &= result == Err(expected)
            && cpu.registers == t.registers
            && cpu.queued_words().is_none()
            && bus.events == events[..=end]
            && bus.entry_count == at + 1
            && cpu.physical_entry_state() == state
            && cpu.physical_inspection() == PhysicalInspection::Unavailable
            && cpu.clock().0 == clocks
            && bus.clock == clocks;
        good &= run::frozen(
            &mut cpu,
            &mut bus,
            if kind == Failure::Host {
                TimingFault::Frozen
            } else {
                TimingFault::DoubleFaultHalted
            },
        );
    } else {
        let mut expected = t.registers;
        expected.ssp = 0x1020_3001;
        expected.pc = 0xabcd_4000;
        expected.set_status_register(0x2700 | t.registers.status_register() & 31);
        good &= result.is_ok_and(|step| step.clocks == 40)
            && cpu.registers == expected
            && cpu.queued_words() == Some([0x7201, 0x4e71])
            && bus.events == events
            && bus.entry_count == 6
            && cpu.clock().0 == start + 40
            && bus.clock == start + 40
            && cpu.physical_inspection() == PhysicalInspection::DiagnosticCompleteValues;
    }
    good && bus.words == words
}

pub(super) fn witnesses() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for size in [Size::Word, Size::Long] {
        for supervisor in [false, true] {
            let mut c = Case::new(Form { size, ea: Ea::Post });
            c.r = registers(supervisor, 31);
            c.register = 7;
            c.value = if size == Size::Long {
                0x80000001
            } else {
                0x8001
            };
            count += 1;
            bad += u32::from(!check(c, 0, None));
            for state in 1..=3 {
                for at in 0..6 {
                    for kind in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                        count += 1;
                        bad += u32::from(!check(c, state, Some((at, kind))));
                    }
                }
                count += 1;
                bad += u32::from(!check(c, state, None));
            }
        }
    }
    (count, bad + u32::from(count != 232))
}
