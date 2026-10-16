use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::{Cpu, SUPERVISOR, StepOutcome};

pub(super) fn seed(
    opcode: u16,
    status: u16,
    odd: bool,
    values: (u32, u32),
) -> (PrefetchCpu, Bus, u32) {
    let size = crate::instruction::size(opcode).unwrap();
    let mode = (opcode >> 3) & 7;
    let register = usize::from(opcode & 7);
    let (old, immediate) = values;
    let mut timed = cpu(status);
    timed.registers.d[1] = 0xbeef_fffe;
    for i in 0..8 {
        timed
            .registers
            .set_address_register(i, 0x2000_3008 + i as u32 * 4 + u32::from(odd));
    }
    let base = timed.registers.address_register(register).unwrap();
    let increment = if size == Size::Byte && register == 7 {
        2
    } else {
        size.bytes()
    };
    let (extensions, address) = match mode {
        0 => {
            timed.registers.d[register] = old;
            (vec![], 0)
        }
        2 | 3 => (vec![], base),
        4 => (vec![], base.wrapping_sub(increment)),
        5 => (vec![0xfffe], base.wrapping_sub(2)),
        6 => (vec![0x1702], base),
        7 if register == 0 => (vec![0x8030 + u16::from(odd)], 0xffff_8030 + u32::from(odd)),
        7 => (
            vec![0xabcd, 0x3010 + u16::from(odd)],
            0xabcd_3010 + u32::from(odd),
        ),
        _ => unreachable!(),
    };
    let immediate = if size == Size::Long {
        vec![(immediate >> 16) as u16, immediate as u16]
    } else {
        vec![immediate as u16]
    };
    let words: Vec<_> = immediate
        .into_iter()
        .chain(extensions)
        .chain([0x7201, 0x7402])
        .collect();
    timed.pipeline.as_mut().unwrap().words = [opcode, words[0]];
    let mut bus = Bus::default();
    let pc = timed.registers.pc & BUS_ADDRESS_MASK;
    bus.words.insert(pc, opcode);
    for (i, word) in words.iter().enumerate() {
        bus.words.insert(pc + 2 + i as u32 * 2, *word);
    }
    if mode != 0 {
        let bus_address = address & BUS_ADDRESS_MASK & !1;
        if size == Size::Byte {
            bus.words.insert(
                bus_address,
                if address & 1 == 0 {
                    ((old as u16 & 0xff) << 8) | 0x5a
                } else {
                    0xa500 | (old as u16 & 0xff)
                },
            );
        } else if size == Size::Long {
            bus.words.insert(bus_address, (old >> 16) as u16);
            bus.words
                .insert(address.wrapping_add(2) & BUS_ADDRESS_MASK, old as u16);
        } else {
            bus.words.insert(bus_address, old as u16);
        }
    }
    (timed, bus, address)
}

