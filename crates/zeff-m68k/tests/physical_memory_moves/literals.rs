use super::*;

#[test]
fn directed_long_post_a7_pre_a7_preserves_literal_source_flags_banks_and_queue() {
    let form = Form {
        size: Size::Long,
        source: Source::Post,
        destination: Destination::Pre,
    };
    for supervisor in [false, true] {
        let mut case = Case::new(form);
        case.r = registers(supervisor, 31);
        case.r.pc = 0x1000;
        case.r.usp = 0x8000;
        case.r.ssp = 0x8000;
        case.source = 7;
        case.destination = 7;
        case.last = 0x7402;
        let (mut cpu, mut bus, _) = case.seed();
        let initial = bus.words.clone();
        let plan = oracle::plan(case, &initial);
        let control = if supervisor { 0x2000 } else { 0 };
        assert_eq!(form.opcode(7, 7), 0x2f1f);
        let actual: Vec<_> = plan
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
            actual,
            vec![
                (0x8000, 0x1002, 0x2f1f, control | 0x1f, 0x8000, None),
                (0x8002, 0x1002, 0x2f1f, control | 0x1f, 0x8004, None),
                (0x1004, 0x1004, 0x2f1f, control | 0x10, 0x8004, None),
                (0x8002, 0x1004, 0x2f1f, control | 0x18, 0x8004, Some(1)),
                (0x8000, 0x1004, 0x7201, control | 0x18, 0x8000, Some(0x8000)),
            ]
        );
        let mut legacy_cpu = cpu.clone();
        let mut legacy_bus = Device::new(case.start);
        legacy_bus.words = initial.clone();
        let expected_step = Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: 0x2f1f,
                clocks: 20,
                exception: None,
            },
        ));
        assert_eq!(
            legacy_cpu.step_physical_instruction(&mut legacy_bus),
            expected_step
        );
        assert_eq!(cpu.step_physical_instruction(&mut bus), expected_step);
        let mut expected = case.r;
        expected.pc = 0x1002;
        expected.set_status_register(control | 0x18);
        assert_eq!(cpu.registers, expected);
        assert_eq!(cpu.queued_words(), Some([0x7201, 0x7402]));
        assert_eq!(bus.sample(0x8000), 0x8000);
        assert_eq!(bus.sample(0x8002), 1);
        assert_eq!(bus.words, initial);
        assert_eq!(bus.events, oracle::events(case, &plan, &bus.elapsed, true));
        assert_eq!(legacy_cpu, cpu);
        assert_eq!(legacy_bus.words, bus.words);
        assert_eq!(legacy_bus.events, bus.events);
        assert_eq!(legacy_bus.clock, bus.clock);
        assert_eq!(cpu.clock().0, case.start + 20);
        for at in 0..5 {
            for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                capture::failed(case, at, failure);
            }
        }
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
                    destination: Destination::Indirect,
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
