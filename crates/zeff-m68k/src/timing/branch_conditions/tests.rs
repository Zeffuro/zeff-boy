use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, Event, cpu};

pub(super) fn branch_seed(opcode: u16, extension: u16, status: u16) -> (PrefetchCpu, Bus) {
    let mut timed = cpu(status);
    timed.pipeline.as_mut().unwrap().words = [opcode, extension];
    let pc = timed.registers.pc;
    let displacement = if opcode & 0xff00 == 0x6100 && opcode as u8 != 0 {
        i32::from(opcode as u8 as i8)
    } else {
        i32::from(extension as i16)
    };
    let target = pc.wrapping_add(2).wrapping_add_signed(displacement);
    let mut bus = Bus::default();
    for (address, word) in [
        (pc, opcode),
        (pc + 2, extension),
        (pc + 4, 0x7201),
        (pc + 6, 0x7402),
    ] {
        bus.words.insert(address & BUS_ADDRESS_MASK, word);
    }
    bus.words.entry(target & BUS_ADDRESS_MASK).or_insert(0x7603);
    bus.words
        .entry(target.wrapping_add(2) & BUS_ADDRESS_MASK)
        .or_insert(0x7804);
    (timed, bus)
}

pub(super) fn oracle(timed: &PrefetchCpu, bus: &Bus) -> (Registers, crate::test_bus::TestBus, u32) {
    let mut raw = crate::test_bus::TestBus::default();
    for (&address, &word) in &bus.words {
        raw.word(address, word);
    }
    let mut registers = timed.registers;
    let execution = crate::instruction::execute(
        &mut registers,
        &mut raw,
        timed.registers.pc,
        timed.pipeline.unwrap().words[0],
    )
    .unwrap();
    (registers, raw, execution.nominal_cycles)
}

pub(super) fn verify(timed: &mut PrefetchCpu, bus: &mut Bus) {
    let (registers, raw, cycles) = oracle(timed, bus);
    let status = timed.registers.status_register();
    let step = timed.step_instruction(bus).unwrap();
    assert_eq!(timed.registers, registers);
    assert_eq!(
        step.clocks,
        u64::from(cycles) + bus.transfers as u64 * u64::from(bus.wait)
    );
    assert_eq!(step.exception, None);
    assert_eq!(timed.trace_pending(), status & TRACE != 0);
    assert_eq!(timed.clock().0, bus.clock);
    for (&address, &word) in &bus.words {
        assert_eq!(
            word,
            u16::from_be_bytes([raw.peek(address), raw.peek(address + 1)])
        );
    }
    let pc = timed.registers.pc;
    assert_eq!(
        timed.queued_words(),
        Some([
            bus.words[&(pc & BUS_ADDRESS_MASK)],
            bus.words[&(pc.wrapping_add(2) & BUS_ADDRESS_MASK)]
        ])
    );
}

pub(super) fn conditions(flags: u16) -> [bool; 16] {
    let c = flags & crate::CARRY != 0;
    let v = flags & crate::OVERFLOW != 0;
    let z = flags & crate::ZERO != 0;
    let n = flags & crate::NEGATIVE != 0;
    [
        true,
        false,
        !c && !z,
        c || z,
        !c,
        c,
        !z,
        z,
        !v,
        v,
        !n,
        n,
        n == v,
        n != v,
        !z && n == v,
        z || n != v,
    ]
}

