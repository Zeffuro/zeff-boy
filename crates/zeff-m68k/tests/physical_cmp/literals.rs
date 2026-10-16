use super::*;

#[test]
fn golden_compare_word_pre_and_long_post_keep_dn_and_distinct_source_phases() {
    for supervisor in [false, true] {
        let control = if supervisor { SUPERVISOR } else { 0 };
        for size in [Size::Word, Size::Long] {
            let mut case = Case::new(Form {
                size,
                ea: if size == Size::Word {
                    Ea::Pre
                } else {
                    Ea::Post
                },
            });
            case.r = registers(supervisor, 31);
            case.r.pc = 0x1000;
            case.r.usp = 0x8000;
            case.r.ssp = 0x8000;
            case.register = 7;
            case.value = if size == Size::Word {
                0x8001
            } else {
                0x8000_0001
            };
            case.r.d[5] = if size == Size::Word { 0xaabb_0000 } else { 0 };
            let (_, bus) = case.seed();
            assert_eq!(oracle::value(case, &bus.words), case.value);
            let plan = oracle::plan(case, &bus.words);
            let tuples: Vec<_> = plan
                .iter()
                .map(|t| {
                    (
                        t.access.logical_address,
                        t.internal_pc,
                        t.ird,
                        t.registers.status_register(),
                        t.registers.address_register(7).unwrap(),
                        t.registers.d[5],
                    )
                })
                .collect();
            assert_eq!(
                tuples,
                if size == Size::Word {
                    vec![
                        (0x7ffe, 0x1004, 0xba67, control | 31, 0x7ffe, 0xaabb_0000),
                        (0x1004, 0x1004, 0x7201, control | 17, 0x7ffe, 0xaabb_0000),
                    ]
                } else {
                    vec![
                        (0x8000, 0x1002, 0xba9f, control | 31, 0x8000, 0),
                        (0x8002, 0x1002, 0xba9f, control | 31, 0x8004, 0),
                        (0x1004, 0x1004, 0xba9f, control | 17, 0x8004, 0),
                    ]
                }
            );
            oracle::completed(case, false);
            oracle::completed(case, true);
            for at in 0..case.form.count() {
                for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                    capture::failed(case, at, failure);
                }
            }
        }
    }
}

#[test]
fn golden_raw_word_predecrement_reproduces_literal_full_frame_and_handler() {
    let mut r = Registers::new();
    r.d = [
        3163707357, 1550976781, 949616387, 1229570850, 3702070924, 2869010757, 3004343827,
        1285750354,
    ];
    r.a = [
        1259381521, 4051363426, 2371106469, 1398560480, 2872997705, 883093764, 277825679,
    ];
    r.usp = 8030138;
    r.ssp = 3463260;
    r.pc = 12190120;
    r.set_status_register(7);
    let mut cpu = PrefetchCpu::from_prefetched(r, [0xb864, 18527], CpuClocks(0)).unwrap();
    let mut bus = Device::new(0);
    let target = 1042306476;
    for (a, v) in [
        (12, 15904),
        (14, 21932),
        (target, 24987),
        (target + 2, 28482),
    ] {
        bus.word(a, v);
    }
    bus.detection = Some((Failure::Boundary, 8));
    let initial = bus.words.clone();
    let c = Group0Capture {
        vector: 3,
        internal_pc: 12190124,
        phase_sr: 7,
        ird: 47204,
        logical_address: 2872997703,
        function_code: FunctionCode::UserData,
        direction: Direction::Read,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    };
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Group0 {
            opcode: 0xb864,
            capture: c,
            clocks: 60
        })
    );
    let mut expected = r;
    expected.a[4] = 2872997703;
    expected.ssp = 3463246;
    expected.pc = target;
    expected.set_status_register(8199);
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some([24987, 28482]));
    let mut events = vec![
        Event::Idle(0, 2),
        Event::Detect(
            oracle::access(2872997703, false, false, AccessKind::Data),
            Direction::Read,
            2,
        ),
        Event::Idle(10, 4),
    ];
    let mut memory = initial;
    for (i, (delta, word)) in [2, 6, 4, 8, 10, 14, 12]
        .into_iter()
        .zip([428, 7, 186, 47204, 30535, 47217, 43838])
        .enumerate()
    {
        let access = oracle::access(r.ssp - delta, true, false, AccessKind::Stack);
        events.push(Event::EntryWrite(access, word, 14 + 4 * i as u64));
        memory.insert(access.bus_address, word);
    }
    for (a, kind, start) in [
        (12, AccessKind::Vector, 42),
        (14, AccessKind::Vector, 46),
        (target, AccessKind::Prefetch, 50),
        (target + 2, AccessKind::Prefetch, 56),
    ] {
        if start == 56 {
            events.push(Event::Idle(54, 2));
        }
        events.push(Event::EntryRead(
            oracle::access(a, true, kind == AccessKind::Prefetch, kind),
            start,
        ));
    }
    assert_eq!(bus.events, events);
    assert_eq!(bus.words, memory);
    assert_eq!(
        (bus.count, bus.entry_count, bus.clock, cpu.clock().0),
        (0, 11, 60, 60)
    );
}

