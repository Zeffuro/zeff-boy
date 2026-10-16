use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, Event, cpu};

pub(super) fn shapes(source: bool) -> Vec<(u16, usize)> {
    [0, 2, 3, 4, 5, 6, 7]
        .into_iter()
        .flat_map(|mode| {
            (0..if mode == 7 {
                if source { 5 } else { 2 }
            } else {
                8
            })
                .map(move |register| (mode, register))
        })
        .collect()
}

pub(super) fn seed(
    family: u16,
    shape: (u16, usize),
    sr: u16,
    value: u16,
    index: u16,
) -> (PrefetchCpu, Bus, u32, u32) {
    let (mode, register) = shape;
    let mut cpu = cpu(sr);
    for register in 0..7 {
        cpu.registers
            .set_address_register(register, 0x2345_3008 + register as u32 * 16);
    }
    for register in 0..8 {
        cpu.registers.d[register] = 0x8765_0010 + register as u32 * 2;
    }
    let pc = cpu.registers.pc;
    let base = if mode == 7 {
        pc + 2
    } else {
        cpu.registers.address_register(register).unwrap()
    };
    let (extensions, address) = match (mode, register) {
        (0, _) => {
            cpu.registers.d[register] = 0xaabb_0000 | u32::from(value);
            (vec![], 0)
        }
        (2 | 3, _) => (vec![], base),
        (4, _) => (vec![], base - 2),
        (5, _) | (7, 2) => (vec![0x0040], base + 0x40),
        (6, _) | (7, 3) => {
            let index_register = usize::from((index >> 12) & 7);
            let raw = if index & 0x8000 == 0 {
                cpu.registers.d[index_register]
            } else {
                cpu.registers.address_register(index_register).unwrap()
            };
            let signed = if index & 0x0800 == 0 {
                i32::from(raw as i16) as u32
            } else {
                raw
            };
            (
                vec![index],
                base.wrapping_add(signed)
                    .wrapping_add_signed(i32::from(index as u8 as i8)),
            )
        }
        (7, 0) => (vec![0x8030], 0xffff_8030),
        (7, 1) => (vec![0xabcd, 0x3010], 0xabcd_3010),
        (7, 4) => (vec![value], 0),
        _ => unreachable!(),
    };
    let cursor = pc + 2 + extensions.len() as u32 * 2;
    let words: Vec<_> = extensions.into_iter().chain([0x7201, 0x7402]).collect();
    let opcode = family | (mode << 3) | register as u16;
    cpu.pipeline.as_mut().unwrap().words = [opcode, words[0]];
    let mut bus = Bus::default();
    bus.handler(8);
    bus.handler(9);
    for (offset, &word) in words.iter().enumerate() {
        bus.words
            .insert((pc + 2 + offset as u32 * 2) & BUS_ADDRESS_MASK, word);
    }
    if mode != 0 && (mode != 7 || register != 4) {
        bus.words.insert(address & BUS_ADDRESS_MASK, value);
    }
    (cpu, bus, address, cursor)
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

pub(super) fn compare(cpu: &mut PrefetchCpu, bus: &mut Bus) {
    let mut raw = crate::test_bus::TestBus::default();
    for (&address, &word) in &bus.words {
        raw.word(address, word);
    }
    let mut expected = cpu.registers;
    let pc = expected.pc;
    let cycles =
        crate::instruction::execute(&mut expected, &mut raw, pc, cpu.pipeline.unwrap().words[0])
            .unwrap()
            .nominal_cycles;
    let initial = cpu.registers;
    let opcode = cpu.pipeline.unwrap().words[0];
    let result = cpu.step_instruction(bus).unwrap();
    assert_eq!(
        cpu.registers,
        expected,
        "{opcode:04x}/{:04x}",
        initial.status_register()
    );
    assert_eq!(
        result.clocks,
        u64::from(cycles) + bus.transfers as u64 * u64::from(bus.wait)
    );
    let privilege = opcode & 0xffc0 == 0x46c0 && !initial.supervisor();
    assert_eq!(result.exception, if privilege { Some(8) } else { None });
    assert_eq!(
        cpu.trace_pending(),
        initial.status_register() & TRACE != 0 && !privilege
    );
    for (&address, &word) in &bus.words {
        assert_eq!(
            word,
            u16::from_be_bytes([raw.peek(address), raw.peek(address.wrapping_add(1))])
        );
    }
    let mut clock = 0;
    for event in &bus.events {
        match event {
            Event::Read(_, start) | Event::Write(_, _, start) => {
                assert_eq!(*start, clock);
                clock += 4 + u64::from(bus.wait);
            }
            Event::Idle(start, clocks) => {
                assert_eq!(*start, clock);
                assert_ne!(*clocks, 0);
                clock += u64::from(*clocks);
            }
            Event::Ack(_, _) => panic!("unexpected acknowledge"),
        }
    }
    assert_eq!(bus.clock, clock);
}

#[test]
fn status_routes_match_complete_state_all_legal_eas_banks_indices_ccrs_and_waits() {
    for family in [0x40c0, 0x44c0, 0x46c0] {
        for shape in shapes(family != 0x40c0) {
            for supervisor in [0, SUPERVISOR] {
                for trace in [0, TRACE] {
                    for ccr in 0..32 {
                        for wait in [0, 1, 3] {
                            for index in [0x1010, 0x1910, 0xf810, 0xff10] {
                                let (mut cpu, mut bus, _, _) = seed(
                                    family,
                                    shape,
                                    supervisor | trace | 0x0700 | ccr,
                                    0x851f ^ ccr,
                                    index,
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
fn each_status_schedule_has_exact_extensions_data_spaces_refetches_and_word_lanes() {
    for family in [0x40c0, 0x44c0, 0x46c0] {
        for (mode, register) in shapes(family != 0x40c0) {
            let (mut cpu, mut bus, address, cursor) =
                seed(family, (mode, register), 0x2710, 0x001f, 0x1910);
            let pc = cpu.registers.pc;
            cpu.step_instruction(&mut bus).unwrap();
            let entry_program = FunctionCode::SupervisorProgram;
            let entry_data = if mode == 7 && matches!(register, 2 | 3) {
                entry_program
            } else {
                FunctionCode::SupervisorData
            };
            let mut expected = Vec::new();
            if mode >= 5 {
                expected.push((pc + 4, AccessKind::Prefetch, entry_program, None));
            }
            if mode == 7 && register == 1 {
                expected.push((pc + 6, AccessKind::Prefetch, entry_program, None));
            }
            if mode != 0 && (mode != 7 || register != 4) {
                expected.push((address, AccessKind::Data, entry_data, None));
            }
            if family == 0x40c0 {
                expected.push((cursor + 2, AccessKind::Prefetch, entry_program, None));
                if mode != 0 {
                    expected.push((
                        address,
                        AccessKind::Data,
                        FunctionCode::SupervisorData,
                        Some(0x2710),
                    ));
                }
            } else {
                let fc = if family == 0x46c0 {
                    FunctionCode::UserProgram
                } else {
                    entry_program
                };
                expected.push((cursor, AccessKind::Prefetch, fc, None));
                expected.push((cursor + 2, AccessKind::Prefetch, fc, None));
            }
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
            assert_eq!(actual, expected, "{family:04x}/{mode}/{register}");
            let idles: Vec<_> = bus
                .events
                .iter()
                .enumerate()
                .filter_map(|(index, event)| match event {
                    Event::Idle(_, clocks) => Some((index, *clocks)),
                    _ => None,
                })
                .collect();
            let mut expected_idle = Vec::new();
            if mode == 4 || mode == 6 || mode == 7 && register == 3 {
                expected_idle.push((0, 2));
            }
            if family == 0x40c0 && mode == 0 {
                expected_idle.push((1, 2));
            }
            if family != 0x40c0 {
                let index = expected.len() - 2 + expected_idle.len();
                expected_idle.extend([(index, 2), (index + 1, 2)]);
            }
            assert_eq!(idles, expected_idle);
            assert_eq!(cpu.registers.pc, cursor);
            assert_eq!(cpu.queued_words(), Some([0x7201, 0x7402]));
        }
    }
}

#[test]
fn all_status_input_words_mask_reserved_bits_and_preserve_dn_parts() {
    for value in 0..=u16::MAX {
        for family in [0x40c0, 0x44c0, 0x46c0] {
            let (mut cpu, mut bus, _, _) =
                seed(family, (0, 7), SUPERVISOR | TRACE | 0x071a, value, 0x1010);
            if family == 0x40c0 {
                cpu.registers.set_status_register(value);
            }
            let sr = cpu.registers.status_register();
            let upper = cpu.registers.d[7] & 0xffff_0000;
            cpu.step_instruction(&mut bus).unwrap();
            if family == 0x40c0 {
                assert_eq!(cpu.registers.d[7], upper | u32::from(sr));
                assert_eq!(cpu.registers.status_register(), sr);
            } else {
                assert_eq!(cpu.registers.d[7], upper | u32::from(value));
                assert_eq!(
                    cpu.registers.status_register(),
                    if family == 0x44c0 {
                        (sr & !0x1f) | (value & 0x1f)
                    } else {
                        value & 0xa71f
                    }
                );
            }
        }
    }
}

#[test]
fn status_refetch_samples_fresh_words_and_bank_switch_preserves_old_a7_effects() {
    for (mode, increment) in [(3, 2_i32), (4, -2)] {
        let (mut cpu, mut bus, _, cursor) = seed(0x46c0, (mode, 7), 0xa71f, 0x0010, 0x1010);
        let old_ssp = cpu.registers.ssp;
        let old_usp = cpu.registers.usp;
        let final_index = 1;
        bus.read_values
            .extend([(final_index, 0x7603), (final_index + 1, 0x7804)]);
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(cpu.registers.ssp, old_ssp.wrapping_add_signed(increment));
        assert_eq!(cpu.registers.usp, old_usp);
        assert_eq!(cpu.registers.stack_pointer(), old_usp);
        assert_eq!(cpu.queued_words(), Some([0x7603, 0x7804]));
        assert_eq!(cpu.registers.pc, cursor);
        assert!(cpu.trace_pending());
        assert_eq!(
            transfers(&bus)[final_index].0.function_code,
            FunctionCode::UserProgram
        );
    }
    for family in [0x44c0, 0x46c0] {
        let (mut cpu, mut bus, _, _) = seed(family, (7, 4), SUPERVISOR, 0xffff, 0x1010);
        bus.read_values
            .extend([(0, 0x7201), (1, 0x7603), (2, 0x7804)]);
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(cpu.queued_words(), Some([0x7603, 0x7804]));
        assert_eq!(
            transfers(&bus)
                .iter()
                .map(|(access, _)| access.logical_address)
                .collect::<Vec<_>>(),
            [0x1234_0104, 0x1234_0104, 0x1234_0106]
        );
    }
    let (mut cpu, mut bus, _, _) = seed(0x40c0, (3, 7), 0x001a, 0xabcd, 0x1010);
    let old_usp = cpu.registers.usp;
    cpu.step_instruction(&mut bus).unwrap();
    assert_eq!(bus.words[&old_usp], 0x001a);
    assert_eq!(cpu.registers.usp, old_usp + 2);
    assert!(!bus.words.contains_key(&(old_usp + 2)));
}

#[test]
fn status_trace_uses_entry_t_and_saves_the_completed_status_and_instruction_cursor() {
    for family in [0x40c0, 0x44c0, 0x46c0] {
        for shape in [(0, 7), (3, 7), (4, 7), (7, 1), (7, 2), (7, 4)] {
            if family == 0x40c0 && shape.0 == 7 && shape.1 > 1 {
                continue;
            }
            for (old, new, traced) in [
                (0xa710, 0x0001, true),
                (0x2710, 0xa001, false),
                (0xa710, 0xa001, true),
            ] {
                let (mut cpu, mut bus, _, _) = seed(family, shape, old, new, 0x1010);
                cpu.step_instruction(&mut bus).unwrap();
                let completed = cpu.registers;
                assert_eq!(cpu.trace_pending(), traced);
                assert_eq!(
                    cpu.service_pending_trace(&mut bus).unwrap(),
                    if traced { Some(34) } else { None }
                );
                if traced {
                    let sp = completed.ssp;
                    assert_eq!(
                        bus.words[&(sp.wrapping_sub(6) & BUS_ADDRESS_MASK)],
                        completed.status_register()
                    );
                    assert_eq!(
                        bus.words[&(sp.wrapping_sub(2) & BUS_ADDRESS_MASK)],
                        completed.pc as u16
                    );
                    assert_eq!(cpu.registers.ssp, sp.wrapping_sub(6));
                    assert!(!cpu.trace_pending());
                }
            }
        }
    }
}