#[test]
fn bsr_all_byte_and_word_displacements_banks_waits_and_trace_keep_return_and_order() {
    for opcode in 0x6100..=0x61ff {
        for extension in if opcode == 0x6100 {
            vec![0, 2, 0xfffe, 0x8000, 0x7ffe]
        } else {
            vec![0x7201]
        } {
            let displacement = if opcode == 0x6100 {
                i32::from(extension as i16)
            } else {
                i32::from(opcode as u8 as i8)
            };
            for status in [
                0x1f,
                SUPERVISOR | 0x1f,
                TRACE | 0x1f,
                SUPERVISOR | TRACE | 0x1f,
            ] {
                for wait in [0, 1, 3] {
                    let (mut timed, mut bus) = branch_seed(opcode, extension, status);
                    let old_pc = timed.registers.pc;
                    let sp = timed.registers.stack_pointer();
                    bus.wait = wait;
                    if displacement & 1 != 0 {
                        let target = old_pc.wrapping_add(2).wrapping_add_signed(displacement);
                        assert_eq!(
                            timed.step_instruction(&mut bus),
                            Err(TimingFault::AddressError { address: target })
                        );
                        assert_eq!(timed.registers.pc, old_pc);
                        assert_eq!(timed.registers.stack_pointer(), sp - 4);
                        assert_eq!(bus.transfers, 2);
                        assert_eq!(bus.words[&(sp - 4)], (old_pc >> 16) as u16);
                        assert_eq!(bus.words[&(sp - 2)], (old_pc + 2) as u16);
                        assert_eq!(timed.clock().0, 10 + u64::from(wait) * 2);
                        continue;
                    }
                    verify(&mut timed, &mut bus);
                    assert_eq!(timed.registers.stack_pointer(), sp - 4);
                    assert_eq!(bus.words[&(sp - 4)], (old_pc >> 16) as u16);
                    assert_eq!(
                        bus.words[&(sp - 2)],
                        (old_pc + if opcode == 0x6100 { 4 } else { 2 }) as u16
                    );
                    assert!(matches!(bus.events[0], Event::Idle(0, 2)));
                    assert!(
                        matches!(bus.events[1], Event::Write(a, _, _) if a.logical_address == sp - 4 && a.kind == AccessKind::Stack && a.function_code == if status & SUPERVISOR == 0 {FunctionCode::UserData} else {FunctionCode::SupervisorData})
                    );
                    assert!(
                        matches!(bus.events[2], Event::Write(a, _, _) if a.logical_address == sp - 2)
                    );
                    assert!(
                        matches!(bus.events[3], Event::Read(a, _) if a.logical_address == timed.registers.pc)
                    );
                    assert!(
                        matches!(bus.events[4], Event::Read(a, _) if a.logical_address == timed.registers.pc + 2)
                    );
                }
            }
        }
    }
}

#[test]
fn dbcc_all_conditions_flags_registers_counter_paths_and_exact_discarded_fetch() {
    for code in 0..16 {
        for flags in 0..32 {
            for register in 0..8 {
                for counter in [0, 1, 2, 0xffff] {
                    for status in [flags, flags | SUPERVISOR | TRACE] {
                        for wait in [0, 1, 3] {
                            let opcode = 0x50c8 | code << 8 | register;
                            let (mut timed, mut bus) = branch_seed(opcode, 0x20, status);
                            timed.registers.d[usize::from(register)] = 0xabcd_0000 | counter;
                            let pc = timed.registers.pc;
                            bus.wait = wait;
                            verify(&mut timed, &mut bus);
                            let truth = conditions(flags)[usize::from(code)];
                            let addresses: Vec<_> = bus
                                .events
                                .iter()
                                .filter_map(|event| match event {
                                    Event::Read(a, _) => Some(a.logical_address),
                                    _ => None,
                                })
                                .collect();
                            assert_eq!(
                                addresses,
                                if truth {
                                    vec![pc + 4, pc + 6]
                                } else if counter == 0 {
                                    vec![pc + 0x22, pc + 4, pc + 6]
                                } else {
                                    vec![pc + 0x22, pc + 0x24]
                                }
                            );
                            assert_eq!(
                                timed.registers.d[usize::from(register)],
                                0xabcd_0000
                                    | if truth {
                                        counter
                                    } else {
                                        counter.wrapping_sub(1) & 0xffff
                                    }
                            );
                            assert_eq!(timed.registers.status_register(), status);
                        }
                    }
                }
            }
        }
    }
}

pub(super) fn scc_seed(opcode: u16, status: u16, odd: bool) -> (PrefetchCpu, Bus, u32) {
    let mode = (opcode >> 3) & 7;
    let register = usize::from(opcode & 7);
    let mut timed = cpu(status);
    for index in 0..8 {
        timed.registers.d[index] = 0x89ab_cd55;
        timed
            .registers
            .set_address_register(index, 0x3008 + index as u32 * 0x100 + u32::from(odd));
    }
    let base = timed.registers.address_register(register).unwrap();
    let (extensions, address) = match (mode, register) {
        (0, _) => (vec![], 0),
        (2 | 3, _) => (vec![], base),
        (4, _) => (vec![], base - if register == 7 { 2 } else { 1 }),
        (5, _) => (vec![0xfffe], base - 2),
        (6, _) => (
            vec![0x8002],
            base + timed.registers.address_register(0).unwrap() + 2,
        ),
        (7, 0) => (vec![0x8030 + u16::from(odd)], 0xffff_8030 + u32::from(odd)),
        (7, 1) => (
            vec![0xabcd, 0x3010 + u16::from(odd)],
            0xabcd_3010 + u32::from(odd),
        ),
        _ => unreachable!(),
    };
    let words: Vec<_> = extensions.into_iter().chain([0x7201, 0x7402]).collect();
    timed.pipeline.as_mut().unwrap().words = [opcode, words[0]];
    let pc = timed.registers.pc;
    let mut bus = Bus::default();
    bus.words.insert(pc & BUS_ADDRESS_MASK, opcode);
    for (index, &word) in words.iter().enumerate() {
        bus.words
            .insert((pc + 2 + index as u32 * 2) & BUS_ADDRESS_MASK, word);
    }
    if mode != 0 {
        bus.words.insert(address & BUS_ADDRESS_MASK & !1, 0xa55a);
    }
    (timed, bus, address)
}

