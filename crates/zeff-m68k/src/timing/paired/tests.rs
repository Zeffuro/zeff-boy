use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::{Cpu, SUPERVISOR, StepOutcome};

pub(super) fn put(bus: &mut Bus, address: u32, size: Size, value: u32) {
    let external = address & BUS_ADDRESS_MASK & !1;
    if size == Size::Byte {
        let old = *bus.words.get(&external).unwrap_or(&0xa55a);
        bus.words.insert(
            external,
            if address & 1 == 0 {
                (old & 255) | ((value as u16 & 255) << 8)
            } else {
                (old & 0xff00) | (value as u16 & 255)
            },
        );
    } else if size == Size::Long {
        bus.words.insert(external, (value >> 16) as u16);
        bus.words
            .insert(address.wrapping_add(2) & BUS_ADDRESS_MASK, value as u16);
    } else {
        bus.words.insert(external, value as u16);
    }
}

pub(super) fn seed(
    opcode: u16,
    status: u16,
    odd: bool,
    values: (u32, u32),
) -> (PrefetchCpu, Bus, u32, u32) {
    let pair = decode(opcode).unwrap();
    let size = match pair {
        Pair::Extend(size, _) | Pair::Compare(size) => size,
        Pair::Decimal(_) => Size::Byte,
    };
    let source = usize::from(opcode & 7);
    let destination = usize::from((opcode >> 9) & 7);
    let mut timed = cpu(status);
    for i in 0..8 {
        timed.registers.d[i] = 0x8000_0000 + i as u32;
        timed
            .registers
            .set_address_register(i, 0x2000_3008 + i as u32 * 0x100 + u32::from(odd));
    }
    let step = |register| {
        if size == Size::Byte && register == 7 {
            2
        } else {
            size.bytes()
        }
    };
    let comparison = matches!(pair, Pair::Compare(_));
    let source_base = timed.registers.address_register(source).unwrap();
    let source_address = if comparison {
        source_base
    } else {
        source_base.wrapping_sub(step(source))
    };
    let destination_base = if source == destination {
        if comparison {
            source_base.wrapping_add(step(source))
        } else {
            source_address
        }
    } else {
        timed.registers.address_register(destination).unwrap()
    };
    let destination_address = if comparison {
        destination_base
    } else {
        destination_base.wrapping_sub(step(destination))
    };
    timed.pipeline.as_mut().unwrap().words = [opcode, 0x7201];
    let mut bus = Bus::default();
    let pc = timed.registers.pc & BUS_ADDRESS_MASK;
    bus.words
        .extend([(pc, opcode), (pc + 2, 0x7201), (pc + 4, 0x7402)]);
    put(&mut bus, source_address, size, values.0);
    put(&mut bus, destination_address, size, values.1);
    (timed, bus, source_address, destination_address)
}

pub(super) fn bases() -> [u16; 11] {
    [
        0xc108, 0x8108, 0xd108, 0xd148, 0xd188, 0x9108, 0x9148, 0x9188, 0xb108, 0xb148, 0xb188,
    ]
}

