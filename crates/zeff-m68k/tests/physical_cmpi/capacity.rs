use super::*;
use zeff_m68k::timing::physical::EntryKind;

fn prefix(c: Case, count: usize, elapsed: &[u32; 7]) -> (u64, Vec<Event>) {
    let (_, bus) = c.seed();
    let plan = oracle::plan(c, &bus.words);
    let idle = c.form.idle() && count >= c.form.immediates();
    let clocks = elapsed[..count].iter().map(|n| u64::from(*n)).sum::<u64>() + u64::from(idle) * 2;
    let mut events = oracle::events(c, &plan[..count], elapsed, true);
    if idle && count == c.form.immediates() {
        events.push(Event::Idle(c.start + clocks - 2, 2));
    }
    (clocks, events)
}
pub(super) fn reservations() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for blocked in 0..form.count() {
                let mut c = Case::new(form);
                c.r = registers(supervisor, 31);
                c.register = 7;
                c.start = u64::MAX - u64::from(u32::MAX) + 1
                    - blocked as u64 * 4
                    - u64::from(form.idle() && blocked >= form.immediates()) * 2;
                let (mut cpu, mut bus) = c.seed();
                let words = bus.words.clone();
                let plan = oracle::plan(c, &words);
                let (clocks, events) = prefix(c, blocked, &bus.elapsed);
                let r = if blocked == 0 {
                    c.r
                } else {
                    plan[blocked - 1].registers
                };
                let result = cpu.step_physical_instruction(&mut bus);
                count += 1;
                bad += u32::from(
                    !(result == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                        && cpu.registers == r
                        && cpu.queued_words() == Some([c.opcode(), c.retained()])
                        && bus.words == words
                        && bus.count == blocked
                        && bus.entry_count == 0
                        && bus.events == events
                        && bus.clock == c.start + clocks
                        && cpu.clock().0 == bus.clock
                        && run::frozen(&mut cpu, &mut bus, TimingFault::Frozen)),
                );
            }
        }
    }
    (count, bad + u32::from(count != 132))
}
pub(super) fn idles() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms().into_iter().filter(|f| f.idle()) {
        for supervisor in [false, true] {
            for room in [1, 2] {
                let mut c = Case::new(form);
                c.r = registers(supervisor, 31);
                c.register = 7;
                let n = form.immediates();
                c.start = u64::MAX - u64::from(u32::MAX) - (n as u64 - 1) * 4;
                let (mut cpu, mut bus) = c.seed();
                bus.elapsed[n - 1] = u32::MAX - room;
                let words = bus.words.clone();
                let plan = oracle::plan(c, &words);
                let mut events = oracle::events(c, &plan[..n], &bus.elapsed, false);
                let end = if room == 2 {
                    events.push(Event::Idle(u64::MAX - 2, 2));
                    u64::MAX
                } else {
                    u64::MAX - 1
                };
                let result = cpu.step_physical_instruction(&mut bus);
                count += 1;
                bad += u32::from(
                    !(result == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                        && cpu.registers == c.r
                        && cpu.queued_words() == Some([c.opcode(), c.retained()])
                        && bus.words == words
                        && bus.count == n
                        && bus.entry_count == 0
                        && bus.events == events
                        && bus.clock == end
                        && cpu.clock().0 == end
                        && run::frozen(&mut cpu, &mut bus, TimingFault::Frozen)),
                );
            }
        }
    }
    (count, bad + u32::from(count != 16))
}
pub(super) fn shorts(boundary: bool) -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for at in 0..form.count() {
                for elapsed in 0..4 {
                    if boundary && elapsed == 2 {
                        continue;
                    }
                    let mut c = Case::new(form);
                    c.r = registers(supervisor, 31);
                    c.register = 7;
                    let (cpu, mut bus) = c.seed();
                    bus.failure = Some((
                        at,
                        if boundary {
                            Failure::Boundary
                        } else {
                            Failure::Complete
                        },
                    ));
                    bus.elapsed[at] = elapsed;
                    count += 1;
                    bad += u32::from(!run::seeded(c, cpu, bus));
                }
            }
        }
    }
    (
        count,
        bad + u32::from(count != if boundary { 396 } else { 528 }),
    )
}
fn maximum_case(mut c: Case, at: usize, kind: Failure) -> bool {
    let clocks = at as u64 * 4 + u64::from(c.form.idle() && at >= c.form.immediates()) * 2;
    c.start = u64::MAX - u64::from(u32::MAX) - clocks;
    let (mut cpu, mut bus) = c.seed();
    bus.elapsed[at] = u32::MAX;
    bus.failure = Some((at, kind));
    if matches!(kind, Failure::Host | Failure::Unavailable) {
        return run::seeded(c, cpu, bus);
    }
    let words = bus.words.clone();
    let plan = oracle::plan(c, &words);
    let result = cpu.step_physical_instruction(&mut bus);
    let commits = kind == Failure::Complete && at + 1 == plan.len();
    let mut r = plan[at].registers;
    if commits {
        r.pc = c.r.pc.wrapping_add(c.form.length());
    }
    let expected = if commits {
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: c.opcode(),
                clocks: u64::MAX - c.start,
                exception: None,
            },
        ))
    } else {
        Err(InstructionFault::Entry(EntryFault::ClockOverflow))
    };
    let queue = if commits {
        Some([
            oracle::next(c, &words),
            oracle::sample(&words, r.pc.wrapping_add(2)),
        ])
    } else if kind == Failure::Boundary {
        None
    } else {
        Some([c.opcode(), c.retained()])
    };
    let mut good = result == expected
        && cpu.registers == r
        && cpu.queued_words() == queue
        && bus.words == words
        && bus.count == at + 1
        && bus.entry_count == 0
        && bus.events == oracle::events(c, &plan[..=at], &bus.elapsed, true)
        && bus.clock == u64::MAX
        && cpu.clock().0 == u64::MAX;
    if kind == Failure::Boundary {
        good &= cpu.physical_entry_state()
            == PhysicalEntryState::Critical(EntryKind::ImportedGroup0)
            && cpu.physical_inspection() == PhysicalInspection::DiagnosticCompleteValues;
    }
    if !commits {
        good &= run::frozen(&mut cpu, &mut bus, TimingFault::Frozen);
    }
    good
}
pub(super) fn maximum() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for at in 0..form.count() {
                for kind in [
                    Failure::Complete,
                    Failure::Host,
                    Failure::Boundary,
                    Failure::Unavailable,
                ] {
                    let mut c = Case::new(form);
                    c.r = registers(supervisor, 31);
                    c.register = 7;
                    count += 1;
                    bad += u32::from(!maximum_case(c, at, kind));
                }
            }
        }
    }
    (count, bad + u32::from(count != 528))
}
pub(super) fn finals() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            let mut c = Case::new(form);
            c.r = registers(supervisor, 31);
            c.register = 7;
            count += 1;
            bad += u32::from(!maximum_case(c, form.count() - 1, Failure::Complete));
        }
    }
    (count, bad + u32::from(count != 28))
}
