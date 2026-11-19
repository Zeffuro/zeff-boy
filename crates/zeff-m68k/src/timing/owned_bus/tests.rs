use super::spy::*;
use super::*;
use crate::SUPERVISOR;

#[test]
fn reset_preserves_cpu_and_owns_exactly_124_clocks_without_double_device_advancement() {
    for ccr in 0..32 {
        for trace in [0, TRACE] {
            for wait in [0, 1, 3] {
                let (mut cpu, mut spy) = reset_seed(SUPERVISOR | trace | 0x0700 | ccr);
                let mut expected = cpu.registers;
                expected.pc += 2;
                spy.bus.wait = wait;
                spy.pulse_writes
                    .extend([(0x340102, 0xbeef), (0x340104, 0xcafe)]);
                let result = cpu.step_instruction(&mut spy).unwrap();
                assert_eq!(cpu.registers, expected);
                assert_eq!(result.clocks, 132 + u64::from(wait));
                assert_eq!(result.exception, None);
                assert_eq!(cpu.trace_pending(), trace != 0);
                assert_eq!(cpu.queued_words(), Some([0x7201, 0xcafe]));
                assert_eq!(spy.owner, Owner::None);
                assert_eq!(spy.bus.transfers, 1);
                let Event::Read(access, _, Owner::None) = spy.events[4] else {
                    panic!("missing tail")
                };
                assert_eq!(access.kind, AccessKind::Prefetch);
                assert_eq!(access.function_code, FunctionCode::SupervisorProgram);
                assert_eq!(access.logical_address, 0x1234_0104);
                assert_eq!(
                    spy.events,
                    [
                        Event::Idle(0, 4, Owner::None),
                        Event::BeginReset(4),
                        Event::Idle(4, 124, Owner::Reset),
                        Event::EndReset(128),
                        Event::Read(access, 128, Owner::None)
                    ]
                );
                assert_eq!(spy.bus.clock, cpu.clock().0);
            }
        }
    }
}

#[test]
fn reset_user_privilege_precedes_optional_capability_and_suppresses_entry_trace() {
    for ccr in 0..32 {
        for trace in [0, TRACE] {
            for support in [Begin::Complete, Begin::Unsupported, Begin::Error] {
                let (mut cpu, mut spy) = reset_seed(trace | ccr);
                let initial = cpu.registers;
                spy.reset = support;
                let result = cpu.step_instruction(&mut spy).unwrap();
                assert_eq!(result.exception, Some(8));
                assert_eq!(result.clocks, 34);
                assert!(!cpu.trace_pending());
                assert_eq!(spy.owner, Owner::None);
                assert_eq!(cpu.registers.usp, initial.usp);
                assert_eq!(cpu.registers.d, initial.d);
                assert_eq!(cpu.registers.a, initial.a);
                assert_eq!(spy.bus.words[&0xffa], initial.status_register());
                assert_eq!(spy.bus.words[&0xffe], initial.pc as u16);
                assert!(!spy.events.iter().any(|event| matches!(
                    event,
                    Event::BeginReset(..) | Event::EndReset(..) | Event::Cleanup(..)
                )));
            }
        }
    }
}

#[test]
fn tas_dn_all_old_bytes_registers_flags_banks_and_waits_need_no_owned_callbacks() {
    for register in 0..8 {
        for old in 0..=255_u32 {
            for ccr in 0..32 {
                for bank in [0, SUPERVISOR] {
                    for wait in [0, 1, 3] {
                        let (mut cpu, mut spy) = reset_seed(bank | TRACE | 0x0700 | ccr);
                        cpu.pipeline.as_mut().unwrap().words[0] = 0x4ac0 | register;
                        cpu.registers.d[usize::from(register)] = 0x89ab_cd00 | old;
                        let mut expected = cpu.registers;
                        expected.pc += 2;
                        expected.d[usize::from(register)] |= 0x80;
                        expected.set_status_register(flags(expected.status_register(), old as u8));
                        spy.reset = Begin::Error;
                        spy.rmw = Begin::Error;
                        spy.bus.wait = wait;
                        assert_eq!(
                            cpu.step_instruction(&mut spy).unwrap().clocks,
                            4 + u64::from(wait)
                        );
                        assert_eq!(cpu.registers, expected);
                        assert_eq!(spy.events.len(), 1);
                        assert!(matches!(spy.events[0], Event::Read(_, 0, Owner::None)));
                        assert_eq!(spy.owner, Owner::None);
                        assert!(cpu.trace_pending());
                    }
                }
            }
        }
    }
}