#[test]
fn scc_all_conditions_eas_banks_lanes_waits_preserve_ccr_and_register_parts() {
    for code in 0..16 {
        for flags in 0..32 {
            for mode in [0, 2, 3, 4, 5, 6, 7] {
                for register in 0..if mode == 7 { 2 } else { 8 } {
                    for bank in [0, SUPERVISOR] {
                        for odd in [false, true] {
                            let opcode = 0x50c0 | code << 8 | mode << 3 | register;
                            let (mut timed, mut bus, address) = scc_seed(opcode, bank | flags, odd);
                            bus.wait = u32::from(odd) * 3;
                            verify(&mut timed, &mut bus);
                            assert_eq!(timed.registers.status_register(), bank | flags);
                            let value = if conditions(flags)[usize::from(code)] {
                                255
                            } else {
                                0
                            };
                            if mode == 0 {
                                assert_eq!(
                                    timed.registers.d[usize::from(register)],
                                    0x89ab_cd00 | value
                                );
                            } else {
                                let word = bus.words[&(address & BUS_ADDRESS_MASK & !1)];
                                assert_eq!(
                                    word,
                                    if address & 1 == 0 {
                                        (value as u16) << 8 | 0x5a
                                    } else {
                                        0xa500 | value as u16
                                    }
                                );
                                assert!(
                                    matches!(bus.events.last(),Some(Event::Write(a,_,_)) if a.logical_address==address && a.kind==AccessKind::Data && a.lanes==if address&1==0 {ByteLanes::Upper} else {ByteLanes::Lower})
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
fn word_displacement_boundaries_and_logical_pc_wrap_preserve_base_and_return() {
    for opcode in [0x6100, 0x50c8, 0x51c8] {
        for extension in [0, 2, 0xfffe, 0x8000, 0x7ffe] {
            for counter in [0, 1, 0xffff] {
                let (mut timed, mut bus) = branch_seed(opcode, extension, 0x1f);
                timed.registers.pc = 0xffff_fffc;
                timed.pipeline.as_mut().unwrap().address = timed.registers.pc;
                timed.registers.d[0] = 0x1234_0000 | counter;
                bus.words.extend([
                    (0x00ff_fffc, opcode),
                    (0x00ff_fffe, extension),
                    (0, 0x7201),
                    (2, 0x7402),
                ]);
                let target = 0xffff_fffeu32.wrapping_add_signed(i32::from(extension as i16));
                bus.words.entry(target & BUS_ADDRESS_MASK).or_insert(0x7603);
                bus.words
                    .entry(target.wrapping_add(2) & BUS_ADDRESS_MASK)
                    .or_insert(0x7804);
                verify(&mut timed, &mut bus);
                assert_eq!(
                    timed.registers.pc,
                    if opcode == 0x50c8 || opcode == 0x51c8 && counter == 0 {
                        0
                    } else {
                        target
                    }
                );
                if opcode == 0x6100 {
                    assert_eq!(bus.words[&0x1ffc], 0);
                    assert_eq!(bus.words[&0x1ffe], 0);
                }
            }
        }
    }
}

#[test]
fn branch_and_condition_trace_frames_save_the_completed_boundary_and_keep_results() {
    for opcode in [0x6120, 0x51c8, 0x50c0, 0x50d7] {
        for bank in [0, SUPERVISOR] {
            let (mut timed, mut bus) = if opcode & 0xf0c0 == 0x50c0 && opcode & 0xf0f8 != 0x50c8 {
                let (timed, bus, _) = scc_seed(opcode, bank | TRACE | 0x1f, false);
                (timed, bus)
            } else {
                branch_seed(opcode, 0x20, bank | TRACE | 0x1f)
            };
            bus.handler(9);
            bus.wait = 1;
            timed.step_instruction(&mut bus).unwrap();
            let completed = timed.registers;
            assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), Some(41));
            assert_eq!(timed.registers.d, completed.d);
            assert_eq!(timed.registers.usp, completed.usp);
            assert_eq!(timed.registers.ssp, completed.ssp - 6);
            assert_eq!(
                bus.words[&((completed.ssp - 2) & BUS_ADDRESS_MASK)],
                completed.pc as u16
            );
            assert_eq!(
                bus.words[&((completed.ssp - 4) & BUS_ADDRESS_MASK)],
                (completed.pc >> 16) as u16
            );
            assert_eq!(
                bus.words[&((completed.ssp - 6) & BUS_ADDRESS_MASK)],
                completed.status_register()
            );
        }
    }
}