#[test]
fn all_families_sizes_modes_registers_banks_lanes_and_waits_match_architectural_work() {
    for family in [0, 0x0200, 0x0400, 0x0600, 0x0a00, 0x0c00] {
        for size in 0..3 {
            for mode in [0, 2, 3, 4, 5, 6, 7] {
                for register in 0..if mode == 7 { 2 } else { 8 } {
                    let opcode = family | size << 6 | mode << 3 | register;
                    for status in [0x1f, SUPERVISOR | 0x1f] {
                        for odd in [false, true] {
                            if odd && (size != 0 || mode == 0) {
                                continue;
                            }
                            for wait in [0, 1, 3] {
                                let (mut timed, mut bus, _) =
                                    seed(opcode, status, odd, (0x8001_fff0, 0x7fff_ffff));
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

#[test]
fn immediate_extensions_precede_ea_work_and_rmw_writes_low_first() {
    for family in [0, 0x0200, 0x0400, 0x0600, 0x0a00, 0x0c00] {
        for size in 0..3 {
            for mode in [0, 2, 3, 4, 5, 6, 7] {
                for register in 0..if mode == 7 { 2 } else { 1 } {
                    let opcode = family | size << 6 | mode << 3 | register;
                    let (mut timed, mut bus, address) =
                        seed(opcode, SUPERVISOR | 0x1f, false, (0x8123_4567, 0x0102_0304));
                    timed.step_instruction(&mut bus).unwrap();
                    let mut expected = if size == 2 { "ff" } else { "f" }.to_owned();
                    expected.push_str(match mode {
                        4 => "i",
                        5 => "f",
                        6 => "if",
                        7 if register == 0 => "f",
                        7 => "ff",
                        _ => "",
                    });
                    if mode != 0 {
                        expected.push_str(if size == 2 { "rr" } else { "r" });
                    }
                    expected.push('f');
                    if mode == 0 && size == 2 {
                        expected.push(if family == 0x0c00 { 'i' } else { 'j' });
                    } else if mode != 0 && family != 0x0c00 {
                        expected.push_str(if size == 2 { "ww" } else { "w" });
                    }
                    let actual: String = bus
                        .events
                        .iter()
                        .map(|event| match event {
                            Event::Idle(_, 2) => 'i',
                            Event::Idle(_, 4) => 'j',
                            Event::Read(a, _) if a.kind == AccessKind::Prefetch => 'f',
                            Event::Read(_, _) => 'r',
                            Event::Write(..) => 'w',
                            _ => panic!(),
                        })
                        .collect();
                    assert_eq!(actual, expected, "op={opcode:04x}");
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
                        if mode != 0 && family != 0x0c00 {
                            if size == 2 {
                                vec![address.wrapping_add(2), address]
                            } else {
                                vec![address]
                            }
                        } else {
                            vec![]
                        }
                    );
                }
            }
        }
    }
}

#[test]
fn ccr_setters_mask_immediate_preserve_banks_and_repeat_sampled_fetch() {
    for opcode in [0x003c, 0x023c, 0x0a3c] {
        for status in [0, SUPERVISOR, crate::TRACE, SUPERVISOR | crate::TRACE] {
            for ccr in 0..32 {
                for immediate in (0..32).chain([0x801f, 0xffff]) {
                    let mut timed = cpu(status | ccr);
                    timed.pipeline.as_mut().unwrap().words = [opcode, immediate];
                    let initial = timed.registers;
                    let mut bus = Bus {
                        wait: 1,
                        read_values: vec![(0, 0x7bad), (1, 0x7201), (2, 0x7402)],
                        ..Bus::default()
                    };
                    assert_eq!(timed.step_instruction(&mut bus).unwrap().clocks, 23);
                    let expected = match opcode {
                        0x003c => ccr | immediate,
                        0x023c => ccr & immediate,
                        _ => ccr ^ immediate,
                    } & 0x1f;
                    assert_eq!(timed.registers.status_register(), status | expected);
                    assert_eq!(timed.registers.ssp, initial.ssp);
                    assert_eq!(timed.registers.usp, initial.usp);
                    assert_eq!(timed.trace_pending(), status & crate::TRACE != 0);
                    assert_eq!(timed.queued_words(), Some([0x7201, 0x7402]));
                    assert_eq!(timed.registers.pc, initial.pc + 4);
                    let events = &bus.events;
                    assert!(
                        matches!(events[0], Event::Read(a, 0) if a.logical_address == initial.pc + 4)
                    );
                    assert_eq!(events[1], Event::Idle(5, 6));
                    assert_eq!(events[2], Event::Idle(11, 2));
                    assert!(
                        matches!(events[3], Event::Read(a, 13) if a.logical_address == initial.pc + 4)
                    );
                    assert!(
                        matches!(events[4], Event::Read(a, 18) if a.logical_address == initial.pc + 6)
                    );
                }
            }
        }
    }
}

#[test]
fn arithmetic_edges_keep_register_parts_and_cmp_preserves_extend() {
    for (opcode, old, immediate, result, flags) in [
        (
            0x0607,
            0xabcd_12ff,
            1,
            0xabcd_1200,
            crate::EXTEND | crate::CARRY | crate::ZERO,
        ),
        (
            0x0647,
            0xabcd_7fff,
            1,
            0xabcd_8000,
            crate::NEGATIVE | crate::OVERFLOW,
        ),
        (0x0447, 0xabcd_8000, 1, 0xabcd_7fff, crate::OVERFLOW),
        (
            0x0c07,
            0xabcd_1201,
            1,
            0xabcd_1201,
            crate::EXTEND | crate::ZERO,
        ),
        (
            0x0a87,
            0x8123_4567,
            0x8123_4567,
            0,
            crate::EXTEND | crate::ZERO,
        ),
    ] {
        let (mut timed, mut bus, _) =
            seed(opcode, SUPERVISOR | crate::EXTEND, false, (old, immediate));
        timed.step_instruction(&mut bus).unwrap();
        assert_eq!(timed.registers.d[7], result);
        assert_eq!(timed.registers.status_register(), SUPERVISOR | flags);
    }
}
