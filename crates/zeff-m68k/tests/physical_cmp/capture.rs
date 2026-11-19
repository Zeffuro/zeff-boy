use super::*;

#[test]
fn all_2096_encoded_positions_consume_three_outcomes_banks_ccr_and_ten_actual_pairs() {
    let mut abstract_positions = [0; 2];
    let mut encoded_positions = [0; 2];
    let mut rows = 0;
    for form in forms() {
        let size = usize::from(form.size == Size::Long);
        abstract_positions[size] += form.count();
        for register in 0..if form.variable() { 8 } else { 1 } {
            for destination in 0..8 {
                for at in 0..form.count() {
                    encoded_positions[size] += 1;
                    for supervisor in [false, true] {
                        for ccr in 0..32 {
                            for operands in completion::pairs(form.size) {
                                for failure in
                                    [Failure::Host, Failure::Boundary, Failure::Unavailable]
                                {
                                    let mut case = Case::new(form);
                                    case.r = registers(supervisor, ccr);
                                    case.register = register;
                                    case.destination = destination;
                                    completion::pair(&mut case, operands);
                                    let (_, bus) = case.seed();
                                    completion::inputs(case, &bus);
                                    failed(case, at, failure);
                                    rows += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(abstract_positions, [25, 34]);
    assert_eq!(encoded_positions, [872, 1224]);
    assert_eq!(rows, 4_024_320);
}

#[test]
fn unknown_retained_n_at_f_classifies_only_word_cmp_after_flags() {
    for form in forms() {
        for supervisor in [false, true] {
            let mut case = Case::new(form);
            case.r = registers(supervisor, 31);
            case.next = 0x4afc;
            for failure in [Failure::Host, Failure::Unavailable] {
                failed(case, form.count() - 1, failure);
            }
            for at in 0..form.count() - 1 {
                failed(case, at, Failure::Boundary);
            }
            if form.size == Size::Long {
                let (cpu, bus) = case.seed();
                let memory = bus.words.clone();
                failed_seeded_with_elapsed(
                    case,
                    form.count() - 1,
                    Failure::Boundary,
                    cpu,
                    bus,
                    memory,
                );
                oracle::completed(case, true);
                continue;
            }
            let (mut cpu, mut bus) = case.seed();
            let initial = bus.words.clone();
            let plan = oracle::plan(case, &initial);
            let at = plan.len() - 1;
            let t = plan[at];
            bus.failure = Some((at, Failure::Boundary));
            let cause = CriticalFault::BusAbort {
                access: t.access,
                direction: Direction::Read,
            };
            assert_eq!(
                cpu.step_physical_instruction(&mut bus),
                Err(InstructionFault::CaptureUnavailable {
                    phase: t.phase,
                    cause
                })
            );
            assert_eq!(cpu.registers, t.registers);
            assert_eq!(cpu.queued_words(), None);
            assert_eq!(bus.words, initial);
            assert_eq!(bus.count, plan.len());
            assert_eq!(bus.entry_count, 0);
            assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
            assert_eq!(
                cpu.clock().0,
                case.start + oracle::clocks(case, &bus.elapsed, plan.len(), true)
            );
            assert_eq!(cpu.clock().0, bus.clock);
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::FaultUnqualified { cause }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
            guarded(&mut cpu, &mut bus, TimingFault::PhysicalFaultUnqualified);
            let before = cpu.clone();
            let events = bus.events.clone();
            assert_eq!(
                cpu.enter_imported_group0(&mut bus),
                Err(EntryFault::FaultUnqualified(cause))
            );
            assert_eq!(cpu, before);
            assert_eq!(bus.events, events);
            oracle::completed(case, true);
        }
    }
}

pub(super) fn failed(case: Case, at: usize, failure: Failure) {
    let (cpu, bus) = case.seed();
    let memory = bus.words.clone();
    failed_seeded(case, at, failure, cpu, bus, memory);
}

pub(super) fn failed_seeded(
    case: Case,
    at: usize,
    failure: Failure,
    cpu: PrefetchCpu,
    mut bus: BusDevice,
    memory: BTreeMap<u32, u16>,
) {
    bus.elapsed = [7, 11, 5, 13, 9];
    failed_seeded_with_elapsed(case, at, failure, cpu, bus, memory);
}

fn failed_seeded_with_elapsed(
    case: Case,
    at: usize,
    failure: Failure,
    mut cpu: PrefetchCpu,
    mut bus: BusDevice,
    memory: BTreeMap<u32, u16>,
) {
    bus.failure = Some((at, failure));
    let plan = oracle::plan(case, &memory);
    let t = plan[at];
    let c = oracle::capture(t, 2);
    let clocks = oracle::clocks(case, &bus.elapsed, at + 1, true);
    let mut events = oracle::events(case, &plan[..=at], &bus.elapsed, true);
    let result = cpu.step_physical_instruction(&mut bus);
    if failure == Failure::Boundary {
        assert_eq!(
            result,
            Ok(PhysicalInstructionStep::Group0 {
                opcode: case.opcode(),
                capture: c,
                clocks: clocks + 50,
            })
        );
        events.extend(oracle::entry_events(
            t.registers.ssp,
            c,
            case.start + clocks,
        ));
        assert_eq!(cpu.registers, oracle::entered(t.registers));
        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
        assert_eq!(bus.words, oracle::entry_memory(memory, t.registers.ssp, c));
        oracle::frame(&bus, t.registers.ssp, c, case.start + clocks);
        assert_eq!(cpu.clock().0, case.start + clocks + 50);
    } else {
        let cause = CriticalFault::BusAbort {
            access: t.access,
            direction: Direction::Read,
        };
        assert_eq!(
            result,
            Err(if failure == Failure::Host {
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
        );
        assert_eq!(cpu.registers, t.registers);
        assert_eq!(cpu.queued_words(), None);
        assert_eq!(bus.words, memory);
        assert_eq!(bus.entry_count, 0);
        assert_eq!(cpu.clock().0, case.start + clocks);
        if failure == Failure::Unavailable {
            assert_eq!(
                cpu.physical_entry_state(),
                PhysicalEntryState::FaultUnqualified { cause }
            );
            assert_eq!(cpu.physical_inspection(), PhysicalInspection::Unavailable);
        }
        guarded(
            &mut cpu,
            &mut bus,
            if failure == Failure::Host {
                TimingFault::Frozen
            } else {
                TimingFault::PhysicalFaultUnqualified
            },
        );
    }
    assert_eq!(bus.count, at + 1);
    assert_eq!(bus.events, events);
    assert_eq!(cpu.clock().0, bus.clock);
}
