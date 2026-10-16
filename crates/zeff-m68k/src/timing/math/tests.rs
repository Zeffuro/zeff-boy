use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::{Cpu, EXTEND, NEGATIVE, SUPERVISOR, ZERO};

pub(super) fn shapes() -> Vec<(u16, u16)> {
    (0..8)
        .filter(|mode| *mode != 1)
        .flat_map(|mode| (0..if mode == 7 { 5 } else { 8 }).map(move |reg| (mode, reg)))
        .collect()
}

pub(super) fn seed(
    opcode: u16,
    status: u16,
    source: u16,
    dividend: u32,
) -> (PrefetchCpu, Bus, u32) {
    let mode = (opcode >> 3) & 7;
    let register = usize::from(opcode & 7);
    let destination = usize::from((opcode >> 9) & 7);
    let mut timed = cpu(status);
    for index in 0..8 {
        timed.registers.d[index] = 0xbeef_0000 + index as u32;
        timed
            .registers
            .set_address_register(index, 0x2000_3008 + index as u32 * 0x100);
    }
    timed.registers.d[destination] = dividend;
    let base = timed.registers.address_register(register).unwrap();
    let pc = timed.registers.pc;
    let (extensions, address) = match (mode, register) {
        (0, _) => {
            timed.registers.d[register] =
                (timed.registers.d[register] & 0xffff_0000) | u32::from(source);
            (vec![], 0)
        }
        (2 | 3, _) => (vec![], base),
        (4, _) => (vec![], base - 2),
        (5, _) => (vec![0xfffe], base - 2),
        (6, _) => (
            vec![0x8002],
            base + timed.registers.address_register(0).unwrap() as u16 as i16 as i32 as u32 + 2,
        ),
        (7, 0) => (vec![0x8030], 0xffff_8030),
        (7, 1) => (vec![0xabcd, 0x3010], 0xabcd_3010),
        (7, 2) => (vec![0x0200], pc + 0x202),
        (7, 3) => (
            vec![0x8032],
            pc + 0x34 + timed.registers.address_register(0).unwrap() as u16 as i16 as i32 as u32,
        ),
        (7, 4) => (vec![source], 0),
        _ => unreachable!(),
    };
    let words: Vec<_> = extensions.into_iter().chain([0x7201, 0x7402]).collect();
    timed.pipeline.as_mut().unwrap().words = [opcode, words[0]];
    let mut bus = Bus::default();
    bus.words.insert(pc & BUS_ADDRESS_MASK, opcode);
    for (index, &word) in words.iter().enumerate() {
        bus.words
            .insert((pc + 2 + index as u32 * 2) & BUS_ADDRESS_MASK, word);
    }
    if mode != 0 && !(mode == 7 && register == 4) {
        bus.words.insert(address & BUS_ADDRESS_MASK, source);
    }
    bus.handler(5);
    bus.handler(6);
    bus.handler(9);
    bus.handler(27);
    (timed, bus, address)
}