#[test]
fn tas_all_memory_encodings_flags_banks_lanes_indices_and_waits_match_full_architecture() {
    assert_eq!(shapes().len(), 42);
    for shape in shapes() {
        for old in [0, 1, 0x7f, 0x80, 0xff] {
            for ccr in 0..32 {
                for bank in [0, SUPERVISOR] {
                    for trace in [0, TRACE] {
                        for parity in [0, 1] {
                            for wait in [0, 1, 3] {
                                for index in [0x1010, 0x1910, 0xf810, 0xff10] {
                                    let (mut cpu, mut spy, address, _) = seed(
                                        shape,
                                        bank | trace | 0x0700 | ccr,
                                        old,
                                        parity,
                                        index,
                                    );
                                    let mut raw = crate::test_bus::TestBus {
                                        support_tas: true,
                                        tas_write: true,
                                        ..Default::default()
                                    };
                                    for (&address, &word) in &spy.bus.words {
                                        raw.word(address, word);
                                    }
                                    let mut expected = cpu.registers;
                                    let pc = expected.pc;
                                    let cycles = crate::instruction::execute(
                                        &mut expected,
                                        &mut raw,
                                        pc,
                                        cpu.pipeline.unwrap().words[0],
                                    )
                                    .unwrap()
                                    .nominal_cycles;
                                    spy.bus.wait = wait;
                                    let result = cpu.step_instruction(&mut spy).unwrap();
                                    assert_eq!(cpu.registers, expected);
                                    assert_eq!(
                                        result.clocks,
                                        u64::from(cycles)
                                            + spy.bus.transfers as u64 * u64::from(wait)
                                    );
                                    assert_eq!(result.exception, None);
                                    assert_eq!(cpu.trace_pending(), trace != 0);
                                    assert_eq!(cpu.queued_words(), Some([0x7201, 0x7402]));
                                    for (&address, &word) in &spy.bus.words {
                                        assert_eq!(
                                            word,
                                            u16::from_be_bytes([
                                                raw.peek(address),
                                                raw.peek(address + 1)
                                            ])
                                        );
                                    }
                                    let begin = spy
                                        .events
                                        .iter()
                                        .position(|event| matches!(event, Event::BeginRmw(..)))
                                        .unwrap();
                                    let Event::BeginRmw(access, start) = spy.events[begin] else {
                                        unreachable!()
                                    };
                                    assert_eq!(access.logical_address, address);
                                    assert_eq!(access.bus_address, address & BUS_ADDRESS_MASK & !1);
                                    assert_eq!(
                                        access.function_code,
                                        if bank == 0 {
                                            FunctionCode::UserData
                                        } else {
                                            FunctionCode::SupervisorData
                                        }
                                    );
                                    assert_eq!(access.kind, AccessKind::Data);
                                    assert_eq!(
                                        access.lanes,
                                        if address & 1 == 0 {
                                            ByteLanes::Upper
                                        } else {
                                            ByteLanes::Lower
                                        }
                                    );
                                    let end = start + 10 + u64::from(wait) * 2;
                                    assert_eq!(
                                        spy.events[begin + 1],
                                        Event::Read(access, start, Owner::Rmw)
                                    );
                                    assert_eq!(
                                        spy.events[begin + 2],
                                        Event::Idle(start + 4 + u64::from(wait), 2, Owner::Rmw)
                                    );
                                    let value = u16::from(old | 0x80)
                                        << if address & 1 == 0 { 8 } else { 0 };
                                    assert_eq!(
                                        spy.events[begin + 3],
                                        Event::Write(
                                            access,
                                            value,
                                            start + 6 + u64::from(wait),
                                            Owner::Rmw
                                        )
                                    );
                                    assert_eq!(spy.events[begin + 4], Event::EndRmw(end));
                                    assert!(
                                        matches!(spy.events[begin + 5], Event::Read(_, time, Owner::None) if time == end)
                                    );
                                    assert_eq!(spy.owner, Owner::None);
                                    assert_eq!(cpu.clock().0, spy.bus.clock);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn tas_uses_sampled_old_bytes_and_board_suppression_preserves_ram_without_skipping_write() {
    for parity in [0, 1] {
        for sample in [0, 1, 0x7f, 0x80, 0xff] {
            for ignore in [false, true] {
                let (mut cpu, mut spy, address, _) =
                    seed((3, 7), SUPERVISOR | 0x1f, 0x7f, parity, 0x1010);
                let original = cpu.registers;
                let aligned = address & BUS_ADDRESS_MASK & !1;
                let before = spy.bus.words[&aligned];
                spy.bus.read_values.push((
                    0,
                    if address & 1 == 0 {
                        u16::from(sample) << 8 | 0x55
                    } else {
                        0x5500 | u16::from(sample)
                    },
                ));
                spy.ignore_write = ignore;
                spy.bus.wait = 3;
                cpu.step_instruction(&mut spy).unwrap();
                assert_eq!(
                    cpu.registers.status_register(),
                    flags(original.status_register(), sample)
                );
                assert_eq!(cpu.registers.ssp, original.ssp + 2);
                let expected = if ignore {
                    before
                } else if address & 1 == 0 {
                    u16::from(sample | 0x80) << 8 | 0x55
                } else {
                    0x5500 | u16::from(sample | 0x80)
                };
                assert_eq!(spy.bus.words[&aligned], expected);
                assert_eq!(spy.bus.transfers, 3);
                assert!(
                    spy.events
                        .iter()
                        .any(|event| matches!(event, Event::Write(_, _, _, Owner::Rmw)))
                );
                assert_eq!(cpu.clock().0, 23);
            }
        }
    }
    let (mut cpu, mut spy, _, _) = seed((2, 0), SUPERVISOR, 0, 0, 0x1010);
    cpu.registers.set_address_register(0, cpu.registers.pc + 4);
    spy.bus.words.insert(0x340104, 0x0002);
    cpu.step_instruction(&mut spy).unwrap();
    assert_eq!(cpu.queued_words(), Some([0x7201, 0x8002]));
    assert_eq!(cpu.registers.status_register(), SUPERVISOR | 4);
}

#[test]
fn completed_owned_operations_release_before_trace_and_preserve_a7_bank_and_wrap() {
    for ccr in 0..32 {
        for bank in [0, SUPERVISOR] {
            for shape in [(3, 7), (4, 7), (7, 0), (7, 1)] {
                let (mut cpu, mut spy, _, _) = seed(shape, bank | TRACE | ccr, 0x80, 0, 0x1010);
                cpu.step_instruction(&mut spy).unwrap();
                let completed = cpu.registers;
                assert_eq!(spy.owner, Owner::None);
                assert_eq!(cpu.service_pending_trace(&mut spy).unwrap(), Some(34));
                assert_eq!(
                    spy.bus.words[&(completed.ssp.wrapping_sub(6) & BUS_ADDRESS_MASK)],
                    completed.status_register()
                );
                assert_eq!(
                    spy.bus.words[&(completed.ssp.wrapping_sub(2) & BUS_ADDRESS_MASK)],
                    completed.pc as u16
                );
            }
        }
        let (mut cpu, mut spy) = reset_seed(SUPERVISOR | TRACE | ccr);
        cpu.step_instruction(&mut spy).unwrap();
        assert_eq!(cpu.service_pending_trace(&mut spy).unwrap(), Some(34));
        assert_eq!(spy.bus.words[&0xffa], SUPERVISOR | TRACE | ccr);
    }
    let (mut cpu, mut spy, _, _) = seed((3, 7), SUPERVISOR, 0, 0, 0x1010);
    cpu.registers.ssp = 0xffff_ffff;
    spy.bus.words.insert(0xfffffe, 0x5500);
    cpu.step_instruction(&mut spy).unwrap();
    assert_eq!(cpu.registers.ssp, 1);
    assert_eq!(spy.bus.words[&0xfffffe], 0x5580);
    assert!(
        matches!(spy.events[0], Event::BeginRmw(access, 0) if access.logical_address == 0xffff_ffff && access.bus_address == 0xfffffe && access.lanes == ByteLanes::Lower)
    );
}
