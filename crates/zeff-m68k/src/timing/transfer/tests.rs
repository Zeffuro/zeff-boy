use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, Event, cpu};

pub(super) fn shapes() -> Vec<(bool, u16, usize)> {
    [false, true]
        .into_iter()
        .flat_map(|load| {
            [2, if load { 3 } else { 4 }, 5, 6, 7]
                .into_iter()
                .flat_map(move |mode| {
                    (0..if mode == 7 {
                        if load { 4 } else { 2 }
                    } else {
                        8
                    })
                        .map(move |register| (load, mode, register))
                })
        })
        .collect()
}

pub(super) fn masks() -> Vec<u16> {
    [0, 0xffff, 0x5555, 0xaaaa]
        .into_iter()
        .chain((0..16).map(|bit| 1 << bit))
        .collect()
}

pub(super) fn opcode(shape: (bool, u16, usize), long: bool) -> u16 {
    0x4880
        | if shape.0 { 0x400 } else { 0 }
        | if long { 0x40 } else { 0 }
        | shape.1 << 3
        | shape.2 as u16
}

pub(super) fn value(registers: &Registers, index: usize) -> u32 {
    if index < 8 {
        registers.d[index]
    } else {
        registers.address_register(index - 8).unwrap()
    }
}

pub(super) fn assign(registers: &mut Registers, index: usize, value: u32) {
    if index < 8 {
        registers.d[index] = value;
    } else {
        registers.set_address_register(index - 8, value);
    }
}

pub(super) fn movem_seed(
    shape: (bool, u16, usize),
    long: bool,
    mask: u16,
    sr: u16,
    index: u16,
) -> (PrefetchCpu, Bus, u32, u32) {
    let (_, mode, register) = shape;
    let mut cpu = cpu(sr);
    for index in 0..7 {
        cpu.registers
            .set_address_register(index, 0x2345_3000 + index as u32 * 0x100);
    }
    for index in 0..8 {
        cpu.registers.d[index] = 0x8765_8010 + index as u32 * 0x102;
    }
    let pc = cpu.registers.pc;
    let base = if mode == 7 {
        pc + 4
    } else {
        cpu.registers.address_register(register).unwrap()
    };
    let (extensions, address) = match (mode, register) {
        (2..=4, _) => (vec![], base),
        (5, _) | (7, 2) => (vec![0x0040], base + 0x40),
        (6, _) | (7, 3) => {
            let raw = value(&cpu.registers, usize::from((index >> 12) & 15));
            let offset = if index & 0x0800 == 0 {
                i32::from(raw as i16) as u32
            } else {
                raw
            };
            (
                vec![index],
                base.wrapping_add(offset)
                    .wrapping_add_signed(i32::from(index as u8 as i8)),
            )
        }
        (7, 0) => (vec![0x8030], 0xffff_8030),
        (7, 1) => (vec![0xabcd, 0x3010], 0xabcd_3010),
        _ => unreachable!(),
    };
    let cursor = pc + 4 + extensions.len() as u32 * 2;
    let words: Vec<_> = [mask]
        .into_iter()
        .chain(extensions)
        .chain([0x7201, 0x7402])
        .collect();
    cpu.pipeline.as_mut().unwrap().words = [opcode(shape, long), mask];
    let mut bus = Bus::default();
    bus.handler(9);
    for offset in -34_i32..=34 {
        let position = address.wrapping_add_signed(offset * 2) & BUS_ADDRESS_MASK;
        bus.words
            .insert(position, 0x8000 | ((offset + 34) as u16 * 0x102));
    }
    for (offset, &word) in words.iter().enumerate() {
        bus.words
            .insert((pc + 2 + offset as u32 * 2) & BUS_ADDRESS_MASK, word);
    }
    (cpu, bus, address, cursor)
}