#[test]
fn every_legal_ea_destination_bank_trace_and_wait_matches_full_architectural_result() {
    for family in [0xc0c0, 0xc1c0, 0x80c0, 0x81c0, 0x4180] {
        for (mode, register) in shapes() {
            for destination in 0..8 {
                let opcode = family | destination << 9 | mode << 3 | register;
                for (source, dividend) in [
                    (0, 0x0000_1234),
                    (1, 0x8000_0000),
                    (3, 7),
                    (0xffff, 0x7fff_ffff),
                    (0x8000, 0xffff_8000),
                    (0x5555, 0x10000),
                ] {
                    for status in [
                        0x1f,
                        TRACE | 0x1f,
                        SUPERVISOR | 0x1f,
                        SUPERVISOR | TRACE | 0x1f,
                    ] {
                        for wait in [0, 1, 3] {
                            let (mut timed, mut bus, _) = seed(opcode, status, source, dividend);
                            let mut raw = crate::test_bus::TestBus::default();
                            for (&address, &word) in &bus.words {
                                raw.word(address, word);
                            }
                            let mut architectural = Cpu {
                                registers: timed.registers,
                                stopped: false,
                            };
                            let expected = crate::instruction::execute(
                                &mut architectural.registers,
                                &mut raw,
                                timed.registers.pc,
                                opcode,
                            )
                            .unwrap();
                            bus.wait = wait;
                            let step = timed.step_instruction(&mut bus).unwrap();
                            assert_eq!(
                                step.clocks,
                                u64::from(expected.nominal_cycles)
                                    + bus.transfers as u64 * u64::from(wait),
                                "op={opcode:04x}"
                            );
                            assert_eq!(step.exception, expected.exception);
                            assert_eq!(timed.registers, architectural.registers, "op={opcode:04x}");
                            for (&address, &word) in &bus.words {
                                assert_eq!(
                                    word,
                                    u16::from_be_bytes([raw.peek(address), raw.peek(address + 1)])
                                );
                            }
                            assert_eq!(
                                timed.queued_words(),
                                Some(if step.exception.is_some() {
                                    [0x4e71, 0x7201]
                                } else {
                                    [0x7201, 0x7402]
                                })
                            );
                            assert_eq!(timed.trace_pending(), status & TRACE != 0);
                            assert_eq!(timed.clock().0, bus.clock);
                            for event in &bus.events {
                                if let Event::Read(access, _) | Event::Write(access, _, _) = event {
                                    assert_eq!(access.lanes, ByteLanes::Both);
                                    assert_eq!(
                                        access.bus_address,
                                        access.logical_address & BUS_ADDRESS_MASK
                                    );
                                    let program = access.kind == AccessKind::Prefetch
                                        || access.kind == AccessKind::Data
                                            && mode == 7
                                            && matches!(register, 2 | 3);
                                    let supervisor = status & SUPERVISOR != 0
                                        || matches!(
                                            access.kind,
                                            AccessKind::Stack | AccessKind::Vector
                                        )
                                        || step.exception.is_some()
                                            && access.logical_address == 0x1234_5000
                                        || step.exception.is_some()
                                            && access.logical_address == 0x1234_5002;
                                    assert_eq!(
                                        access.function_code,
                                        match (program, supervisor) {
                                            (true, true) => FunctionCode::SupervisorProgram,
                                            (true, false) => FunctionCode::UserProgram,
                                            (false, true) => FunctionCode::SupervisorData,
                                            (false, false) => FunctionCode::UserData,
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
fn selected_flags_overflow_and_variable_internal_clocks_have_independent_examples() {
    for (family, source, dividend, result, flags, cycles) in [
        (0xc0c0, 0, 0xdead_beef, 0, ZERO, 38),
        (0xc0c0, 0xffff, 0xdead_ffff, 0xfffe_0001, NEGATIVE, 70),
        (0xc1c0, 0xffff, 0x1234_8000, 0x8000, 0, 40),
        (0xc1c0, 0x5555, 3, 0xffff, 0, 70),
        (0x80c0, 3, 7, 0x0001_0002, 0, 134),
        (0x80c0, 1, 0x10000, 0x10000, NEGATIVE | crate::OVERFLOW, 10),
        (0x81c0, 3, 7, 0x0001_0002, 0, 148),
        (0x81c0, 3, 0xffff_fff9, 0xffff_fffe, NEGATIVE, 154),
        (
            0x81c0,
            1,
            0x8000_0000,
            0x8000_0000,
            NEGATIVE | crate::OVERFLOW,
            18,
        ),
        (0x81c0, 1, 0x8000, 0x8000, NEGATIVE | crate::OVERFLOW, 148),
    ] {
        let (mut timed, mut bus, _) = seed(family | 0x10, SUPERVISOR | 0x1f, source, dividend);
        let step = timed.step_instruction(&mut bus).unwrap();
        assert_eq!(timed.registers.d[0], result, "family={family:04x}");
        assert_eq!(
            timed.registers.status_register(),
            SUPERVISOR | EXTEND | flags
        );
        assert_eq!(step.clocks, cycles + 4);
        let idle = Event::Idle(
            if matches!(family, 0xc0c0 | 0xc1c0) {
                8
            } else {
                4
            },
            (cycles - 4) as u32,
        );
        assert!(bus.events.contains(&idle), "{family:04x}: {:?}", bus.events);
    }
}

#[test]
fn chk_normal_fast_and_slow_and_zero_divide_save_cursor_without_sequential_fetch() {
    for (family, source, dividend, cycles, vector, flags) in [
        (0x4180, 0, 0, 10, None, ZERO),
        (0x4180, 0, 1, 38, Some(6), 0),
        (0x4180, 0, 0xffff, 40, Some(6), NEGATIVE),
        (0x4180, 0xfffe, 0xffff, 38, Some(6), NEGATIVE),
        (0x4180, 0xffff, 0x8000, 40, Some(6), NEGATIVE),
        (0x4180, 0, 0x8000, 38, Some(6), NEGATIVE),
        (0x4180, 0x7fff, 0x8000, 38, Some(6), NEGATIVE),
        (0x80c0, 0, 0x1234, 38, Some(5), ZERO),
        (0x80c0, 0, 0x0001_0000, 38, Some(5), 0),
        (0x80c0, 0, 0x8000_0000, 38, Some(5), NEGATIVE),
        (0x81c0, 0, 0x8000_1234, 38, Some(5), ZERO),
    ] {
        for (mode, register) in [(0, 1), (2, 0), (7, 1), (7, 4)] {
            let opcode = family | mode << 3 | register;
            let (mut timed, mut bus, _) = seed(opcode, SUPERVISOR | TRACE | 0x1f, source, dividend);
            let old_sp = timed.registers.ssp;
            let old_pc = timed.registers.pc;
            let ea = match (mode, register) {
                (0, _) => 0,
                (2, _) | (7, 4) => 4,
                _ => 12,
            };
            let length = match (mode, register) {
                (7, 1) => 6,
                (7, 4) => 4,
                _ => 2,
            };
            let step = timed.step_instruction(&mut bus).unwrap();
            assert_eq!(step.exception, vector);
            assert_eq!(step.clocks, cycles + ea);
            assert_eq!(timed.registers.d[0], dividend);
            assert!(timed.trace_pending());
            if let Some(vector) = vector {
                let saved_sr = SUPERVISOR | TRACE | EXTEND | flags;
                assert_eq!(
                    bus.words[&((old_sp - 2) & BUS_ADDRESS_MASK)],
                    (old_pc + length) as u16
                );
                assert_eq!(
                    bus.words[&((old_sp - 4) & BUS_ADDRESS_MASK)],
                    (old_pc >> 16) as u16
                );
                assert_eq!(bus.words[&((old_sp - 6) & BUS_ADDRESS_MASK)], saved_sr);
                assert_eq!(timed.registers.status_register(), saved_sr & !TRACE);
                let suffix = &bus.events[bus.events.len() - 9..];
                assert_eq!(suffix[0], Event::Idle(ea, (cycles - 30) as u32));
                for (index, address) in [old_sp - 2, old_sp - 6, old_sp - 4].into_iter().enumerate()
                {
                    assert!(
                        matches!(suffix[index + 1], Event::Write(a, _, _) if a.logical_address == address)
                    );
                }
                assert!(
                    matches!(suffix[4], Event::Read(a, _) if a.logical_address == u32::from(vector) * 4)
                );
                assert!(
                    matches!(suffix[5], Event::Read(a, _) if a.logical_address == u32::from(vector) * 4 + 2)
                );
                assert!(matches!(suffix[6], Event::Read(a, _) if a.logical_address == 0x1234_5000));
                assert!(matches!(suffix[7], Event::Idle(_, 2)));
                assert!(matches!(suffix[8], Event::Read(a, _) if a.logical_address == 0x1234_5002));
                assert!(!bus.events.iter().any(|event| matches!(event, Event::Read(a, _) if a.logical_address == old_pc + length + 2)));
            } else {
                assert_eq!(bus.events[bus.events.len() - 2], Event::Idle(ea, 6));
                assert!(
                    matches!(bus.events.last(), Some(Event::Read(a, _)) if a.logical_address == old_pc + length + 2)
                );
            }
        }
    }
}

#[test]
fn group_two_exception_prefix_retains_trace_then_irq_waits_for_the_second_frame() {
    for opcode in [0x80d0, 0x81d0, 0x4190] {
        for bank in [0, SUPERVISOR] {
            let (mut timed, mut bus, _) = seed(opcode, bank | TRACE | EXTEND, 0, 1);
            let old_sp = timed.registers.ssp;
            let instruction = timed.step_instruction(&mut bus).unwrap();
            assert_eq!(
                instruction.exception,
                Some(if opcode == 0x4190 { 6 } else { 5 })
            );
            assert!(timed.trace_pending());
            timed.interrupt_input().set_level(3);
            assert_eq!(
                timed.step_instruction(&mut bus),
                Err(TimingFault::PendingTrace)
            );
            let first_sr = timed.registers.status_register();
            assert_eq!(
                timed.step_boundary(&mut bus).unwrap(),
                TimedOutcome::Trace { clocks: 34 }
            );
            assert!(!timed.trace_pending());
            assert_eq!(timed.registers.ssp, old_sp - 12);
            assert_eq!(bus.words[&((old_sp - 8) & BUS_ADDRESS_MASK)], 0x5000);
            assert_eq!(bus.words[&((old_sp - 10) & BUS_ADDRESS_MASK)], 0x1234);
            assert_eq!(bus.words[&((old_sp - 12) & BUS_ADDRESS_MASK)], first_sr);
            assert_eq!(
                timed.step_boundary(&mut bus).unwrap(),
                TimedOutcome::Interrupt {
                    level: 3,
                    vector: 27,
                    clocks: 44
                }
            );
            assert_eq!(timed.registers.ssp, old_sp - 18);
            let (mut aggregate, mut other, _) = seed(opcode, bank | TRACE | EXTEND, 0, 1);
            assert_eq!(
                aggregate.step(&mut other).unwrap(),
                TimedStep {
                    opcode,
                    clocks: instruction.clocks + 34,
                    exception: Some(9)
                }
            );
        }
    }
}

#[test]
fn all_ea_prefixes_and_early_multiply_late_divide_tail_orders_are_explicit() {
    for (family, source, dividend, work, exception) in [
        (0xc0c0, 3, 7, 38, false),
        (0xc1c0, 3, 7, 38, false),
        (0x80c0, 3, 7, 130, false),
        (0x81c0, 3, 7, 144, false),
        (0x4180, 3, 2, 6, false),
        (0x80c0, 0, 1, 8, true),
        (0x81c0, 0, 1, 8, true),
        (0x4180, 0, 1, 8, true),
        (0x4180, 0, 0xffff, 10, true),
    ] {
        for (mode, register) in shapes() {
            let destination = if mode == 0 { (register + 1) & 7 } else { 0 };
            let opcode = family | destination << 9 | mode << 3 | register;
            let (mut timed, mut bus, address) = seed(opcode, 0, source, dividend);
            let pc = timed.registers.pc;
            timed.step_instruction(&mut bus).unwrap();
            let actual: String = bus
                .events
                .iter()
                .map(|event| match event {
                    Event::Read(a, _) if a.kind == AccessKind::Prefetch => 'f',
                    Event::Read(_, _) => 'r',
                    Event::Write(..) => 'w',
                    Event::Idle(..) => 'i',
                    _ => panic!(),
                })
                .collect();
            let prefix = match (mode, register) {
                (0, _) => "",
                (2 | 3, _) => "r",
                (4, _) => "ir",
                (5, _) | (7, 0 | 2) => "fr",
                (6, _) | (7, 3) => "ifr",
                (7, 1) => "ffr",
                (7, 4) => "f",
                _ => unreachable!(),
            };
            let tail = if exception {
                "iwwwrrfif"
            } else if matches!(family, 0xc0c0 | 0xc1c0) {
                "fi"
            } else {
                "if"
            };
            assert_eq!(actual, format!("{prefix}{tail}"), "op={opcode:04x}");
            let tail_start = prefix.len();
            let idle_index =
                tail_start + usize::from(!exception && matches!(family, 0xc0c0 | 0xc1c0));
            assert!(matches!(bus.events[idle_index], Event::Idle(_, clocks) if clocks == work));
            if mode != 0 && !(mode == 7 && register == 4) {
                assert!(
                    matches!(bus.events[tail_start - 1], Event::Read(a, _) if a.logical_address == address && a.kind == AccessKind::Data)
                );
            }
            if !exception {
                let extension_bytes = match (mode, register) {
                    (5 | 6, _) | (7, 0 | 2..=4) => 2,
                    (7, 1) => 4,
                    _ => 0,
                };
                let fetch_index = tail_start + usize::from(!matches!(family, 0xc0c0 | 0xc1c0));
                assert!(
                    matches!(bus.events[fetch_index], Event::Read(a, _) if a.logical_address == pc + 4 + extension_bytes)
                );
            }
        }
    }
}
