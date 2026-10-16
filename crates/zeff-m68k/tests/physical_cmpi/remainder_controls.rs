use super::*;
use zeff_m68k::timing::physical::{
    EntryInputError, EntryKind, ExternalResetBoundary, ResetQualification,
};

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

fn numeric(c: Case) -> bool {
    let (_, bus) = c.seed();
    let plan = oracle::plan(c, &bus.words);
    let mut r = c.r;
    if matches!(c.form.ea, Ea::Post | Ea::Pre) {
        let step = if c.register == 7 { 2 } else { 1 };
        let old = r.address_register(usize::from(c.register)).unwrap();
        r.set_address_register(
            usize::from(c.register),
            if c.form.ea == Ea::Post {
                old.wrapping_add(step)
            } else {
                old.wrapping_sub(step)
            },
        );
    }
    r.set_status_register((c.r.status_register() & !15) | 11);
    let f = plan.last().unwrap();
    f.registers == r && f.ird == if c.form.tail() { c.opcode() } else { c.next }
}

fn reject_prefix(c: Case, blocked: usize, elapsed: [u32; 7]) -> bool {
    let (mut cpu, mut bus) = c.seed();
    bus.elapsed = elapsed;
    let words = bus.words.clone();
    let plan = oracle::plan(c, &words);
    let idle = c.form.idle() && blocked >= c.form.immediates();
    let clock = c.start
        + elapsed[..blocked]
            .iter()
            .map(|e| u64::from(*e))
            .sum::<u64>()
        + u64::from(idle) * 2;
    let mut events = oracle::events(c, &plan[..blocked], &elapsed, true);
    if idle && blocked == c.form.immediates() {
        events.push(Event::Idle(clock - 2, 2));
    }
    let r = if blocked == 0 {
        c.r
    } else {
        plan[blocked - 1].registers
    };
    let before = cpu.clone();
    let result = cpu.step_physical_instruction(&mut bus);
    result == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
        && cpu.registers == r
        && cpu.registers.status_register() == c.r.status_register()
        && cpu.registers.d == before.registers.d
        && cpu.queued_words() == Some([c.opcode(), c.retained()])
        && cpu.clock().0 == clock
        && bus.clock == clock
        && bus.events == events
        && bus.words == words
        && bus.count == blocked
        && bus.entry_count == 0
        && cpu.physical_entry_state() == PhysicalEntryState::Idle
        && run::frozen(&mut cpu, &mut bus, TimingFault::Frozen)
}

fn short_and_retained(total: &mut (u32, u32)) {
    for bank in [false, true] {
        for word in remainder::REPRESENTATIVES {
            let c = remainder::case(word, bank, 1);
            for at in 0..c.form.count() {
                total.0 += 1;
                total.1 +=
                    u32::from(!numeric(c) || !run::check(c, Some((at, Failure::Complete)), 0));
            }
        }
        for word in [0x0c00, 0x0c40, 0x0c80, 0x0c39] {
            let mut c = remainder::case(word, bank, 1);
            c.next = 0x4afc;
            total.0 += 1;
            total.1 += u32::from(
                !numeric(c) || !run::check(c, Some((c.form.count() - 1, Failure::Boundary)), 3),
            );
        }
    }
}

fn capacity(total: &mut (u32, u32)) {
    let limit = u64::from(u32::MAX);
    for bank in [false, true] {
        for word in [0x0c27, 0x0c37] {
            for room in [1, 2] {
                let mut c = remainder::case(word, bank, 1);
                c.start = u64::MAX - limit - room;
                let (mut cpu, mut bus) = c.seed();
                bus.elapsed[0] = u32::MAX;
                let words = bus.words.clone();
                let plan = oracle::plan(c, &words);
                let mut events = vec![Event::Read(plan[0].access, c.start)];
                let clock = if room == 2 {
                    events.push(Event::Idle(u64::MAX - 2, 2));
                    u64::MAX
                } else {
                    u64::MAX - 1
                };
                let result = cpu.step_physical_instruction(&mut bus);
                total.0 += 1;
                total.1 += u32::from(
                    !(result == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                        && cpu.registers == c.r
                        && cpu.queued_words() == Some([word, c.retained()])
                        && cpu.clock().0 == clock
                        && bus.clock == clock
                        && bus.words == words
                        && bus.events == events
                        && bus.count == 1
                        && bus.entry_count == 0
                        && cpu.physical_entry_state() == PhysicalEntryState::Idle
                        && run::frozen(&mut cpu, &mut bus, TimingFault::Frozen)),
                );
            }
        }
        for word in remainder::REPRESENTATIVES {
            let mut c = remainder::case(word, bank, 1);
            let blocked = c.form.count() - 1;
            let prefix = (blocked as u64 - 1) * 4 + u64::from(c.form.idle()) * 2;
            c.start = u64::MAX - prefix - limit - limit + 1;
            let mut elapsed = [4; 7];
            elapsed[blocked - 1] = u32::MAX;
            total.0 += 1;
            total.1 += u32::from(!reject_prefix(c, blocked, elapsed));
            for kind in [Failure::Host, Failure::Unavailable] {
                let mut c = remainder::case(word, bank, 1);
                let prefix = blocked as u64 * 4 + u64::from(c.form.idle()) * 2;
                c.start = u64::MAX - prefix - limit;
                let (cpu, mut bus) = c.seed();
                bus.elapsed[blocked] = u32::MAX;
                bus.failure = Some((blocked, kind));
                total.0 += 1;
                total.1 += u32::from(!numeric(c) || !run::seeded(c, cpu, bus));
            }
        }
        for word in [0x0c00, 0x0c40, 0x0c39] {
            let mut c = remainder::case(word, bank, 1);
            let final_index = c.form.count() - 1;
            c.start = u64::MAX - limit - final_index as u64 * 4;
            let (cpu, mut bus) = c.seed();
            bus.elapsed[final_index] = u32::MAX;
            total.0 += 1;
            total.1 += u32::from(!numeric(c) || !run::seeded(c, cpu, bus));
        }
    }
}

