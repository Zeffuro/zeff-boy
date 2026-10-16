use super::*;

#[test]
fn directed_long_post_a7_abs_long_fetches_write_mutated_n_after_both_writes() {
    let form = Form {
        size: Size::Long,
        source: Source::Post,
        destination: Destination::AbsoluteLong,
    };
    for supervisor in [false, true] {
        let mut case = Case::new(form);
        case.r = registers(supervisor, 31);
        case.r.pc = 0x1000;
        case.r.usp = 0x8000;
        case.r.ssp = 0x8000;
        case.source = 7;
        case.destination_extension = 0;
        case.destination_low = 0x1006;
        case.last = 0x7402;
        let (mut cpu, mut bus, _) = case.seed();
        let initial = bus.words.clone();
        let plan = oracle::plan(case, &initial);
        let control = if supervisor { 0x2000 } else { 0 };
        assert_eq!(form.opcode(7, 0), 0x23df);
        let phases: Vec<_> = plan
            .iter()
            .map(|t| {
                (
                    t.access.logical_address,
                    t.internal_pc,
                    t.ird,
                    t.registers.status_register(),
                    t.registers.address_register(7).unwrap(),
                    t.value,
                )
            })
            .collect();
        assert_eq!(
            phases,
            vec![
                (0x8000, 0x1002, 0x23df, control | 0x1f, 0x8000, None),
                (0x8002, 0x1002, 0x23df, control | 0x1f, 0x8004, None),
                (0x1004, 0x1004, 0x23df, control | 0x1f, 0x8004, None),
                (0x1006, 0x1004, 0x23df, control | 0x10, 0x8004, Some(0x8000)),
                (0x1008, 0x1004, 0x23df, control | 0x18, 0x8004, Some(1)),
                (0x1006, 0x1006, 0x23df, control | 0x18, 0x8004, None),
                (0x1008, 0x1008, 0x8000, control | 0x18, 0x8004, None),
            ]
        );
        assert_eq!(
            cpu.step_physical_instruction(&mut bus),
            Ok(PhysicalInstructionStep::Instruction(
                zeff_m68k::timing::TimedStep {
                    opcode: 0x23df,
                    clocks: 28,
                    exception: None,
                }
            ))
        );
        let mut expected = case.r;
        expected.set_address_register(7, 0x8004);
        expected.pc = 0x1006;
        expected.set_status_register(control | 0x18);
        assert_eq!(cpu.registers, expected);
        assert_eq!(cpu.queued_words(), Some([0x8000, 1]));
        assert_eq!(bus.sample(0x1006), 0x8000);
        assert_eq!(bus.sample(0x1008), 1);
        assert_eq!(bus.count, 7);
        assert_eq!(bus.words, oracle::memory(initial, &plan));
        assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
        assert_eq!(cpu.clock().0, case.start + 28);
        assert_eq!(cpu.clock().0, bus.clock);
        for at in 0..7 {
            for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                if at == 6 && failure == Failure::Boundary {
                    continue;
                }
                capture::failed(case, at, failure);
            }
        }
        let (mut fault_cpu, mut fault_bus, _) = case.seed();
        fault_bus.failure = Some((6, Failure::Boundary));
        let t = plan[6];
        let capture = oracle::capture(t, 2);
        assert_eq!(capture.ird, 0x8000);
        assert_eq!(capture.phase_sr, control | 0x18);
        assert_eq!(capture.internal_pc, 0x1008);
        assert_eq!(capture.special_status_word(), 0x8012 | control >> 11);
        assert_eq!(
            fault_cpu.step_physical_instruction(&mut fault_bus),
            Ok(PhysicalInstructionStep::Group0 {
                opcode: 0x23df,
                capture,
                clocks: 78,
            })
        );
        assert_eq!(fault_cpu.registers, oracle::entered(t.registers));
        assert_eq!(fault_bus.count, 7);
        assert_eq!(fault_bus.entry_count, 11);
        assert_eq!(fault_cpu.queued_words(), Some([0x4e71, 0x7201]));
        assert_eq!(
            fault_bus.words,
            oracle::entry_memory(bus.words.clone(), t.registers.ssp, capture)
        );
        let mut events = bus.events.clone();
        events.extend(oracle::entry_events(
            t.registers.ssp,
            capture,
            case.start + 28,
        ));
        assert_eq!(fault_bus.events, events);
        oracle::frame(&fault_bus, t.registers.ssp, capture, case.start + 28);
        assert_eq!(fault_cpu.clock().0, case.start + 78);
        assert_eq!(fault_cpu.clock().0, fault_bus.clock);
    }
}

