use super::*;

#[test]
fn golden_word_and_long_same_an_post_and_pre_sample_after_update_in_both_a7_banks() {
    for size in [Size::Word, Size::Long] {
        for ea in [Ea::Post, Ea::Pre] {
            for register in [0, 7] {
                for supervisor in [false, true] {
                    let mut case = Case::new(Form { size, ea });
                    case.r = registers(supervisor, 31);
                    case.r.pc = 0x1000;
                    case.register = register;
                    case.destination = register;
                    case.value = 2;
                    let initial = if ea == Ea::Post {
                        2u32.wrapping_sub(case.form.stride())
                    } else {
                        2 + case.form.stride()
                    };
                    case.r.set_address_register(usize::from(register), initial);
                    let (mut cpu, mut bus) = case.seed();
                    let expected_ea = if ea == Ea::Post { initial } else { 2 };
                    assert_eq!(oracle::address(case, &bus.words), expected_ea);
                    assert_eq!(expected_ea & 1, 0);
                    assert_eq!(oracle::value(case, &bus.words), 2);
                    let plan = oracle::plan(case, &bus.words);
                    let control = if supervisor { SUPERVISOR } else { 0 };
                    let mut tuples = vec![(
                        expected_ea,
                        case.r.pc
                            + if ea == Ea::Pre && size == Size::Word {
                                4
                            } else {
                                2
                            },
                        case.opcode(),
                        control | 31,
                        if size == Size::Long && ea == Ea::Post {
                            initial
                        } else {
                            2
                        },
                    )];
                    if size == Size::Long {
                        tuples.push((
                            expected_ea.wrapping_add(2),
                            case.r.pc + 2,
                            case.opcode(),
                            control | 31,
                            2,
                        ));
                    }
                    tuples.push((0x1004, 0x1004, case.opcode(), control | 20, 2));
                    assert_eq!(
                        plan.iter()
                            .map(|t| (
                                t.access.logical_address,
                                t.internal_pc,
                                t.ird,
                                t.registers.status_register(),
                                t.registers.address_register(usize::from(register)).unwrap()
                            ))
                            .collect::<Vec<_>>(),
                        tuples
                    );
                    assert_eq!(plan.last().unwrap().registers.d, case.r.d);
                    assert_ne!(
                        oracle::flags(case.r.status_register(), Size::Long, 2, initial),
                        control | 20
                    );
                    assert!(cpu.step_physical_instruction(&mut bus).is_ok());
                    assert_eq!(
                        cpu.registers.address_register(usize::from(register)),
                        Some(2)
                    );
                    assert_eq!(cpu.registers.status_register(), control | 20);
                    assert_eq!(cpu.registers.d, case.r.d);
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
    }
}

#[test]
fn golden_both_sizes_tail_i2_consumed_f_overflow_keeps_old_pc_queue_and_exact_room_commits() {
    for size in [Size::Word, Size::Long] {
        for supervisor in [false, true] {
            for ea in EAS {
                for room in [0, 1, 2] {
                    let mut case = Case::new(Form { size, ea });
                    case.r = registers(supervisor, 31);
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
                        assert_eq!(
                            result,
                            Ok(PhysicalInstructionStep::Instruction(
                                zeff_m68k::timing::TimedStep {
                                    opcode: case.opcode(),
                                    clocks: u64::MAX - case.start,
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
}

#[test]
fn golden_raw_word_pre_firstodd_retains_full_frame_handler_and_old_sr() {
    let mut r = Registers::new();
    r.d = [
        1219529663, 2433793244, 2609774209, 225934632, 2564942341, 2158144226, 2811115252,
        3468025223,
    ];
    r.a = [
        4064020785, 598452473, 1896672153, 775206608, 2317115287, 2436495112, 402552151,
    ];
    r.usp = 13392254;
    r.ssp = 4460878;
    r.pc = 8983662;
    r.set_status_register(8969);
    let mut cpu = PrefetchCpu::from_prefetched(r, [0xbee4, 24442], CpuClocks(0)).unwrap();
    let mut bus = Device::new(0);
    let target = 1673456420;
    for (a, v) in [
        (r.pc, 0xbee4),
        (r.pc + 2, 24442),
        (12, 25534),
        (14, 60196),
        (target, 6476),
        (target + 2, 528),
    ] {
        bus.word(a, v);
    }
    bus.detection = Some((Failure::Boundary, 8));
    let initial = bus.words.clone();
    let c = Group0Capture {
        vector: 3,
        internal_pc: 8983666,
        phase_sr: 8969,
        ird: 0xbee4,
        logical_address: 2317115285,
        function_code: FunctionCode::SupervisorData,
        direction: Direction::Read,
        processing_class: ProcessingClass::InstructionOrGroup2,
        ssw_profile: SswProfile::SelectedIrdUpperBits,
    };
    assert_eq!(c.special_status_word(), 48885);
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Group0 {
            opcode: 0xbee4,
            capture: c,
            clocks: 60
        })
    );
    let mut expected = r;
    expected.a[4] = 2317115285;
    expected.ssp -= 14;
    expected.pc = target;
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some([6476, 528]));
    assert_eq!(
        [
            bus.sample(r.ssp - 2),
            bus.sample(r.ssp - 6),
            bus.sample(r.ssp - 4),
            bus.sample(r.ssp - 8),
            bus.sample(r.ssp - 10),
            bus.sample(r.ssp - 14),
            bus.sample(r.ssp - 12)
        ],
        [5234, 8969, 137, 48868, 24469, 48885, 35356]
    );
    let mut events = vec![
        Event::Idle(0, 2),
        Event::Detect(
            oracle::access(2317115285, true, false, AccessKind::Data),
            Direction::Read,
            2,
        ),
    ];
    let mut suffix = oracle::entry_events(r.ssp, c, 10);
    for e in &mut suffix {
        if let Event::EntryRead(a, _) = e {
            if a.logical_address == 0x4321_4000 {
                *a = oracle::access(target, true, true, AccessKind::Prefetch);
            } else if a.logical_address == 0x4321_4002 {
                *a = oracle::access(target + 2, true, true, AccessKind::Prefetch);
            }
        }
    }
    events.extend(suffix);
    assert_eq!(bus.events, events);
    assert_eq!(bus.words, oracle::entry_memory(initial, r.ssp, c));
    assert_eq!(
        (cpu.clock().0, bus.clock, bus.count, bus.entry_count),
        (60, 60, 0, 11)
    );
}

#[test]
fn golden_raw_long_post_uses_actual_ram_unchanged_destination_and_trailing_i2() {
    let mut r = Registers::new();
    r.d = [
        165191649, 1133048817, 1628122477, 3110141672, 1658463515, 2289814490, 2284570393,
        2513220126,
    ];
    r.a = [
        1276321882, 1434265669, 4171408820, 1267864659, 3291022629, 3665668048, 2531371548,
    ];
    r.usp = 5951474;
    r.ssp = 16443218;
    r.pc = 11996614;
    r.set_status_register(1309);
    let mut cpu = PrefetchCpu::from_prefetched(r, [0xb5dd, 23951], CpuClocks(0)).unwrap();
    let mut bus = Device::new(0);
    for (a, v) in [
        (r.pc, 0xb5dd),
        (r.pc + 2, 23951),
        (r.pc + 4, 41188),
        (3665668048, 67),
        (3665668050, 9443),
    ] {
        bus.word(a, v);
    }
    assert_eq!(
        u32::from(bus.sample(3665668048)) << 16 | u32::from(bus.sample(3665668050)),
        4400355
    );
    let initial = bus.words.clone();
    assert_eq!(
        cpu.step_physical_instruction(&mut bus),
        Ok(PhysicalInstructionStep::Instruction(
            zeff_m68k::timing::TimedStep {
                opcode: 0xb5dd,
                clocks: 14,
                exception: None
            }
        ))
    );
    let mut expected = r;
    expected.a[5] += 4;
    expected.pc += 2;
    expected.set_status_register(1304);
    assert_eq!(cpu.registers, expected);
    assert_eq!(cpu.queued_words(), Some([23951, 41188]));
    assert_eq!(
        bus.events,
        vec![
            Event::Read(
                oracle::access(3665668048, false, false, AccessKind::Data),
                0
            ),
            Event::Read(
                oracle::access(3665668050, false, false, AccessKind::Data),
                4
            ),
            Event::Read(
                oracle::access(11996618, false, true, AccessKind::Prefetch),
                8
            ),
            Event::Idle(12, 2)
        ]
    );
    assert_eq!(bus.words, initial);
    assert_eq!((cpu.clock().0, bus.clock, bus.count), (14, 14, 3));
}