pub(super) fn movep_seed(
    opcode: u16,
    sr: u16,
    displacement: u16,
    parity: u32,
) -> (PrefetchCpu, Bus, u32) {
    let mut cpu = cpu(sr);
    for register in 0..7 {
        cpu.registers
            .set_address_register(register, 0x2345_4000 + register as u32 * 0x100 + parity);
    }
    cpu.registers.set_address_register(7, 0x3000 + parity);
    for register in 0..8 {
        cpu.registers.d[register] = 0xa1b2_c3d4 ^ (register as u32 * 0x0306_0912);
    }
    let pc = cpu.registers.pc;
    let address = cpu
        .registers
        .address_register(usize::from(opcode & 7))
        .unwrap()
        .wrapping_add_signed(i32::from(displacement as i16));
    cpu.pipeline.as_mut().unwrap().words = [opcode, displacement];
    let mut bus = Bus::default();
    bus.handler(9);
    for offset in 0..5 {
        bus.words.insert(
            address.wrapping_add(offset * 2) & BUS_ADDRESS_MASK & !1,
            0x1234 + offset as u16 * 0x1111,
        );
    }
    bus.words.extend([
        (pc.wrapping_add(2) & BUS_ADDRESS_MASK, displacement),
        (pc.wrapping_add(4) & BUS_ADDRESS_MASK, 0x7201),
        (pc.wrapping_add(6) & BUS_ADDRESS_MASK, 0x7402),
    ]);
    (cpu, bus, address)
}

pub(super) fn transfers(bus: &Bus) -> Vec<(BusAccess, Option<u16>)> {
    bus.events
        .iter()
        .filter_map(|event| match event {
            Event::Read(access, _) => Some((*access, None)),
            Event::Write(access, value, _) => Some((*access, Some(*value))),
            _ => None,
        })
        .collect()
}

fn compare(cpu: &mut PrefetchCpu, bus: &mut Bus) {
    let mut raw = crate::test_bus::TestBus::default();
    for (&address, &word) in &bus.words {
        raw.word(address, word);
    }
    let mut expected = cpu.registers;
    let status = expected.status_register();
    let pc = expected.pc;
    let opcode = cpu.pipeline.unwrap().words[0];
    let cycles = crate::instruction::execute(&mut expected, &mut raw, pc, opcode)
        .unwrap()
        .nominal_cycles;
    let result = cpu.step_instruction(bus).unwrap();
    assert_eq!(cpu.registers, expected, "{opcode:04x}");
    assert_eq!(result.exception, None);
    assert_eq!(cpu.trace_pending(), status & TRACE != 0);
    assert_eq!(
        result.clocks,
        u64::from(cycles) + bus.transfers as u64 * u64::from(bus.wait)
    );
    assert_eq!(cpu.clock().0, bus.clock);
    for (&address, &word) in &bus.words {
        assert_eq!(
            word,
            u16::from_be_bytes([raw.peek(address), raw.peek(address.wrapping_add(1))])
        );
    }
    let pc = cpu.registers.pc;
    assert_eq!(
        cpu.queued_words(),
        Some([
            bus.words[&(pc & BUS_ADDRESS_MASK)],
            bus.words[&(pc.wrapping_add(2) & BUS_ADDRESS_MASK)]
        ])
    );
}

