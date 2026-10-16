use super::tests::{bases, nbcd_seed, put, seed};
use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, Event, cpu};

fn updates(registers: &mut Registers, pair: Pair, source: usize, destination: usize, fail: usize) {
    let size = match pair {
        Pair::Extend(size, _) | Pair::Compare(size) => size,
        Pair::Decimal(_) => Size::Byte,
    };
    let step = |register| {
        if size == Size::Byte && register == 7 {
            2
        } else {
            size.bytes()
        }
    };
    let mut advance = |register, amount: u32, subtract| {
        let old = registers.address_register(register).unwrap();
        registers.set_address_register(
            register,
            if subtract {
                old.wrapping_sub(amount)
            } else {
                old.wrapping_add(amount)
            },
        );
    };
    if matches!(pair, Pair::Compare(_)) {
        if size == Size::Long {
            advance(source, 2, false);
            if fail >= 1 {
                advance(source, 2, false);
            }
            if fail >= 4 {
                advance(destination, 4, false);
            }
        } else {
            advance(source, step(source), false);
            if fail >= 2 {
                advance(destination, step(destination), false);
            }
        }
    } else if size == Size::Long {
        if fail >= 1 {
            advance(source, 4, true);
        }
        if fail >= 3 {
            advance(destination, 4, true);
        }
    } else {
        advance(source, step(source), true);
        if fail >= 1 {
            advance(destination, step(destination), true);
        }
    }
}