fn default_secondary(c: Case) -> bool {
    let (mut cpu, mut bus) = c.seed();
    let words = bus.words.clone();
    let plan = oracle::plan(c, &words);
    let at = plan.len() - 1;
    bus.failure = Some((at, Failure::Boundary));
    let start = c.start + oracle::clocks(c, &bus.elapsed, plan.len(), true);
    let t = plan[at];
    let p = entry::predict(c, t, 2, start, words, (None, None));
    let mut events = oracle::events(c, &plan, &bus.elapsed, true);
    events.extend(
        p.events
            .iter()
            .copied()
            .filter(|e| !matches!(e, Event::EntryDetect(..))),
    );
    let result = cpu.step_physical_instruction(&mut DefaultDetection(&mut bus));
    result == p.result
        && cpu.registers == p.registers
        && cpu.queued_words() == p.queue
        && cpu.clock().0 == p.end
        && cpu.physical_entry_state() == p.state
        && cpu.physical_inspection() == p.inspection
        && bus.words == p.words
        && bus.events == events
        && bus.clock == p.end
        && bus.count == plan.len()
        && bus.entry_count == 0
        && run::frozen(&mut cpu, &mut bus, TimingFault::DoubleFaultHalted)
}

fn reset_events(start: u64) -> Vec<Event> {
    let mut out = vec![Event::Idle(start, 14)];
    for (i, address) in [0, 2, 4, 6, 0xabcd_4000, 0xabcd_4002]
        .into_iter()
        .enumerate()
    {
        let clock = start + 14 + i as u64 * 4 + u64::from(i == 5) * 2;
        if i == 5 {
            out.push(Event::Idle(clock - 2, 2));
        }
        out.push(Event::EntryRead(
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
    }
    out
}

fn reset(c: Case, state: u8) -> bool {
    let (mut cpu, mut bus) = c.seed();
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
    let plan = oracle::plan(c, &words);
    let at = plan.len() - 1;
    let t = plan[at];
    let kind = if state == 0 {
        Failure::Host
    } else if state == 1 {
        Failure::Unavailable
    } else {
        Failure::Boundary
    };
    bus.failure = Some((at, kind));
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
    let start = c.start + oracle::clocks(c, &bus.elapsed, plan.len(), true);
    let result = cpu.step_physical_instruction(&mut bus);
    let cause = CriticalFault::BusAbort {
        access: t.access,
        direction: Direction::Read,
    };
    let mut events = oracle::events(c, &plan, &bus.elapsed, true);
    let mut expected_words = words;
    let mut expected_clock = start;
    let good = if state >= 2 {
        let p = entry::predict(c, t, 2, start, expected_words, (bus.entry_failure, None));
        events.extend(p.events);
        expected_words = p.words;
        expected_clock = p.end;
        result == p.result
            && cpu.physical_entry_state() == p.state
            && cpu.physical_inspection() == p.inspection
            && bus.entry_count == 1
    } else {
        result
            == Err(if state == 0 {
                InstructionFault::Entry(EntryFault::HostFailure {
                    access: t.access,
                    direction: Direction::Read,
                    source: "host",
                })
            } else {
                InstructionFault::TransitionTimingUnavailable {
                    phase: t.phase,
                    cause,
                }
            })
            && bus.entry_count == 0
            && cpu.physical_entry_state()
                == if state == 0 {
                    PhysicalEntryState::Idle
                } else {
                    PhysicalEntryState::FaultUnqualified { cause }
                }
            && cpu.physical_inspection()
                == if state == 0 {
                    PhysicalInspection::DiagnosticCompleteValues
                } else {
                    PhysicalInspection::Unavailable
                }
    };
    let mut good = good
        && cpu.registers == t.registers
        && cpu.queued_words().is_none()
        && cpu.clock().0 == expected_clock
        && bus.clock == expected_clock
        && bus.words == expected_words
        && bus.events == events
        && bus.count == plan.len();
    if !good {
        return false;
    }
    good &= run::frozen(
        &mut cpu,
        &mut bus,
        if state == 0 {
            TimingFault::Frozen
        } else if state == 1 {
            TimingFault::PhysicalFaultUnqualified
        } else {
            TimingFault::DoubleFaultHalted
        },
    );
    let before = cpu.clone();
    let device = bus.clone();
    for (release, qualified, expected) in [
        (
            expected_clock,
            false,
            if state == 0 {
                EntryFault::HostFrozen
            } else {
                EntryFault::Input(EntryInputError::UnqualifiedReset)
            },
        ),
        (
            expected_clock - 1,
            true,
            if state == 0 {
                EntryFault::HostFrozen
            } else {
                EntryFault::Input(EntryInputError::NonmonotonicRelease)
            },
        ),
    ] {
        good &= cpu.external_warm_reset(&mut bus, reset_boundary(release, qualified))
            == Err(expected)
            && cpu == before
            && bus == device;
    }
    if state == 0 {
        return good
            && cpu.external_warm_reset(&mut bus, reset_boundary(expected_clock, true))
                == Err(EntryFault::HostFrozen)
            && cpu == before
            && bus == device;
    }
    bus.entry_failure = None;
    let old_events = bus.events.clone();
    let old_count = bus.entry_count;
    let mut expected_events = old_events;
    expected_events.extend(reset_events(expected_clock));
    let mut r = t.registers;
    r.ssp = 0x1020_3001;
    r.pc = 0xabcd_4000;
    r.set_status_register(0x2700 | t.registers.status_register() & 31);
    let result = cpu.external_warm_reset(&mut bus, reset_boundary(expected_clock, true));
    good && result.is_ok_and(|s| s.kind == EntryKind::ExternalWarmReset && s.clocks == 40)
        && cpu.registers == r
        && cpu.queued_words() == Some([0x7201, 0x4e71])
        && cpu.clock().0 == expected_clock + 40
        && bus.clock == expected_clock + 40
        && bus.events == expected_events
        && bus.words == expected_words
        && bus.entry_count == old_count + 6
        && bus.count == plan.len()
        && cpu.physical_entry_state() == PhysicalEntryState::Critical(EntryKind::ExternalWarmReset)
        && cpu.physical_inspection() == PhysicalInspection::DiagnosticCompleteValues
}

fn preflight(c: Case, route: u8) -> bool {
    let (mut cpu, mut bus) = c.seed();
    let expected = match route {
        0 => {
            cpu.registers
                .set_status_register(cpu.registers.status_register() | TRACE);
            InstructionFault::TraceUnqualified
        }
        1 => {
            cpu.interrupt_input().set_level(7);
            cpu.interrupt_input().set_level(0);
            InstructionFault::InterruptUnqualified
        }
        _ => {
            cpu.registers.pc = cpu.registers.pc.wrapping_add(2);
            InstructionFault::Execution(TimingFault::Unprimed)
        }
    };
    let before = cpu.clone();
    let device = bus.clone();
    let mut copied = before.clone();
    let mut copied_bus = device.clone();
    cpu.step_physical_instruction(&mut bus) == Err(expected.clone())
        && cpu == before
        && bus == device
        && copied.step_physical_instruction(&mut copied_bus) == Err(expected)
        && copied == before
        && copied_bus == device
}

pub(super) fn controls() -> (u32, u32) {
    let mut total = (0, 0);
    short_and_retained(&mut total);
    capacity(&mut total);
    for bank in [false, true] {
        for word in [0x0c00, 0x0c40, 0x0c80, 0x0c39] {
            let c = remainder::case(word, bank, 1);
            let mut odd = c;
            odd.r.ssp |= 1;
            total.0 += 1;
            total.1 += u32::from(!default_secondary(odd));
            for state in 0..4 {
                total.0 += 1;
                total.1 += u32::from(!reset(c, state));
            }
            for route in 0..3 {
                total.0 += 1;
                total.1 += u32::from(!preflight(c, route));
            }
        }
    }
    (total.0, total.1 + u32::from(total.0 != 212))
}