#[test]
fn all_sixty_four_pairs_sizes_banks_lanes_x_z_and_waits_match_architectural_work() {
    for base in bases() {
        let pair = decode(base).unwrap();
        let byte = matches!(
            pair,
            Pair::Decimal(_) | Pair::Extend(Size::Byte, _) | Pair::Compare(Size::Byte)
        );
        for source in 0..8 {
            for destination in 0..8 {
                let opcode = base | destination << 9 | source;
                for status in [0, crate::ZERO, crate::EXTEND, crate::EXTEND | crate::ZERO] {
                    for bank in [0, SUPERVISOR] {
                        for odd in [false, true] {
                            if odd && !byte {
                                continue;
                            }
                            for values in [
                                (0, 0),
                                (0xffff_ffff, 0x8000_0000),
                                (0x0102_0399, 0x7fff_128f),
                            ] {
                                for wait in [0, 1, 3] {
                                    let (mut timed, mut bus, _, _) =
                                        seed(opcode, status | bank, odd, values);
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
                                        matches!(expected, StepOutcome::Executed { nominal_cycles, .. } if observed.clocks == u64::from(nominal_cycles) + bus.transfers as u64 * u64::from(wait)),
                                        "op={opcode:04x}"
                                    );
                                    assert_eq!(
                                        timed.registers, architectural.registers,
                                        "op={opcode:04x}"
                                    );
                                    for (&address, &word) in &bus.words {
                                        assert_eq!(
                                            word,
                                            u16::from_be_bytes([
                                                raw.peek(address),
                                                raw.peek(address + 1)
                                            ])
                                        );
                                    }
                                    assert_eq!(timed.queued_words(), Some([0x7201, 0x7402]));
                                    assert_eq!(timed.clock().0, bus.clock);
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
    }
}

#[test]
fn paired_order_addresses_and_long_write_before_fetch_are_exact() {
    for base in bases() {
        for source in 0..8 {
            for destination in 0..8 {
                let opcode = base | destination << 9 | source;
                let (mut timed, mut bus, source_address, destination_address) =
                    seed(opcode, SUPERVISOR | 0x1f, false, (0x0102_0304, 0x8123_4567));
                let pair = decode(opcode).unwrap();
                timed.step_instruction(&mut bus).unwrap();
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
                let long_extend = matches!(pair, Pair::Extend(Size::Long, _));
                let comparison = matches!(pair, Pair::Compare(_));
                assert_eq!(
                    actual,
                    if long_extend {
                        "irrrrwfw"
                    } else if comparison {
                        if matches!(pair, Pair::Compare(Size::Long)) {
                            "rrrrf"
                        } else {
                            "rrf"
                        }
                    } else {
                        "irrfw"
                    }
                );
                let reads: Vec<_> = bus
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        Event::Read(a, _) if a.kind == AccessKind::Data => Some(a.logical_address),
                        _ => None,
                    })
                    .collect();
                let expected = if long_extend {
                    vec![
                        source_address.wrapping_add(2),
                        source_address,
                        destination_address.wrapping_add(2),
                        destination_address,
                    ]
                } else if matches!(pair, Pair::Compare(Size::Long)) {
                    vec![
                        source_address,
                        source_address.wrapping_add(2),
                        destination_address,
                        destination_address.wrapping_add(2),
                    ]
                } else {
                    vec![source_address, destination_address]
                };
                assert_eq!(reads, expected);
                let writes: Vec<_> = bus
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        Event::Write(a, _, _) => Some(a.logical_address),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    writes,
                    if long_extend {
                        vec![destination_address.wrapping_add(2), destination_address]
                    } else if comparison {
                        vec![]
                    } else {
                        vec![destination_address]
                    }
                );
            }
        }
    }
}

pub(super) fn nbcd_seed(opcode: u16, status: u16, odd: bool, value: u8) -> (PrefetchCpu, Bus, u32) {
    let mode = (opcode >> 3) & 7;
    let register = usize::from(opcode & 7);
    let mut timed = cpu(status);
    timed.registers.d[1] = 0xbeef_fffe;
    for i in 0..8 {
        timed
            .registers
            .set_address_register(i, 0x2000_3008 + i as u32 * 4 + u32::from(odd));
    }
    let base = timed.registers.address_register(register).unwrap();
    let (extensions, address) = match mode {
        2 | 3 => (vec![], base),
        4 => (vec![], base.wrapping_sub(if register == 7 { 2 } else { 1 })),
        5 => (vec![0xfffe], base.wrapping_sub(2)),
        6 => (vec![0x1702], base),
        7 if register == 0 => (vec![0x8030 + u16::from(odd)], 0xffff_8030 + u32::from(odd)),
        7 => (
            vec![0xabcd, 0x3010 + u16::from(odd)],
            0xabcd_3010 + u32::from(odd),
        ),
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
    put(&mut bus, address, Size::Byte, u32::from(value));
    (timed, bus, address)
}

#[test]
fn nbcd_all_shapes_banks_lanes_decimal_edges_x_z_and_waits_match_architectural_work() {
    for mode in 2..8 {
        for register in 0..if mode == 7 { 2 } else { 8 } {
            let opcode = 0x4800 | mode << 3 | register;
            for bank in [0, SUPERVISOR] {
                for flags in [0, crate::ZERO, crate::EXTEND, crate::EXTEND | crate::ZERO] {
                    for odd in [false, true] {
                        for value in [0, 1, 9, 10, 0x99, 0x9a, 0xff] {
                            for wait in [0, 1, 3] {
                                let (mut timed, mut bus, _) =
                                    nbcd_seed(opcode, bank | flags, odd, value);
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
                                assert_eq!(timed.registers, architectural.registers);
                                for (&address, &word) in &bus.words {
                                    assert_eq!(
                                        word,
                                        u16::from_be_bytes([
                                            raw.peek(address),
                                            raw.peek(address + 1)
                                        ])
                                    );
                                }
                                assert_eq!(timed.queued_words(), Some([0x7201, 0x7402]));
                                assert_eq!(timed.clock().0, bus.clock);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn long_extend_self_modifies_before_tail_fetch_and_keeps_already_queued_next_opcode() {
    let (mut timed, mut bus, _, _) = seed(0xd388, SUPERVISOR, false, (0x0102_0304, 0));
    let pc = timed.registers.pc;
    timed.registers.a[1] = pc + 6;
    bus.words.insert((pc + 2) & BUS_ADDRESS_MASK, 0x7201);
    bus.words.insert((pc + 4) & BUS_ADDRESS_MASK, 0x1234);
    timed.step_instruction(&mut bus).unwrap();
    assert_eq!(timed.queued_words(), Some([0x7201, 0x1538]));
    assert_eq!(bus.words[&((pc + 2) & BUS_ADDRESS_MASK)], 0x7303);
    assert_eq!(bus.words[&((pc + 4) & BUS_ADDRESS_MASK)], 0x1538);
    let (mut timed, mut bus, _, _) = seed(0xd388, SUPERVISOR, false, (0x0102_0304, 0));
    timed.registers.a[1] = 2;
    put(&mut bus, 0xffff_fffe, Size::Long, 0x1122_3344);
    timed.step_instruction(&mut bus).unwrap();
    let writes: Vec<_> = bus
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Write(a, word, _) => Some((a.logical_address, a.bus_address, *word)),
            _ => None,
        })
        .collect();
    assert_eq!(writes, [(0, 0, 0x3648), (0xffff_fffe, 0xfffffe, 0x1224)]);
}
