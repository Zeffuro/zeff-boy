use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::{Cpu, SUPERVISOR, StepOutcome};

fn seed(opcode: u16, status: u16, value: u16) -> (PrefetchCpu, Bus, u32) {
    let mode = (opcode >> 3) & 7;
    let register = usize::from(opcode & 7);
    let mut timed = cpu(status);
    timed.registers.d[1] = 0xbeef_fffe;
    for i in 0..8 {
        timed
            .registers
            .set_address_register(i, 0x2000_3008 + i as u32 * 4);
    }
    let base = timed.registers.address_register(register).unwrap();
    let (extensions, address) = match mode {
        2 | 3 => (vec![], base),
        4 => (vec![], base.wrapping_sub(2)),
        5 => (vec![0xfffe], base.wrapping_sub(2)),
        6 => (vec![0x1702], base),
        7 if register == 0 => (vec![0x8030], 0xffff_8030),
        7 => (vec![0xabcd, 0x3010], 0xabcd_3010),
        _ => unreachable!(),
    };
    let words: Vec<_> = extensions.into_iter().chain([0x7201, 0x7402]).collect();
    timed.pipeline.as_mut().unwrap().words = [opcode, words[0]];
    let mut bus = Bus::default();
    let pc = timed.registers.pc & BUS_ADDRESS_MASK;
    bus.words.insert(pc, opcode);
    for (i, word) in words.iter().enumerate() {
        bus.words.insert(pc + 2 + i as u32 * 2, *word);
    }
    bus.words.insert(address & BUS_ADDRESS_MASK, value);
    (timed, bus, address)
}