#[test]
fn source_literal_pc_ird_old_sr_and_program_data_space_table_is_independent() {
    let rows = [
        (Source::Indirect, vec![], 2, 2, false),
        (Source::Post, vec![], 2, 2, false),
        (Source::Pre, vec![], 4, 2, false),
        (Source::Displacement, vec![2], 2, 2, false),
        (Source::Indexed, vec![2], 2, 2, false),
        (Source::AbsoluteWord, vec![4], 4, 4, false),
        (Source::AbsoluteLong, vec![2, 6], 6, 6, false),
        (Source::PcDisplacement, vec![2], 2, 2, true),
        (Source::PcIndexed, vec![2], 2, 2, true),
    ];
    for (source, extension_pc, word_pc, long_pc, program) in rows {
        for size in [Size::Byte, Size::Word, Size::Long] {
            for supervisor in [false, true] {
                let form = Form {
                    size,
                    source,
                    destination: Destination::Displacement,
                };
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                let (_, bus, _) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                for (index, offset) in extension_pc.iter().enumerate() {
                    assert_eq!(plan[index].internal_pc, case.r.pc + offset);
                    assert_eq!(plan[index].access.kind, AccessKind::Prefetch);
                }
                let operand = &plan[form.extensions()..form.reads()];
                for t in operand {
                    assert_eq!(
                        t.internal_pc,
                        case.r.pc + if size == Size::Long { long_pc } else { word_pc }
                    );
                    assert_eq!(t.registers.status_register(), case.r.status_register());
                    assert_eq!(t.ird, form.opcode(case.source, case.destination));
                    assert_eq!(t.access.kind, AccessKind::Data);
                    assert_eq!(
                        t.access.function_code,
                        match (program, supervisor) {
                            (false, false) => FunctionCode::UserData,
                            (false, true) => FunctionCode::SupervisorData,
                            (true, false) => FunctionCode::UserProgram,
                            (true, true) => FunctionCode::SupervisorProgram,
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn extended_destination_literal_low_full_flags_pc_and_ird_tables_are_independent() {
    for destination in DESTINATIONS {
        for size in [Size::Byte, Size::Word, Size::Long] {
            for supervisor in [false, true] {
                let mut case = Case::new(Form {
                    size,
                    source: Source::AbsoluteLong,
                    destination,
                });
                case.r = registers(supervisor, 31);
                case.r.pc = 0x1000;
                case.value = 0x0001_8000;
                let (_, bus, _) = case.seed();
                let plan = oracle::plan(case, &bus.words);
                let c = 0x1006;
                let o = case.form.opcode(case.source, case.destination);
                let control = if supervisor { SUPERVISOR } else { 0 };
                let low = match size {
                    Size::Byte => 0x14,
                    _ => 0x18,
                };
                let full = if size == Size::Long { 0x10 } else { low };
                let tuples: Vec<_> = plan[case.form.reads()..]
                    .iter()
                    .map(|t| {
                        (
                            t.phase,
                            t.internal_pc,
                            t.ird,
                            t.registers.status_register() & 31,
                        )
                    })
                    .collect();
                let mut expected = vec![(
                    InstructionPhase::ExtensionFirst,
                    c + 2,
                    o,
                    if size == Size::Long && destination == Destination::AbsoluteLong {
                        31
                    } else {
                        low
                    },
                )];
                expected.push((
                    InstructionPhase::OperandWriteHigh,
                    c + 2,
                    o,
                    if destination == Destination::AbsoluteLong {
                        low
                    } else {
                        full
                    },
                ));
                if size == Size::Long {
                    expected.push((InstructionPhase::OperandWriteLow, c + 2, o, full));
                }
                if destination == Destination::AbsoluteLong {
                    expected.push((InstructionPhase::ExtensionSecond, c + 4, o, full));
                }
                expected.push((
                    InstructionPhase::OperandRefill,
                    c + if destination == Destination::AbsoluteLong {
                        6
                    } else {
                        4
                    },
                    0x7201,
                    full,
                ));
                assert_eq!(tuples, expected);
                assert!(
                    plan.iter()
                        .all(|t| t.registers.status_register() & SUPERVISOR == control)
                );
                completion::completed(case, false);
            }
        }
    }
}
