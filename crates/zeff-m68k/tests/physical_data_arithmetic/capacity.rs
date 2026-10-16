use super::*;
use zeff_m68k::timing::physical::EntryKind;

fn overflow(case: Case, blocked: usize) -> bool {
    let (mut cpu, mut bus) = case.seed();
    let memory = bus.words.clone();
    let plan = oracle::plan(case, &memory);
    let result = cpu.step_physical_instruction(&mut bus);
    let expected = if blocked == 0 {
        case.r
    } else {
        plan[blocked - 1].registers
    };
    let end = case.start + oracle::clocks(case, &bus.elapsed, blocked, true);
    result == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
        && cpu.registers == expected
        && cpu.queued_words() == Some([case.opcode(), case.retained()])
        && bus.words == memory
        && bus.count == blocked
        && bus.entry_count == 0
        && bus.events == oracle::events(case, &plan[..blocked], &bus.elapsed, true)
        && bus.clock == end
        && cpu.clock().0 == end
        && run::frozen(&mut cpu, &mut bus, TimingFault::Frozen)
}

pub(super) fn reservations() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for blocked in 0..form.count() {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.register = 7;
                case.start = u64::MAX - u64::from(u32::MAX) + 1
                    - blocked as u64 * 4
                    - u64::from(form.idle()) * 2;
                count += 1;
                bad += u32::from(!overflow(case, blocked));
            }
        }
    }
    bad += u32::from(count != 472);
    (count, bad)
}

pub(super) fn idles() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms().into_iter().filter(|f| f.idle()) {
        for supervisor in [false, true] {
            for room in [1, 2] {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.register = 7;
                case.start = u64::MAX - room;
                let (mut cpu, mut bus) = case.seed();
                let words = bus.words.clone();
                let result = cpu.step_physical_instruction(&mut bus);
                let events = if room == 2 {
                    vec![Event::Idle(case.start, 2)]
                } else {
                    vec![]
                };
                let end = case.start + if room == 2 { 2 } else { 0 };
                count += 1;
                bad += u32::from(
                    !(result == Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                        && cpu.registers == case.r
                        && cpu.queued_words() == Some([case.opcode(), case.retained()])
                        && bus.count == 0
                        && bus.events == events
                        && bus.words == words
                        && bus.clock == end
                        && cpu.clock().0 == end
                        && run::frozen(&mut cpu, &mut bus, TimingFault::Frozen)),
                );
            }
        }
    }
    bad += u32::from(count != 96);
    (count, bad)
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
                    let mut case = Case::new(form);
                    case.r = registers(supervisor, 31);
                    case.register = 7;
                    let (cpu, mut bus) = case.seed();
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
                    bad += u32::from(!run::seeded(case, cpu, bus));
                }
            }
        }
    }
    bad += u32::from(count != if boundary { 1416 } else { 1888 });
    (count, bad)
}

