use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::{Cpu, SUPERVISOR, StepOutcome};

fn opcode(size: Size, source: u16, destination: u16, sr: u16, dr: u16) -> u16 {
    let family = match size {
        Size::Byte => 1,
        Size::Word => 3,
        Size::Long => 2,
    };
    (family << 12) | (dr << 9) | (destination << 6) | (source << 3) | sr
}

fn seeded(op: u16, status: u16, odd_byte: bool) -> (PrefetchCpu, Bus) {
    let mut timed = cpu(status);
    timed.pipeline.as_mut().unwrap().words = [op, 0x7201];
    for i in 0..8 {
        timed.registers.d[i] = 0x8001_fff0 + i as u32;
        timed
            .registers
            .set_address_register(i, 0x3008 + 4 * i as u32 + u32::from(odd_byte));
    }
    let mut bus = Bus::default();
    for address in (0x3000..0x3040).step_by(2) {
        bus.words.insert(address, (address as u16).wrapping_mul(13));
    }
    bus.words.insert(0x340104, 0x7402);
    (timed, bus)
}

#[test]
fn all_bounded_modes_registers_and_stack_banks_match_architectural_state_and_memory() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        for source in 0..=4 {
            for destination in 0..=4 {
                if size == Size::Byte && (source == 1 || destination == 1) {
                    continue;
                }
                for sr in 0..8 {
                    for dr in 0..8 {
                        for status in [0x1f, SUPERVISOR | 0x1f] {
                            for odd in [false, true] {
                                if odd && size != Size::Byte {
                                    continue;
                                }
                                let op = opcode(size, source, destination, sr, dr);
                                let (mut timed, mut bus) = seeded(op, status, odd);
                                let mut raw = crate::test_bus::TestBus::default();
                                for (&address, &word) in &bus.words {
                                    raw.word(address, word);
                                }
                                raw.word(timed.registers.pc, op);
                                let mut architectural = Cpu {
                                    registers: timed.registers,
                                    stopped: false,
                                };
                                let expected = architectural.step(&mut raw);
                                let observed = timed.step_instruction(&mut bus).unwrap();
                                assert!(
                                    matches!(expected, StepOutcome::Executed { nominal_cycles, .. } if u64::from(nominal_cycles) == observed.clocks),
                                    "op={op:04x}"
                                );
                                assert_eq!(timed.registers, architectural.registers, "op={op:04x}");
                                for (&address, &word) in &bus.words {
                                    assert_eq!(
                                        word,
                                        u16::from_be_bytes([
                                            raw.peek(address),
                                            raw.peek(address + 1)
                                        ]),
                                        "op={op:04x}, address={address:x}"
                                    );
                                }
                                assert_eq!(timed.queued_words(), Some([0x7201, 0x7402]));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn memory_schedule_distinguishes_predecrement_destination_and_long_word_order() {
    for size in [Size::Byte, Size::Word, Size::Long] {
        for source in 0..=4 {
            for destination in 0..=4 {
                if size == Size::Byte && (source == 1 || destination == 1) {
                    continue;
                }
                for wait in [0, 1, 3] {
                    let op = opcode(size, source, destination, 0, 1);
                    let (mut timed, mut bus) = seeded(op, 0x201f, false);
                    bus.wait = wait;
                    timed.step_instruction(&mut bus).unwrap();
                    let mut expected = Vec::new();
                    if source == 4 {
                        expected.push(('i', 2));
                    }
                    if source >= 2 {
                        expected.push(('r', 4 + wait));
                        if size == Size::Long {
                            expected.push(('r', 4 + wait));
                        }
                    }
                    if destination == 4 {
                        expected.push(('f', 4 + wait));
                    }
                    if destination >= 2 {
                        expected.push(('w', 4 + wait));
                        if size == Size::Long {
                            expected.push(('w', 4 + wait));
                        }
                    }
                    if destination != 4 {
                        expected.push(('f', 4 + wait));
                    }
                    let actual: Vec<_> = bus
                        .events
                        .iter()
                        .map(|e| match e {
                            Event::Idle(_, n) => ('i', *n),
                            Event::Read(a, _) => (
                                if a.kind == AccessKind::Prefetch {
                                    'f'
                                } else {
                                    'r'
                                },
                                4 + wait,
                            ),
                            Event::Write(_, _, _) => ('w', 4 + wait),
                            _ => panic!(),
                        })
                        .collect();
                    assert_eq!(actual, expected, "op={op:04x}");
                    let writes: Vec<_> = bus
                        .events
                        .iter()
                        .filter_map(|e| match e {
                            Event::Write(a, _, _) => Some(a.logical_address),
                            _ => None,
                        })
                        .collect();
                    if size == Size::Long && destination >= 2 {
                        assert_eq!(
                            writes[1],
                            writes[0].wrapping_add_signed(if destination == 4 { -2 } else { 2 })
                        );
                    }
                    let reads: Vec<_> = bus
                        .events
                        .iter()
                        .filter_map(|e| match e {
                            Event::Read(a, _) if a.kind == AccessKind::Data => {
                                Some(a.logical_address)
                            }
                            _ => None,
                        })
                        .collect();
                    if size == Size::Long && source >= 2 {
                        assert_eq!(reads[1], reads[0].wrapping_add(2));
                    }
                    for e in &bus.events {
                        if let Event::Read(a, _) | Event::Write(a, _, _) = e {
                            assert_eq!(
                                a.function_code,
                                if a.kind == AccessKind::Data {
                                    FunctionCode::SupervisorData
                                } else {
                                    FunctionCode::SupervisorProgram
                                }
                            );
                        }
                    }
                    assert_eq!(bus.clock, timed.clock().0);
                }
            }
        }
    }
}

#[test]
fn self_modifying_tail_observes_instruction_specific_fetch_order() {
    for mode in [2, 4] {
        let (mut timed, mut bus) = seeded(opcode(Size::Word, 0, mode, 0, 1), 0x2000, false);
        timed.registers.a[1] = timed
            .registers
            .pc
            .wrapping_add(if mode == 4 { 6 } else { 4 });
        timed.registers.d[0] = 0xbabe;
        timed.step(&mut bus).unwrap();
        assert_eq!(bus.words[&0x340104], 0xbabe);
        assert_eq!(
            timed.queued_words(),
            Some([0x7201, if mode == 4 { 0x7402 } else { 0xbabe }])
        );
    }
}

#[test]
fn every_transfer_failure_retains_completed_writes_and_freezes_without_retry() {
    for source in 0..=4 {
        for destination in 0..=4 {
            let op = opcode(Size::Long, source, destination, 0, 0);
            let (mut complete, mut full) = seeded(op, 0xa01f, false);
            full.wait = 1;
            complete.step_instruction(&mut full).unwrap();
            for fail in 0..full.transfers {
                let (mut timed, mut bus) = seeded(op, 0xa01f, false);
                bus.wait = 1;
                bus.fail = Some(fail);
                assert!(
                    matches!(timed.step_instruction(&mut bus), Err(TimingFault::Bus {source, ..}) if source == fail)
                );
                assert_eq!(bus.transfers, fail + 1);
                assert_eq!(bus.clock, timed.clock().0);
                assert!(!timed.trace_pending());
                let mut expected = seeded(op, 0xa01f, false).1.words;
                let mut seen = 0;
                for e in &full.events {
                    if let Event::Read(_, _) | Event::Write(_, _, _) = e {
                        if seen == fail {
                            break;
                        }
                        if let Event::Write(a, value, _) = e {
                            expected.insert(a.bus_address, *value);
                        }
                        seen += 1;
                    }
                }
                assert_eq!(bus.words, expected, "op={op:04x}, fail={fail}");
                let saved = bus.events.clone();
                assert_eq!(timed.step_boundary(&mut bus), Err(TimingFault::Frozen));
                assert_eq!(bus.events, saved);
            }
        }
    }
}

#[test]
fn illegal_operand_forms_have_no_bus_effects() {
    for op in [
        0x1008, 0x1040, 0x303d, 0x303e, 0x303f, 0x35c0, 0x37c0, 0x39c0,
    ] {
        let (mut timed, mut bus) = seeded(op, 0x2000, false);
        let saved = timed.clone();
        assert_eq!(
            timed.step(&mut bus),
            Err(TimingFault::UnsupportedInstruction {
                pc: timed.registers.pc,
                opcode: op
            })
        );
        assert_eq!(timed, saved);
        assert!(bus.events.is_empty());
    }
}

#[test]
fn odd_word_fault_and_external_wrap_are_explicit() {
    let (mut timed, mut bus) = seeded(0x3010, 0x2000, false);
    timed.registers.a[0] = 0x0100_0001;
    assert_eq!(
        timed.step(&mut bus),
        Err(TimingFault::AddressError {
            address: 0x0100_0001
        })
    );
    assert!(bus.events.is_empty());
    assert_eq!(timed.step(&mut bus), Err(TimingFault::Frozen));
    let (mut timed, mut bus) = seeded(0x2010, 0, false);
    timed.registers.a[0] = 0xffff_fffe;
    bus.words.insert(0xfffffe, 0x1234);
    bus.words.insert(0, 0x5678);
    timed.step(&mut bus).unwrap();
    assert_eq!(timed.registers.d[0], 0x1234_5678);
    assert!(
        matches!(bus.events[0], Event::Read(a, 0) if a.logical_address == 0xffff_fffe && a.bus_address == 0xfffffe && a.function_code == FunctionCode::UserData)
    );
    assert!(
        matches!(bus.events[1], Event::Read(a, 4) if a.logical_address == 0 && a.bus_address == 0)
    );
}

#[test]
fn host_failures_preserve_address_updates_at_their_verified_transfer_stage() {
    for status in [0x1f, SUPERVISOR | 0x1f] {
        for size in [Size::Byte, Size::Word, Size::Long] {
            let increment = step(size, 7);
            for fail in 0..if size == Size::Long { 2 } else { 1 } {
                let op = opcode(size, 3, 0, 7, 0);
                let (mut timed, mut bus) = seeded(op, status, false);
                let old = timed.registers.stack_pointer();
                bus.fail = Some(fail);
                assert!(matches!(timed.step(&mut bus), Err(TimingFault::Bus { .. })));
                let updated = size != Size::Long || fail == 1;
                assert_eq!(
                    timed.registers.stack_pointer(),
                    old + if updated { increment } else { 0 }
                );
            }
            for mode in [3, 4] {
                let transfers = if size == Size::Long { 3 } else { 2 };
                for fail in 0..transfers {
                    let op = opcode(size, 0, mode, 0, 7);
                    let (mut timed, mut bus) = seeded(op, status, false);
                    let old = timed.registers.stack_pointer();
                    let initial = timed.registers;
                    bus.fail = Some(fail);
                    assert!(matches!(timed.step(&mut bus), Err(TimingFault::Bus { .. })));
                    let updated = if mode == 3 {
                        fail == transfers - 1
                    } else {
                        fail >= if size == Size::Long { 2 } else { 1 }
                    };
                    assert_eq!(
                        timed.registers.stack_pointer(),
                        if !updated {
                            old
                        } else if mode == 3 {
                            old + increment
                        } else {
                            old - increment
                        },
                        "op={op:04x}, fail={fail}"
                    );
                    assert_eq!(timed.registers.d, initial.d);
                    assert_eq!(timed.registers.pc, initial.pc);
                    assert_eq!(
                        timed.registers.usp,
                        if status & SUPERVISOR != 0 {
                            initial.usp
                        } else {
                            timed.registers.stack_pointer()
                        }
                    );
                    assert_eq!(
                        timed.registers.ssp,
                        if status & SUPERVISOR == 0 {
                            initial.ssp
                        } else {
                            timed.registers.stack_pointer()
                        }
                    );
                    if size == Size::Long && mode == 4 && fail == 2 {
                        assert_eq!(bus.words[&(old - 2)], initial.d[0] as u16);
                        assert_eq!(
                            bus.words[&(old - 4)],
                            seeded(op, status, false).1.words[&(old - 4)]
                        );
                    }
                }
            }
        }
        let (mut timed, mut bus) = seeded(0x2020, status, false);
        let old = timed.registers.a[0];
        bus.fail = Some(0);
        assert!(matches!(timed.step(&mut bus), Err(TimingFault::Bus { .. })));
        assert_eq!(timed.registers.a[0], old - 4);
        assert_eq!(bus.events[0], Event::Idle(0, 2));
    }
}
