use super::*;

#[test]
fn golden_both_sizes_same_an_post_pre_publish_low_then_high_without_changing_sr() {
    for operation in [Operation::Add, Operation::Subtract] {
        let (sampled, partial, result) = if operation == Operation::Add {
            (0x0001_fffe, 0x0001_0000, 0x0002_0000)
        } else {
            (0x0002_0000, 0x0002_fffe, 0x0001_fffe)
        };
        for size in [Size::Word, Size::Long] {
            for ea in [Ea::Post, Ea::Pre] {
                for register in [0, 7] {
                    for supervisor in [false, true] {
                        let mut case = Case::new(Form {
                            operation,
                            size,
                            ea,
                        });
                        case.r = registers(supervisor, 31);
                        case.r.pc = 0x1000;
                        case.register = register;
                        case.destination = register;
                        case.value = 2;
                        let initial = if ea == Ea::Post {
                            sampled - case.form.stride()
                        } else {
                            sampled + case.form.stride()
                        };
                        case.r.set_address_register(usize::from(register), initial);
                        let (_, bus) = case.seed();
                        let expected_ea = if ea == Ea::Post { initial } else { sampled };
                        assert_eq!(oracle::address(case, &bus.words), expected_ea);
                        assert_eq!(expected_ea & 1, 0);
                        assert_eq!(oracle::value(case, &bus.words), 2);
                        assert_eq!(oracle::arithmetic(case, &bus.words), (partial, result));
                        let plan = oracle::plan(case, &bus.words);
                        let mut tuples = vec![(
                            expected_ea,
                            if ea == Ea::Pre && size == Size::Word {
                                0x1004
                            } else {
                                0x1002
                            },
                            case.opcode(),
                            if size == Size::Long && ea == Ea::Post {
                                initial
                            } else {
                                sampled
                            },
                        )];
                        if size == Size::Long {
                            tuples.push((expected_ea + 2, 0x1002, case.opcode(), sampled));
                        }
                        tuples.push((0x1004, 0x1004, 0x7201, partial));
                        assert_eq!(
                            plan.iter()
                                .map(|t| (
                                    t.access.logical_address,
                                    t.internal_pc,
                                    t.ird,
                                    t.registers.address_register(usize::from(register)).unwrap()
                                ))
                                .collect::<Vec<_>>(),
                            tuples
                        );
                        assert!(plan.iter().all(|t| t.registers.status_register()
                            == case.r.status_register()
                            && t.registers.d == case.r.d));
                        oracle::completed(case, false);
                        oracle::completed(case, true);
                        for at in 0..case.form.count() {
                            for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable]
                            {
                                capture::failed(case, at, failure);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn golden_word_two_i2_and_long_one_i2_freeze_exact_publication_and_old_queue_at_max() {
    for form in forms() {
        for supervisor in [false, true] {
            let tail = if form.size == Size::Word { 4 } else { 2 };
            for room in 0..=tail {
                let mut case = Case::new(form);
                case.r = registers(supervisor, 31);
                case.value = 2;
                case.r.a[5] = if form.operation == Operation::Add {
                    0x1fffe
                } else {
                    0x20000
                };
                let count = form.count();
                let prefix = (count as u64 - 1) * 4 + u64::from(form.idle()) * 2;
                case.start = u64::MAX - u64::from(u32::MAX) - prefix;
                let (mut cpu, mut bus) = case.seed();
                bus.elapsed[count - 1] = u32::MAX - room;
                let memory = bus.words.clone();
                assert_eq!(oracle::value(case, &memory), 2);
                assert_eq!(oracle::address(case, &memory) & 1, 0);
                let plan = oracle::plan(case, &memory);
                let mut events = oracle::events(case, &plan, &bus.elapsed, true);
                let mut expected = plan.last().unwrap().registers;
                let full = form.size == Size::Long || room >= 2;
                if full {
                    expected.set_address_register(5, oracle::arithmetic(case, &memory).1);
                }
                let successful_idles = if room == tail {
                    tail / 2
                } else if form.size == Size::Word && room >= 2 {
                    1
                } else {
                    0
                };
                for i in 0..successful_idles {
                    events.push(Event::Idle(
                        u64::MAX - u64::from(room) + u64::from(i * 2),
                        2,
                    ));
                }
                let result = cpu.step_physical_instruction(&mut bus);
                if room < tail {
                    assert_eq!(
                        result,
                        Err(InstructionFault::Entry(EntryFault::ClockOverflow))
                    );
                    assert_eq!(cpu.queued_words(), Some([case.opcode(), case.retained()]));
                    assert_eq!(
                        cpu.clock().0,
                        u64::MAX - u64::from(room) + u64::from(successful_idles * 2)
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
                    expected.pc = case.r.pc.wrapping_add(form.length());
                    assert_eq!(
                        cpu.queued_words(),
                        Some([oracle::next(case, &memory), case.last])
                    );
                    assert_eq!(cpu.clock().0, u64::MAX);
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

#[test]
fn golden_newly_odd_partial_a7_keeps_usp_separate_and_double_halts_supervisor_stack() {
    for operation in [Operation::Add, Operation::Subtract] {
        for size in [Size::Word, Size::Long] {
            for supervisor in [false, true] {
                let mut case = Case::new(Form {
                    operation,
                    size,
                    ea: Ea::Indirect,
                });
                case.r = registers(supervisor, 31);
                case.register = 0;
                case.destination = 7;
                case.value = if operation == Operation::Add { 3 } else { 1 };
                case.r.set_address_register(
                    7,
                    if operation == Operation::Add {
                        0x1fffe
                    } else {
                        0x20000
                    },
                );
                let (_, bus) = case.seed();
                assert_eq!(oracle::address(case, &bus.words) & 1, 0);
                assert_eq!(oracle::value(case, &bus.words), case.value);
                assert_eq!(
                    oracle::arithmetic(case, &bus.words),
                    if operation == Operation::Add {
                        (0x10001, 0x20001)
                    } else {
                        (0x2ffff, 0x1ffff)
                    }
                );
                oracle::completed(case, false);
                oracle::completed(case, true);
                for failure in [Failure::Host, Failure::Boundary, Failure::Unavailable] {
                    capture::failed(case, case.form.count() - 1, failure);
                }
            }
        }
    }
}