#[test]
fn every_memory_shift_shape_bank_extend_and_wait_matches_architectural_work() {
    for kind_direction in 0..8 {
        for mode in 2..8 {
            for register in 0..if mode == 7 { 2 } else { 8 } {
                let opcode = 0xe0c0 | kind_direction << 8 | mode << 3 | register;
                for status in [0x0f, 0x1f, SUPERVISOR | 0x0f, SUPERVISOR | 0x1f] {
                    for value in [0, 1, 0x7fff, 0x8000, 0xffff] {
                        for wait in [0, 1, 3] {
                            let (mut timed, mut bus, _) = seed(opcode, status, value);
                            let mut raw = crate::test_bus::TestBus::default();
                            for (&address, &word) in &bus.words {
                                raw.word(address, word);
                            }
                            let mut architectural = Cpu {
                                registers: timed.registers,
                                stopped: false,
                            };
                            let expected = architectural.step(&mut raw);
                            bus.wait = wait;
                            let observed = timed.step_instruction(&mut bus).unwrap();
                            assert!(
                                matches!(expected, StepOutcome::Executed { nominal_cycles, .. } if observed.clocks == u64::from(nominal_cycles) + bus.transfers as u64 * u64::from(wait))
                            );
                            assert_eq!(timed.registers, architectural.registers, "op={opcode:04x}");
                            for (&address, &word) in &bus.words {
                                assert_eq!(
                                    word,
                                    u16::from_be_bytes([raw.peek(address), raw.peek(address + 1)])
                                );
                            }
                            assert_eq!(timed.queued_words(), Some([0x7201, 0x7402]));
                            assert_eq!(timed.clock().0, bus.clock);
                            let mut expected = match (mode, register) {
                                (4, _) => "i",
                                (6, _) => "if",
                                (5, _) | (7, 0) => "f",
                                (7, 1) => "ff",
                                _ => "",
                            }
                            .to_owned();
                            expected.push_str("rfw");
                            let actual: String = bus
                                .events
                                .iter()
                                .map(|event| match event {
                                    Event::Idle(_, 2) => 'i',
                                    Event::Read(a, _) if a.kind == AccessKind::Prefetch => 'f',
                                    Event::Read(_, _) => 'r',
                                    Event::Write(..) => 'w',
                                    _ => panic!(),
                                })
                                .collect();
                            assert_eq!(actual, expected);
                            for event in &bus.events {
                                if let Event::Read(a, _) | Event::Write(a, _, _) = event {
                                    assert_eq!(
                                        a.function_code,
                                        if a.kind == AccessKind::Prefetch {
                                            crate::ea::program_space(&timed.registers)
                                        } else {
                                            crate::ea::data_space(&timed.registers)
                                        }
                                    );
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
fn all_word_values_and_extend_inputs_have_independent_one_bit_results_and_flags() {
    for kind_direction in 0..8 {
        let opcode = 0xe0d0 | kind_direction << 8;
        for extend in [false, true] {
            let (template, mut bus, address) = seed(
                opcode,
                SUPERVISOR | if extend { crate::EXTEND } else { 0 },
                0,
            );
            for value in 0u16..=u16::MAX {
                let mut timed = template.clone();
                bus.clock = 0;
                bus.transfers = 0;
                bus.events.clear();
                bus.words.insert(address & BUS_ADDRESS_MASK, value);
                let negative = value & 0x8000 != 0;
                let carry = if kind_direction & 1 == 0 {
                    value & 1 != 0
                } else {
                    negative
                };
                let result = match kind_direction {
                    0 => (value >> 1) | (value & 0x8000),
                    1 | 3 => value.wrapping_shl(1),
                    2 => value >> 1,
                    4 => (value >> 1) | if extend { 0x8000 } else { 0 },
                    5 => value.wrapping_shl(1) | u16::from(extend),
                    6 => value.rotate_right(1),
                    7 => value.rotate_left(1),
                    _ => unreachable!(),
                };
                let expected_x = if kind_direction >= 6 { extend } else { carry };
                let mut flags = 0;
                if expected_x {
                    flags |= crate::EXTEND;
                }
                if result & 0x8000 != 0 {
                    flags |= crate::NEGATIVE;
                }
                if result == 0 {
                    flags |= crate::ZERO;
                }
                if kind_direction == 1 && negative != (result & 0x8000 != 0) {
                    flags |= crate::OVERFLOW;
                }
                if carry {
                    flags |= crate::CARRY;
                }
                assert_eq!(timed.step_instruction(&mut bus).unwrap().clocks, 12);
                assert_eq!(
                    bus.words[&(address & BUS_ADDRESS_MASK)],
                    result,
                    "op={opcode:04x} value={value:04x} x={extend}"
                );
                assert_eq!(timed.registers.status_register(), SUPERVISOR | flags);
                assert!(
                    matches!(bus.events.as_slice(), [Event::Read(a, 0), Event::Read(f, 4), Event::Write(w, _, 8)] if a.kind == AccessKind::Data && f.kind == AccessKind::Prefetch && w == a)
                );
            }
        }
    }
}

#[test]
fn every_shift_transfer_failure_preserves_completed_updates_flags_and_no_retry() {
    for kind_direction in 0..8 {
        for mode in 2..8 {
            for register in if mode == 7 { vec![0, 1] } else { vec![7] } {
                let opcode = 0xe0c0 | kind_direction << 8 | mode << 3 | register;
                for status in [0x1f, SUPERVISOR | 0x1f] {
                    let (mut completed, mut full, _) = seed(opcode, status, 0x8001);
                    full.wait = 1;
                    completed.step_instruction(&mut full).unwrap();
                    let transfers: Vec<_> = full
                        .events
                        .iter()
                        .filter(|event| !matches!(event, Event::Idle(..)))
                        .collect();
                    let data_read = transfers.iter().position(|event| matches!(event, Event::Read(a, _) if a.kind == AccessKind::Data)).unwrap();
                    for fail in 0..full.transfers {
                        let (mut timed, mut bus, _) = seed(opcode, status, 0x8001);
                        let initial = timed.clone();
                        let mut expected = timed.registers;
                        bus.fail = Some(fail);
                        bus.wait = 1;
                        assert!(matches!(
                            timed.step_instruction(&mut bus),
                            Err(TimingFault::Bus { .. })
                        ));
                        if matches!(mode, 3 | 4) && fail >= data_read {
                            expected.set_address_register(7, completed.registers.stack_pointer());
                        }
                        if fail > data_read {
                            expected.set_status_register(completed.registers.status_register());
                        }
                        assert_eq!(timed.registers, expected);
                        assert_eq!(timed.queued_words(), initial.queued_words());
                        assert!(!timed.trace_pending());
                        assert_eq!(timed.clock().0, bus.clock);
                        assert_eq!(bus.words, seed(opcode, status, 0x8001).1.words);
                        let events = bus.events.clone();
                        assert_eq!(timed.step_boundary(&mut bus), Err(TimingFault::Frozen));
                        assert_eq!(bus.events, events);
                    }
                }
            }
        }
    }
}

#[test]
fn invalid_memory_encodings_reject_before_callbacks_and_odd_words_freeze() {
    for opcode in 0xe000u16..=0xefff {
        if opcode & 0x00c0 != 0x00c0 {
            continue;
        }
        let mode = (opcode >> 3) & 7;
        if opcode & 0x0800 == 0 && mode >= 2 && (mode != 7 || opcode & 7 <= 1) {
            continue;
        }
        let mut timed = cpu(SUPERVISOR);
        timed.pipeline.as_mut().unwrap().words[0] = opcode;
        let initial = timed.clone();
        let mut bus = Bus::default();
        assert!(matches!(
            timed.step_instruction(&mut bus),
            Err(TimingFault::UnsupportedInstruction { .. })
        ));
        assert_eq!(timed, initial);
        assert!(bus.events.is_empty());
    }
    let (mut timed, mut bus, _) = seed(0xe0d0, SUPERVISOR, 0);
    timed.registers.a[0] = 0x0100_0001;
    assert_eq!(
        timed.step_instruction(&mut bus),
        Err(TimingFault::AddressError {
            address: 0x0100_0001
        })
    );
    assert!(bus.events.is_empty());
    assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
}

#[test]
fn self_modifying_tail_and_trace_observe_prefetch_before_completed_shift_write() {
    for kind_direction in 0..8 {
        let opcode = 0xe0d0 | kind_direction << 8;
        let (mut timed, mut bus, _) = seed(opcode, crate::TRACE | SUPERVISOR | crate::EXTEND, 0);
        let tail = timed.registers.pc + 4;
        timed.registers.a[0] = tail;
        bus.words.insert(tail & BUS_ADDRESS_MASK, 0x8001);
        bus.handler(9);
        timed.step_instruction(&mut bus).unwrap();
        assert_eq!(timed.queued_words(), Some([0x7201, 0x8001]));
        assert_ne!(bus.words[&(tail & BUS_ADDRESS_MASK)], 0x8001);
        assert!(timed.trace_pending());
        let flags = timed.registers.status_register();
        let sp = timed.registers.ssp;
        let boundary = bus.events.len();
        assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), Some(34));
        assert_eq!(bus.words[&(sp.wrapping_sub(6) & BUS_ADDRESS_MASK)], flags);
        assert!(matches!(bus.events[boundary], Event::Idle(_, 4)));
        let events = bus.events.clone();
        assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), None);
        assert_eq!(bus.events, events);
    }
}
