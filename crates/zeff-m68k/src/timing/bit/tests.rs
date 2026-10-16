use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::{Cpu, SUPERVISOR, StepOutcome};

pub(super) fn seed(
    opcode: u16,
    status: u16,
    odd: bool,
    values: (u32, u32),
) -> (PrefetchCpu, Bus, u32) {
    let immediate = opcode & 0xff00 == 0x0800;
    let mode = (opcode >> 3) & 7;
    let register = usize::from(opcode & 7);
    let (old, bit) = values;
    let mut timed = cpu(status);
    timed.registers.d[1] = 0xbeef_fffe;
    timed.registers.d[usize::from((opcode >> 9) & 7)] = bit;
    for i in 0..8 {
        timed
            .registers
            .set_address_register(i, 0x2000_3008 + i as u32 * 4 + u32::from(odd));
    }
    let base = timed.registers.address_register(register).unwrap();
    let cursor = timed
        .registers
        .pc
        .wrapping_add(if immediate { 4 } else { 2 });
    let (extensions, address) = match mode {
        0 => {
            timed.registers.d[register] = old;
            (vec![], 0)
        }
        2 | 3 => (vec![], base),
        4 => (vec![], base.wrapping_sub(if register == 7 { 2 } else { 1 })),
        5 => (vec![0xfffe], base.wrapping_sub(2)),
        6 => (vec![0x1702], base),
        7 => match register {
            0 => (vec![0x8030 + u16::from(odd)], 0xffff_8030 + u32::from(odd)),
            1 => (
                vec![0xabcd, 0x3010 + u16::from(odd)],
                0xabcd_3010 + u32::from(odd),
            ),
            2 => (
                vec![0x0200 + u16::from(odd)],
                cursor.wrapping_add(0x200 + u32::from(odd)),
            ),
            3 => (
                vec![0x1732 + u16::from(odd)],
                cursor.wrapping_add(0x30 + u32::from(odd)),
            ),
            4 => (vec![old as u16], 0),
            _ => unreachable!(),
        },
        _ => unreachable!(),
    };
    let words: Vec<_> = if immediate { vec![bit as u16] } else { vec![] }
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
    if mode != 0 && !(mode == 7 && register == 4) {
        bus.words.insert(
            address & BUS_ADDRESS_MASK & !1,
            if address & 1 == 0 {
                ((old as u16 & 0xff) << 8) | 0x5a
            } else {
                0xa500 | (old as u16 & 0xff)
            },
        );
    }
    (timed, bus, address)
}

pub(super) fn opcodes() -> Vec<u16> {
    let mut values = Vec::new();
    for base in [0x0800, 0x0700] {
        for operation in 0..4 {
            for mode in [0, 2, 3, 4, 5, 6, 7] {
                if base == 0x0700 && mode == 0 {
                    continue;
                }
                let limit = if mode == 7 {
                    if operation == 0 {
                        if base == 0x0800 { 4 } else { 5 }
                    } else {
                        2
                    }
                } else {
                    8
                };
                for register in 0..limit {
                    values.push(base | operation << 6 | mode << 3 | register);
                }
            }
        }
    }
    values
}

#[test]
fn all_bit_shapes_counts_banks_lanes_and_waits_match_architectural_work() {
    for opcode in opcodes() {
        for status in [0x1f, SUPERVISOR | 0x1f] {
            for odd in [false, true] {
                if odd && opcode & 0x38 == 0 {
                    continue;
                }
                for bit in [0, 7, 16, 31, 32, 0xffff] {
                    for wait in [0, 1, 3] {
                        let (mut timed, mut bus, _) = seed(opcode, status, odd, (0x8123_00a5, bit));
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
                            "op={opcode:04x} bit={bit}"
                        );
                        assert_eq!(
                            timed.registers, architectural.registers,
                            "op={opcode:04x} bit={bit}"
                        );
                        for (&address, &word) in &bus.words {
                            assert_eq!(
                                word,
                                u16::from_be_bytes([raw.peek(address), raw.peek(address + 1)])
                            );
                        }
                        assert_eq!(timed.queued_words(), Some([0x7201, 0x7402]));
                        assert_eq!(timed.clock().0, bus.clock);
                        for event in &bus.events {
                            if let Event::Read(a, _) | Event::Write(a, _, _) = event {
                                let program = a.kind == AccessKind::Prefetch
                                    || opcode & 0x38 == 0x38 && matches!(opcode & 7, 2 | 3);
                                assert_eq!(
                                    a.function_code,
                                    if program {
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

#[test]
fn static_register_modulo_and_half_dependent_idle_follow_tail_fetch() {
    for operation in 0..4 {
        for bit in [0, 7, 15, 16, 31, 32, 0xffff] {
            let opcode = 0x0807 | operation << 6;
            let (mut timed, mut bus, _) =
                seed(opcode, SUPERVISOR | 0x1f, false, (0x8001_00a5, bit));
            let old = timed.registers.d[7];
            let mask = 1 << (bit & 31);
            let expected_value = match operation {
                1 => old ^ mask,
                2 => old & !mask,
                3 => old | mask,
                _ => old,
            };
            let idle = if operation == 0 {
                2
            } else {
                (if operation == 2 { 4 } else { 2 }) + if bit & 16 != 0 { 2 } else { 0 }
            };
            let observed = timed.step_instruction(&mut bus).unwrap();
            assert_eq!(observed.clocks, 8 + idle);
            assert_eq!(timed.registers.d[7], expected_value);
            assert_eq!(
                timed.registers.status_register(),
                SUPERVISOR | 0x1b | if old & mask == 0 { crate::ZERO } else { 0 }
            );
            assert_eq!(bus.events.len(), 3);
            assert!(matches!(bus.events[0], Event::Read(a, 0) if a.logical_address == 0x1234_0104));
            assert!(matches!(bus.events[1], Event::Read(a, 4) if a.logical_address == 0x1234_0106));
            assert_eq!(bus.events[2], Event::Idle(8, idle as u32));
        }
    }
}

#[test]
fn memory_prefixes_and_immediate_btst_have_exact_order_and_selected_lane() {
    for opcode in opcodes().into_iter().filter(|opcode| opcode & 0x38 != 0) {
        let (mut timed, mut bus, address) = seed(opcode, SUPERVISOR | 0x1f, true, (0x81, 32));
        let mode = (opcode >> 3) & 7;
        let register = opcode & 7;
        let operation = (opcode >> 6) & 3;
        let mut expected = if opcode & 0xff00 == 0x0800 { "f" } else { "" }.to_owned();
        expected.push_str(match (mode, register) {
            (4, _) => "i",
            (6, _) | (7, 3) => "if",
            (5, _) | (7, 0 | 2 | 4) => "f",
            (7, 1) => "ff",
            _ => "",
        });
        if !(mode == 7 && register == 4) {
            expected.push('r');
        }
        expected.push('f');
        if mode == 7 && register == 4 {
            expected.push('i');
        } else if operation != 0 {
            expected.push('w');
        }
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
        assert_eq!(actual, expected, "op={opcode:04x}");
        let writes: Vec<_> = bus
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Write(a, value, _) => Some((a.logical_address, a.lanes, *value)),
                _ => None,
            })
            .collect();
        if operation == 0 {
            assert!(writes.is_empty());
        } else {
            let value = match operation {
                1 | 2 => 0x80,
                _ => 0x81,
            };
            assert_eq!(
                writes,
                [(
                    address,
                    if address & 1 == 0 {
                        ByteLanes::Upper
                    } else {
                        ByteLanes::Lower
                    },
                    if address & 1 == 0 { value << 8 } else { value }
                )]
            );
        }
    }
}