#[test]
fn all_movem_encodings_masks_banks_trace_waits_and_ccrs_match_full_architectural_state() {
    assert_eq!(shapes().len(), 70);
    for shape in shapes() {
        for long in [false, true] {
            for mask in masks() {
                for bank in [0, SUPERVISOR] {
                    for trace in [0, TRACE] {
                        for wait in [0, 1, 3] {
                            for ccr in [0, 0x1f] {
                                let (mut cpu, mut bus, _, _) = movem_seed(
                                    shape,
                                    long,
                                    mask,
                                    bank | trace | 0x0500 | ccr,
                                    0x1810,
                                );
                                bus.wait = wait;
                                compare(&mut cpu, &mut bus);
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn movem_schedule_word_order_fc_discard_and_original_store_values_are_exact() {
    for shape in shapes() {
        let (load, mode, register) = shape;
        for long in [false, true] {
            for mask in masks() {
                for bank in [0, SUPERVISOR] {
                    let (mut cpu, mut bus, address, cursor) =
                        movem_seed(shape, long, mask, bank | 0x1f, 0x1810);
                    let original = cpu.registers;
                    let pc = original.pc;
                    cpu.step_instruction(&mut bus).unwrap();
                    let program = crate::ea::program_space(&original);
                    let data = if mode == 7 && register >= 2 {
                        program
                    } else {
                        crate::ea::data_space(&original)
                    };
                    let mut expected = vec![(pc + 4, AccessKind::Prefetch, program, None)];
                    if mode >= 5 {
                        expected.push((pc + 6, AccessKind::Prefetch, program, None));
                    }
                    if mode == 7 && register == 1 {
                        expected.push((pc + 8, AccessKind::Prefetch, program, None));
                    }
                    let selected: Vec<_> = (0..16)
                        .filter(|bit| mask & (1 << bit) != 0)
                        .map(|bit| if mode == 4 { 15 - bit } else { bit })
                        .collect();
                    let bytes = if long { 4 } else { 2 };
                    for (position, &index) in selected.iter().enumerate() {
                        let target = if mode == 4 {
                            address.wrapping_sub((position as u32 + 1) * bytes)
                        } else {
                            address.wrapping_add(position as u32 * bytes)
                        };
                        let raw = value(&original, index);
                        let mut words = if long {
                            vec![
                                (target, (raw >> 16) as u16),
                                (target.wrapping_add(2), raw as u16),
                            ]
                        } else {
                            vec![(target, raw as u16)]
                        };
                        if mode == 4 && long {
                            words.reverse();
                        }
                        for (target, value) in words {
                            expected.push((
                                target,
                                AccessKind::Data,
                                data,
                                if load { None } else { Some(value) },
                            ));
                        }
                    }
                    if load {
                        expected.push((
                            address.wrapping_add(selected.len() as u32 * bytes),
                            AccessKind::Discard,
                            data,
                            None,
                        ));
                    }
                    expected.push((cursor + 2, AccessKind::Prefetch, program, None));
                    let actual: Vec<_> = transfers(&bus)
                        .into_iter()
                        .map(|(access, value)| {
                            assert_eq!(access.lanes, ByteLanes::Both);
                            assert_eq!(
                                access.bus_address,
                                access.logical_address & BUS_ADDRESS_MASK
                            );
                            (
                                access.logical_address,
                                access.kind,
                                access.function_code,
                                value,
                            )
                        })
                        .collect();
                    assert_eq!(actual, expected, "{:04x}/{mask:04x}", opcode(shape, long));
                    let idle: Vec<_> = bus
                        .events
                        .iter()
                        .enumerate()
                        .filter_map(|(index, event)| match event {
                            Event::Idle(_, clocks) => Some((index, *clocks)),
                            _ => None,
                        })
                        .collect();
                    assert_eq!(
                        idle,
                        if mode == 6 || mode == 7 && register == 3 {
                            vec![(1, 2)]
                        } else {
                            vec![]
                        }
                    );
                    if matches!(mode, 3 | 4) {
                        let expected = if mode == 4 {
                            address.wrapping_sub(selected.len() as u32 * bytes)
                        } else {
                            address.wrapping_add(selected.len() as u32 * bytes)
                        };
                        assert_eq!(cpu.registers.address_register(register), Some(expected));
                    }
                }
            }
        }
    }
}

#[test]
fn movem_index_base_and_register_inputs_remain_immutable_across_selected_loads() {
    for load in [false, true] {
        for base_register in 0..8 {
            for index_register in 0..16 {
                for width in [0, 0x0800] {
                    for ignored in [0, 0x0700] {
                        for bank in [0, SUPERVISOR] {
                            let index = (index_register << 12) | width | ignored | 0x10;
                            let (mut cpu, mut bus, address, _) = movem_seed(
                                (load, 6, base_register),
                                true,
                                0xffff,
                                bank | 0x1f,
                                index,
                            );
                            let original = cpu.registers;
                            compare(&mut cpu, &mut bus);
                            let events: Vec<_> = transfers(&bus)
                                .into_iter()
                                .filter(|(access, _)| access.kind != AccessKind::Prefetch)
                                .collect();
                            assert_eq!(events[0].0.logical_address, address);
                            assert_eq!(events[31].0.logical_address, address + 62);
                            if !load {
                                assert_eq!(events[31].1, Some(original.stack_pointer() as u16));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn all_movep_register_pairs_widths_directions_lanes_displacements_banks_and_waits_match() {
    for destination in 0..8 {
        for address_register in 0..8 {
            for long in [false, true] {
                for store in [false, true] {
                    for parity in [0, 1] {
                        for displacement in [0, 2, 0xfffe, 0x7ffe, 0x8000, 0xffff] {
                            for bank in [0, SUPERVISOR] {
                                for trace in [0, TRACE] {
                                    for wait in [0, 1, 3] {
                                        let opcode = 0x0108
                                            | destination << 9
                                            | address_register
                                            | if long { 0x40 } else { 0 }
                                            | if store { 0x80 } else { 0 };
                                        let (mut cpu, mut bus, address) = movep_seed(
                                            opcode,
                                            bank | trace | 0x051f,
                                            displacement,
                                            parity,
                                        );
                                        bus.wait = wait;
                                        let original = cpu.registers;
                                        let before = bus.words.clone();
                                        compare(&mut cpu, &mut bus);
                                        let events = transfers(&bus);
                                        let bytes = if long { 4 } else { 2 };
                                        assert_eq!(events.len(), bytes + 2);
                                        assert_eq!(events[0].0.logical_address, original.pc + 4);
                                        assert_eq!(
                                            events[bytes + 1].0.logical_address,
                                            original.pc + 6
                                        );
                                        for position in 0..bytes {
                                            let (access, value) = events[position + 1];
                                            let target = address.wrapping_add(position as u32 * 2);
                                            assert_eq!(access.logical_address, target);
                                            assert_eq!(
                                                access.bus_address,
                                                target & BUS_ADDRESS_MASK & !1
                                            );
                                            assert_eq!(
                                                access.function_code,
                                                crate::ea::data_space(&original)
                                            );
                                            assert_eq!(access.kind, AccessKind::Data);
                                            assert_eq!(
                                                access.lanes,
                                                if target & 1 == 0 {
                                                    ByteLanes::Upper
                                                } else {
                                                    ByteLanes::Lower
                                                }
                                            );
                                            let preserved =
                                                if target & 1 == 0 { 0x00ff } else { 0xff00 };
                                            assert_eq!(
                                                bus.words[&access.bus_address] & preserved,
                                                before[&access.bus_address] & preserved
                                            );
                                            assert_eq!(value.is_some(), store);
                                        }
                                        assert!(
                                            !bus.events
                                                .iter()
                                                .any(|event| matches!(event, Event::Idle(..)))
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

#[test]
fn every_ccr_is_preserved_and_completed_transfers_trace_the_resulting_bank_state() {
    for bank in [0, SUPERVISOR] {
        for ccr in 0..32 {
            for long in [false, true] {
                for shape in [(true, 3, 7), (false, 4, 7), (true, 7, 2), (false, 7, 1)] {
                    let (mut cpu, mut bus, _, _) =
                        movem_seed(shape, long, 0xffff, bank | TRACE | 0x0700 | ccr, 0x1810);
                    cpu.step_instruction(&mut bus).unwrap();
                    let completed = cpu.registers;
                    assert_eq!(completed.status_register(), bank | TRACE | 0x0700 | ccr);
                    let sp = completed.ssp;
                    assert_eq!(cpu.service_pending_trace(&mut bus).unwrap(), Some(34));
                    assert_eq!(
                        bus.words[&(sp.wrapping_sub(6) & BUS_ADDRESS_MASK)],
                        completed.status_register()
                    );
                    assert_eq!(
                        bus.words[&(sp.wrapping_sub(2) & BUS_ADDRESS_MASK)],
                        completed.pc as u16
                    );
                }
                for store in [false, true] {
                    let opcode =
                        0x010f | if long { 0x40 } else { 0 } | if store { 0x80 } else { 0 };
                    let (mut cpu, mut bus, _) = movep_seed(opcode, bank | TRACE | ccr, 0xfffe, 0);
                    cpu.step_instruction(&mut bus).unwrap();
                    let completed = cpu.registers;
                    assert_eq!(completed.status_register(), bank | TRACE | ccr);
                    assert_eq!(cpu.service_pending_trace(&mut bus).unwrap(), Some(34));
                    assert_eq!(bus.words[&(completed.ssp - 6)], completed.status_register());
                }
            }
        }
    }
}

#[test]
fn movem_self_modification_keeps_next_sample_and_refetches_tail_after_writes() {
    for long in [false, true] {
        let (mut cpu, mut bus, _, cursor) = movem_seed(
            (false, 2, 0),
            long,
            if long { 1 } else { 3 },
            SUPERVISOR,
            0x1810,
        );
        cpu.registers.set_address_register(0, cursor);
        cpu.registers.d[0] = if long { 0xbabe_cafe } else { 0xbabe };
        cpu.registers.d[1] = 0xcafe;
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(bus.words[&(cursor & BUS_ADDRESS_MASK)], 0xbabe);
        assert_eq!(bus.words[&((cursor + 2) & BUS_ADDRESS_MASK)], 0xcafe);
        assert_eq!(cpu.queued_words(), Some([0x7201, 0xcafe]));
    }
    let (mut cpu, mut bus, _, cursor) = movem_seed((false, 7, 1), true, 1, SUPERVISOR, 0x1810);
    let pc = cpu.registers.pc;
    bus.words
        .insert((pc + 4) & BUS_ADDRESS_MASK, (cursor >> 16) as u16);
    bus.words.insert((pc + 6) & BUS_ADDRESS_MASK, cursor as u16);
    cpu.registers.d[0] = 0xbabe_cafe;
    cpu.step_instruction(&mut bus).unwrap();
    assert_eq!(cpu.queued_words(), Some([0x7201, 0xcafe]));
    assert_eq!(
        transfers(&bus)
            .iter()
            .map(|(access, _)| access.logical_address)
            .collect::<Vec<_>>(),
        [pc + 4, pc + 6, pc + 8, cursor, cursor + 2, cursor + 2]
    );
    for long in [false, true] {
        let opcode = 0x0188 | if long { 0x40 } else { 0 };
        let (mut cpu, mut bus, _) = movep_seed(opcode, SUPERVISOR, 0, 0);
        let pc = cpu.registers.pc;
        cpu.registers.set_address_register(0, pc + 4);
        cpu.registers.d[0] = if long { 0xbabe_cafe } else { 0xbabe };
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(cpu.queued_words(), Some([0x7201, 0xbe02]));
        assert_eq!(bus.words[&((pc + 4) & BUS_ADDRESS_MASK)], 0xba01);
    }
}

#[test]
fn movem_and_movep_preserve_logical_wrap_and_external_lane_masking() {
    for bank in [0, SUPERVISOR] {
        let (mut cpu, mut bus, _, _) = movem_seed((true, 3, 7), true, 1, bank, 0x1810);
        cpu.registers.set_address_register(7, 0xffff_fffe);
        bus.words
            .extend([(0xfffffe, 0x1234), (0, 0x5678), (2, 0xabcd)]);
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(cpu.registers.d[0], 0x1234_5678);
        assert_eq!(cpu.registers.stack_pointer(), 2);
        let accesses = transfers(&bus);
        assert_eq!(accesses[1].0.logical_address, 0xffff_fffe);
        assert_eq!(accesses[2].0.logical_address, 0);
        assert_eq!(accesses[3].0.logical_address, 2);
        let (mut cpu, mut bus, _, _) = movem_seed((false, 4, 7), true, 1, bank, 0x1810);
        cpu.registers.set_address_register(7, 2);
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(cpu.registers.stack_pointer(), 0xffff_fffe);
        assert_eq!(transfers(&bus)[1].0.logical_address, 0);
        assert_eq!(transfers(&bus)[2].0.logical_address, 0xffff_fffe);
        assert_eq!(bus.words[&0], 2);
        assert_eq!(bus.words[&0xfffffe], 0);
        let (mut cpu, mut bus, _) = movep_seed(0x014f, bank, 0, 1);
        cpu.registers.set_address_register(7, 0xffff_ffff);
        bus.words
            .extend([(0xfffffe, 0xab12), (0, 0xcd34), (2, 0xef56), (4, 0x9078)]);
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(cpu.registers.d[0], 0x1234_5678);
        assert_eq!(cpu.registers.stack_pointer(), 0xffff_ffff);
        assert_eq!(
            transfers(&bus)[1..5]
                .iter()
                .map(|(access, _)| access.logical_address)
                .collect::<Vec<_>>(),
            [0xffff_ffff, 1, 3, 5]
        );
    }
}