#[test]
fn golden_raw_long_postincrement_uses_actual_operand_then_refill_and_tail_i2() {
    let mut case = Case::new(Form {
        size: Size::Long,
        ea: Ea::Post,
    });
    case.r.d = [
        2840639815, 4227757821, 3570826347, 2116977896, 2563699950, 406450036, 2977729503,
        2486384061,
    ];
    case.r.a = [
        3631823290, 1838664556, 3686470150, 721292173, 2429535577, 3045700420, 2419636620,
    ];
    case.r.usp = 14100126;
    case.r.ssp = 4699762;
    case.r.pc = 4119386;
    case.r.set_status_register(9495);
    case.register = 6;
    case.destination = 1;
    case.value = 578374964;
    case.next = 30732;
    case.last = 26780;
    case.start = 0;
    let (mut cpu, mut bus) = case.seed();
    assert_eq!(case.opcode(), 0xb29e);
    assert_eq!(oracle::address(case, &bus.words), 2419636620);
    assert_eq!(oracle::value(case, &bus.words), 578374964);
    let initial = bus.words.clone();
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: 0xb29e,
                clocks: 14,
                exception: None
            }
        ))
    );
    let mut expected = case.r;
    expected.a[6] = 2419636624;
    expected.pc = 4119388;
    expected.set_status_register(9496);
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some([30732, 26780]));
    assert_eq!(
        bus.events,
        vec![
            Event::Read(oracle::access(2419636620, true, false, AccessKind::Data), 0),
            Event::Read(oracle::access(2419636622, true, false, AccessKind::Data), 4),
            Event::Read(oracle::access(4119390, true, true, AccessKind::Prefetch), 8),
            Event::Idle(12, 2),
        ]
    );
    assert_eq!(bus.words, initial);
    assert_eq!((cpu.clock().0, bus.clock, bus.count), (14, 14, 3));
}

#[test]
fn long_tail_i2_consumed_f_overflow_retains_old_queue_and_exact_two_clocks_commit() {
    for supervisor in [false, true] {
        for ea in EAS {
            for room in [0, 1, 2] {
                let mut case = Case::new(Form {
                    size: Size::Long,
                    ea,
                });
                case.r = registers(supervisor, 31);
                case.value = 0x8000_0001;
                case.r.d[5] = 0;
                let count = case.form.count();
                let prefix = (count as u64 - 1) * 4 + u64::from(case.form.idle()) * 2;
                case.start = u64::MAX - u64::from(u32::MAX) - prefix;
                let (mut cpu, mut bus) = case.seed();
                bus.elapsed[count - 1] = u32::MAX - room;
                let memory = bus.words.clone();
                let plan = oracle::plan(case, &memory);
                let mut events = oracle::events(case, &plan, &bus.elapsed, true);
                let mut expected = plan.last().unwrap().registers;
                let result = cpu.step_physical_instruction(&mut bus);
                if room < 2 {
                    assert_eq!(
                        result,
                        Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                    );
                    assert_eq!(cpu.queued_words(), Some([case.opcode(), case.retained()]));
                    assert_eq!(cpu.clock().0, u64::MAX - u64::from(room));
                    assert_eq!(
                        cpu.physical_inspection(),
                        PhysicalInspection::DiagnosticCompleteValues
                    );
                    guarded(&mut cpu, &mut bus, TimingFault::Frozen);
                } else {
                    let clocks = u64::MAX - case.start;
                    assert_eq!(
                        result,
                        Ok(PhysicalInstructionStep::Instruction(
                            zeff_m68k::timing::TimedStep {
                                opcode: case.opcode(),
                                clocks,
                                exception: None
                            }
                        ))
                    );
                    expected.pc = case.r.pc.wrapping_add(case.form.length());
                    assert_eq!(
                        cpu.queued_words(),
                        Some([oracle::next(case, &memory), case.last])
                    );
                    assert_eq!(cpu.clock().0, u64::MAX);
                    events.push(Event::Idle(u64::MAX - 2, 2));
                }
                assert_eq!(cpu.registers, expected);
                assert_eq!(bus.events, events);
                assert_eq!(bus.words, memory);
                assert_eq!(bus.count, count);
                assert_eq!(bus.entry_count, 0);
                assert_eq!(cpu.clock().0, bus.clock);
            }
        }
    }
}