#[test]
fn all_pair_aliases_every_transfer_failure_keep_verified_address_stages_and_completed_low_write() {
    for base in bases() {
        for source in 0..8 {
            for destination in 0..8 {
                let opcode = base | destination << 9 | source;
                let pair = decode(opcode).unwrap();
                let long = matches!(
                    pair,
                    Pair::Extend(Size::Long, _) | Pair::Compare(Size::Long)
                );
                for status in [0x1f, SUPERVISOR | 0x1f] {
                    let (mut completed, mut full, _, _) =
                        seed(opcode, status, false, (0x0102_0304, 0x8123_4567));
                    full.wait = 1;
                    completed.step_instruction(&mut full).unwrap();
                    let transfers: Vec<_> = full
                        .events
                        .iter()
                        .filter(|event| !matches!(event, Event::Idle(..)))
                        .collect();
                    for fail in 0..full.transfers {
                        let (mut timed, mut bus, _, _) =
                            seed(opcode, status, false, (0x0102_0304, 0x8123_4567));
                        let queue = timed.queued_words();
                        let mut expected = timed.registers;
                        let mut memory = bus.words.clone();
                        updates(
                            &mut expected,
                            pair,
                            source as usize,
                            destination as usize,
                            fail,
                        );
                        if fail > if long { 3 } else { 1 } {
                            expected.set_status_register(completed.registers.status_register());
                        }
                        bus.fail = Some(fail);
                        bus.wait = 1;
                        assert!(matches!(
                            timed.step_instruction(&mut bus),
                            Err(TimingFault::Bus { .. })
                        ));
                        for event in transfers.iter().take(fail) {
                            if let Event::Write(a, value, _) = event {
                                let old = memory[&a.bus_address];
                                memory.insert(
                                    a.bus_address,
                                    match a.lanes {
                                        ByteLanes::Both => *value,
                                        ByteLanes::Upper => (old & 255) | (value & 0xff00),
                                        ByteLanes::Lower => (old & 0xff00) | (value & 255),
                                    },
                                );
                            }
                        }
                        assert_eq!(timed.registers, expected, "op={opcode:04x} fail={fail}");
                        assert_eq!(timed.queued_words(), queue);
                        assert_eq!(bus.words, memory);
                        assert_eq!(timed.clock().0, bus.clock);
                        assert!(!timed.trace_pending());
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
fn nbcd_every_transfer_failure_and_exact_schedules_preserve_saved_ea() {
    for mode in 2..8 {
        for register in if mode == 7 { vec![0, 1] } else { vec![7] } {
            let opcode = 0x4800 | mode << 3 | register;
            for status in [crate::TRACE | 0x1f, crate::TRACE | SUPERVISOR | 0x1f] {
                let (mut completed, mut full, address) = nbcd_seed(opcode, status, false, 0x9a);
                full.wait = 1;
                completed.step_instruction(&mut full).unwrap();
                let actual: String = full
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
                let prefix = match (mode, register) {
                    (4, _) => "i",
                    (5, _) | (7, 0) => "f",
                    (6, _) => "if",
                    (7, 1) => "ff",
                    _ => "",
                };
                assert_eq!(actual, format!("{prefix}rfw"));
                assert!(
                    matches!(full.events.last(), Some(Event::Write(a, _, _)) if a.logical_address == address)
                );
                let transfers: Vec<_> = full
                    .events
                    .iter()
                    .filter(|event| !matches!(event, Event::Idle(..)))
                    .collect();
                let data_read = transfers
                    .iter()
                    .position(
                        |event| matches!(event, Event::Read(a, _) if a.kind == AccessKind::Data),
                    )
                    .unwrap();
                for fail in 0..full.transfers {
                    let (mut timed, mut bus, _) = nbcd_seed(opcode, status, false, 0x9a);
                    let initial = timed.clone();
                    let memory = bus.words.clone();
                    let mut expected = timed.registers;
                    if matches!(mode, 3 | 4) && fail >= data_read {
                        expected.set_address_register(7, completed.registers.stack_pointer());
                    }
                    if fail > data_read {
                        expected.set_status_register(completed.registers.status_register());
                    }
                    bus.fail = Some(fail);
                    bus.wait = 1;
                    assert!(matches!(
                        timed.step_instruction(&mut bus),
                        Err(TimingFault::Bus { .. })
                    ));
                    assert_eq!(timed.registers, expected);
                    assert_eq!(timed.queued_words(), initial.queued_words());
                    assert_eq!(bus.words, memory);
                    assert!(!timed.trace_pending());
                    assert_eq!(timed.clock().0, bus.clock);
                    let events = bus.events.clone();
                    assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
                    assert_eq!(bus.events, events);
                }
            }
        }
    }
}

#[test]
fn illegal_nbcd_and_register_aliases_are_not_misdecoded_as_memory_pairs() {
    for opcode in [
        0xc100, 0x8100, 0xd100, 0x9100, 0xb100, 0xd1c8, 0x91c8, 0xb1c8, 0x4800,
    ] {
        let mut timed = cpu(SUPERVISOR);
        timed.pipeline.as_mut().unwrap().words[0] = opcode;
        let initial = timed.clone();
        let mut bus = Bus::default();
        assert_eq!(
            timed.paired_instruction(&mut bus, timed.pipeline.unwrap()),
            Ok(false)
        );
        assert_eq!(timed, initial);
        assert!(bus.events.is_empty());
    }
    for operand in [8, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f] {
        let mut timed = cpu(SUPERVISOR);
        timed.pipeline.as_mut().unwrap().words[0] = 0x4800 | operand;
        let initial = timed.clone();
        let mut bus = Bus::default();
        assert!(matches!(
            timed.step_instruction(&mut bus),
            Err(TimingFault::UnsupportedInstruction { .. })
        ));
        assert_eq!(timed, initial);
        assert!(bus.events.is_empty());
    }
}

#[test]
fn extended_and_decimal_zero_flags_are_sticky_and_source_values_survive_overlapping_writes() {
    for (opcode, source, destination, flags, result, expected_flags) in [
        (0xd308, 0, 0, crate::ZERO, 0, crate::ZERO),
        (0xd308, 0, 0, 0, 0, 0),
        (
            0x9348,
            0xffff,
            0,
            crate::EXTEND | crate::ZERO,
            0,
            crate::EXTEND | crate::CARRY | crate::ZERO,
        ),
        (
            0xd388,
            0x7fff_ffff,
            0,
            crate::EXTEND | crate::ZERO,
            0x8000_0000,
            crate::NEGATIVE | crate::OVERFLOW,
        ),
        (
            0xc308,
            0x99,
            0,
            crate::EXTEND | crate::ZERO,
            0,
            crate::EXTEND | crate::CARRY | crate::ZERO,
        ),
        (
            0x8308,
            0x99,
            0,
            crate::EXTEND | crate::ZERO,
            0,
            crate::EXTEND | crate::CARRY | crate::ZERO,
        ),
    ] {
        let (mut timed, mut bus, _, address) =
            seed(opcode, SUPERVISOR | flags, false, (source, destination));
        let size = match decode(opcode).unwrap() {
            Pair::Extend(size, _) => size,
            _ => Size::Byte,
        };
        timed.step_instruction(&mut bus).unwrap();
        assert_eq!(
            timed.registers.status_register(),
            SUPERVISOR | expected_flags
        );
        let first = bus.words[&(address & BUS_ADDRESS_MASK & !1)];
        let observed = match size {
            Size::Byte => u32::from(if address & 1 == 0 {
                first >> 8
            } else {
                first & 255
            }),
            Size::Word => u32::from(first),
            Size::Long => {
                (u32::from(first) << 16)
                    | u32::from(bus.words[&(address.wrapping_add(2) & BUS_ADDRESS_MASK)])
            }
        };
        assert_eq!(observed, result);
    }
    let (mut timed, mut bus, source_address, _) = seed(0xd388, SUPERVISOR, false, (0x0102_0304, 0));
    timed.registers.a[1] = source_address + 2;
    put(&mut bus, source_address.wrapping_sub(2), Size::Word, 0xabcd);
    timed.step_instruction(&mut bus).unwrap();
    assert_eq!(bus.words[&(source_address & BUS_ADDRESS_MASK)], 0x0406);
    assert_eq!(
        bus.words[&(source_address.wrapping_sub(2) & BUS_ADDRESS_MASK)],
        0xaccf
    );
    assert_eq!(
        bus.words[&(source_address.wrapping_add(2) & BUS_ADDRESS_MASK)],
        0x0304
    );
}

#[test]
fn trace_follows_full_pair_completion_and_trace_host_failure_retains_instruction_writes() {
    for base in bases() {
        let opcode = base | 7 << 9 | 7;
        let (mut timed, mut bus, _, _) = seed(
            opcode,
            crate::TRACE | SUPERVISOR | 0x1f,
            false,
            (0x0102_0304, 0x8123_4567),
        );
        bus.handler(9);
        timed.step_instruction(&mut bus).unwrap();
        assert!(timed.trace_pending());
        let flags = timed.registers.status_register();
        let sp = timed.registers.ssp;
        assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), Some(34));
        assert_eq!(bus.words[&(sp.wrapping_sub(6) & BUS_ADDRESS_MASK)], flags);
        let (mut timed, mut bus, _, _) = seed(
            opcode,
            crate::TRACE | SUPERVISOR | 0x1f,
            false,
            (0x0102_0304, 0x8123_4567),
        );
        bus.handler(9);
        timed.step_instruction(&mut bus).unwrap();
        let registers = timed.registers;
        let memory = bus.words.clone();
        let queue = timed.queued_words();
        bus.fail = Some(bus.transfers);
        assert!(matches!(
            timed.service_pending_trace(&mut bus),
            Err(TimingFault::Bus { .. })
        ));
        assert_eq!(timed.registers, registers);
        assert_eq!(timed.queued_words(), queue);
        assert_eq!(bus.words, memory);
        let events = bus.events.clone();
        assert_eq!(timed.step_boundary(&mut bus), Err(TimingFault::Frozen));
        assert_eq!(bus.events, events);
    }
}

#[test]
fn odd_word_addresses_freeze_at_source_or_destination_without_unearned_register_updates() {
    for opcode in [0xd348, 0xd388, 0xb348, 0xb388] {
        let pair = decode(opcode).unwrap();
        let comparison = matches!(pair, Pair::Compare(_));
        let long = matches!(
            pair,
            Pair::Extend(Size::Long, _) | Pair::Compare(Size::Long)
        );
        for odd_source in [true, false] {
            let (mut timed, mut bus, _, _) =
                seed(opcode, SUPERVISOR | 0x1f, false, (0x0102_0304, 0x8123_4567));
            let register = if odd_source { 0 } else { 1 };
            let base = 0x0100_0001;
            timed.registers.set_address_register(register, base);
            let initial = timed.clone();
            let address = if comparison {
                base
            } else {
                base.wrapping_sub(2)
            };
            assert_eq!(
                timed.step_instruction(&mut bus),
                Err(TimingFault::AddressError { address })
            );
            assert_eq!(
                timed.registers.status_register(),
                initial.registers.status_register()
            );
            assert_eq!(timed.registers.pc, initial.registers.pc);
            assert_eq!(timed.queued_words(), initial.queued_words());
            let expected_address_register = if comparison && odd_source {
                base + 2
            } else if !comparison && !long {
                base.wrapping_sub(2)
            } else {
                base
            };
            assert_eq!(
                timed.registers.address_register(register).unwrap(),
                expected_address_register
            );
            assert_eq!(
                bus.transfers,
                if odd_source {
                    0
                } else if long {
                    2
                } else {
                    1
                }
            );
            assert!(
                !bus.events
                    .iter()
                    .any(|event| matches!(event, Event::Write(..)))
            );
            let events = bus.events.clone();
            assert_eq!(timed.step_boundary(&mut bus), Err(TimingFault::Frozen));
            assert_eq!(bus.events, events);
        }
    }
}