fn maximum_case(mut case: Case, at: usize, kind: Failure) -> bool {
    let prefix = at as u64 * 4 + u64::from(case.form.idle()) * 2;
    case.start = u64::MAX - u64::from(u32::MAX) - prefix;
    let (mut cpu, mut bus) = case.seed();
    bus.elapsed[at] = u32::MAX;
    bus.failure = Some((at, kind));
    let memory = bus.words.clone();
    let plan = oracle::plan(case, &memory);
    if matches!(kind, Failure::Host | Failure::Unavailable) {
        return run::seeded(case, cpu, bus);
    }
    let result = cpu.step_physical_instruction(&mut bus);
    let mut expected = plan[at].registers;
    let final_word = at + 1 == plan.len();
    let commits = kind == Failure::Complete && final_word && case.form.size == Size::Word;
    if kind == Failure::Complete && final_word && case.form.size == Size::Long {
        expected.d[usize::from(case.destination)] =
            oracle::arithmetic(case, oracle::value(case, &memory)).0;
    }
    if commits {
        expected.pc = case.r.pc.wrapping_add(case.form.length());
    }
    let fault = Err(InstructionFault::Entry(EntryFault::ClockOverflow));
    let expected_result = if commits {
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: case.opcode(),
                clocks: u64::MAX - case.start,
                exception: None,
            },
        ))
    } else {
        fault
    };
    let queue = if commits {
        Some([
            oracle::next(case, &memory),
            oracle::sample(&memory, expected.pc.wrapping_add(2)),
        ])
    } else if kind == Failure::Boundary {
        None
    } else {
        Some([case.opcode(), case.retained()])
    };
    let mut good = result == expected_result
        && cpu.registers == expected
        && cpu.queued_words() == queue
        && bus.count == at + 1
        && bus.entry_count == 0
        && bus.events == oracle::events(case, &plan[..=at], &bus.elapsed, true)
        && bus.words == memory
        && bus.clock == u64::MAX
        && cpu.clock().0 == u64::MAX;
    if kind == Failure::Boundary {
        good &=
            cpu.physical_entry_state() == PhysicalEntryState::Critical(EntryKind::ImportedGroup0);
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
                    let mut case = Case::new(form);
                    case.r = registers(supervisor, 31);
                    case.register = 7;
                    count += 1;
                    bad += u32::from(!maximum_case(case, at, kind));
                }
            }
        }
    }
    bad += u32::from(count != 1888);
    (count, bad)
}

pub(super) fn tails() -> (u32, u32) {
    let (mut count, mut bad) = (0, 0);
    for form in forms() {
        for supervisor in [false, true] {
            for room in 0..if form.size == Size::Long { 3 } else { 1 } {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.register = 7;
                case.value = if form.size == Size::Long {
                    0x8000_0001
                } else {
                    0x8001
                };
                let prefix = (form.count() - 1) as u64 * 4 + u64::from(form.idle()) * 2;
                case.start = u64::MAX - u64::from(u32::MAX) - prefix;
                let (mut cpu, mut bus) = case.seed();
                bus.elapsed[form.count() - 1] = u32::MAX - room;
                let memory = bus.words.clone();
                let plan = oracle::plan(case, &memory);
                let result = cpu.step_physical_instruction(&mut bus);
                let success = form.size == Size::Word || room == 2;
                let mut expected = oracle::final_registers(case, &memory, &plan);
                if !success {
                    expected.pc = case.r.pc;
                }
                let end = if success {
                    u64::MAX
                } else {
                    u64::MAX - u64::from(room)
                };
                let mut events = oracle::events(case, &plan, &bus.elapsed, true);
                if success && form.size == Size::Long {
                    events.push(Event::Idle(u64::MAX - 2, 2));
                }
                let expected_result = if success {
                    Ok(PhysicalInstructionStep::Instruction(
                        zeff_m68k::timing::TimedStep {
                            opcode: case.opcode(),
                            clocks: u64::MAX - case.start,
                            exception: None,
                        },
                    ))
                } else {
                    Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                };
                let queue = if success {
                    Some([
                        oracle::next(case, &memory),
                        oracle::sample(&memory, expected.pc.wrapping_add(2)),
                    ])
                } else {
                    Some([case.opcode(), case.retained()])
                };
                let mut good = result == expected_result
                    && cpu.registers == expected
                    && cpu.queued_words() == queue
                    && bus.words == memory
                    && bus.events == events
                    && bus.count == form.count()
                    && bus.entry_count == 0
                    && cpu.clock().0 == end
                    && bus.clock == end;
                if !success {
                    good &= cpu.physical_inspection()
                        == PhysicalInspection::DiagnosticCompleteValues
                        && run::frozen(&mut cpu, &mut bus, TimingFault::Frozen);
                }
                count += 1;
                bad += u32::from(!good);
            }
        }
    }
    bad += u32::from(count != 288);
    (count, bad)
}
