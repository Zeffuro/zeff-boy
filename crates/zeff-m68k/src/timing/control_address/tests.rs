use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, Event, cpu};

pub(super) fn shapes() -> Vec<(u16, u16)> {
    [2, 5, 6, 7]
        .into_iter()
        .flat_map(|mode| (0..if mode == 7 { 4 } else { 8 }).map(move |register| (mode, register)))
        .collect()
}

pub(super) fn seed(opcode: u16, status: u16, extension: u16) -> (PrefetchCpu, Bus, u32) {
    let mut timed = cpu(status);
    for index in 0..8 {
        timed.registers.d[index] = 0x1234_0010 + index as u32 * 2;
        timed
            .registers
            .set_address_register(index, 0x2000_3008 + index as u32 * 0x100);
    }
    let mode = (opcode >> 3) & 7;
    let register = usize::from(opcode & 7);
    let pc = timed.registers.pc;
    let base = if mode == 7 {
        pc + 2
    } else {
        timed.registers.address_register(register).unwrap()
    };
    let (extensions, address) = match (mode, register) {
        (2, _) => (vec![], base),
        (5, _) | (7, 2) => (vec![0xfffe], base - 2),
        (6, _) | (7, 3) => (vec![extension], indexed(&timed.registers, base, extension)),
        (7, 0) => (vec![0x8030], 0xffff_8030),
        (7, 1) => (vec![0xabcd, 0x3010], 0xabcd_3010),
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
    bus.words
        .entry(address & BUS_ADDRESS_MASK)
        .or_insert(0x7603);
    bus.words
        .entry(address.wrapping_add(2) & BUS_ADDRESS_MASK)
        .or_insert(0x7804);
    (timed, bus, address)
}

fn compare(timed: &mut PrefetchCpu, bus: &mut Bus) {
    let mut raw = crate::test_bus::TestBus::default();
    for (&address, &word) in &bus.words {
        raw.word(address, word);
    }
    let mut registers = timed.registers;
    let pc = registers.pc;
    let status = registers.status_register();
    let execution = crate::instruction::execute(
        &mut registers,
        &mut raw,
        pc,
        timed.pipeline.unwrap().words[0],
    )
    .unwrap();
    let opcode = timed.pipeline.unwrap().words[0];
    let sampled_target = bus.words[&(registers.pc & BUS_ADDRESS_MASK)];
    let step = timed.step_instruction(bus).unwrap();
    assert_eq!(timed.registers, registers);
    assert_eq!(step.exception, None);
    assert_eq!(
        step.clocks,
        u64::from(execution.nominal_cycles) + bus.transfers as u64 * u64::from(bus.wait)
    );
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
            if opcode & 0xffc0 == 0x4e80 {
                sampled_target
            } else {
                bus.words[&(pc & BUS_ADDRESS_MASK)]
            },
            bus.words[&(pc.wrapping_add(2) & BUS_ADDRESS_MASK)]
        ])
    );
    for event in &bus.events {
        if let Event::Read(a, _) | Event::Write(a, _, _) = event {
            assert_eq!(a.lanes, ByteLanes::Both);
            assert_eq!(a.bus_address, a.logical_address & BUS_ADDRESS_MASK);
            assert_eq!(
                a.function_code,
                if a.kind == AccessKind::Prefetch {
                    if status & SUPERVISOR != 0 {
                        FunctionCode::SupervisorProgram
                    } else {
                        FunctionCode::UserProgram
                    }
                } else {
                    if status & SUPERVISOR != 0 {
                        FunctionCode::SupervisorData
                    } else {
                        FunctionCode::UserData
                    }
                }
            );
            assert!(matches!(a.kind, AccessKind::Prefetch | AccessKind::Stack));
        }
    }
}

#[test]
fn all_control_eas_lea_destinations_index_widths_banks_trace_waits_match_architecture() {
    for base in [0x41c0, 0x4840, 0x4e80, 0x4ec0] {
        for (mode, register) in shapes() {
            for destination in 0..if base == 0x41c0 { 8 } else { 1 } {
                let opcode = base | destination << 9 | mode << 3 | register;
                for extension in [0x8702, 0xf802, 0x0002, 0x1802] {
                    for status in [
                        0x1f,
                        SUPERVISOR | 0x1f,
                        TRACE | 0x1f,
                        SUPERVISOR | TRACE | 0x1f,
                    ] {
                        for wait in [0, 1, 3] {
                            let (mut timed, mut bus, _) = seed(opcode, status, extension);
                            bus.wait = wait;
                            compare(&mut timed, &mut bus);
                            assert_eq!(timed.registers.status_register(), status);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn all_control_templates_have_exact_sequential_target_stack_and_idle_order() {
    for base in [0x41c0, 0x4840, 0x4e80, 0x4ec0] {
        for (mode, register) in shapes() {
            let opcode = base | mode << 3 | register;
            let (mut timed, mut bus, address) = seed(opcode, 0x1f, 0x8702);
            let pc = timed.registers.pc;
            let sp = timed.registers.stack_pointer();
            timed.step_instruction(&mut bus).unwrap();
            let actual: String = bus
                .events
                .iter()
                .map(|event| match event {
                    Event::Read(_, _) => 'f',
                    Event::Write(..) => 'w',
                    Event::Idle(_, _) => 'i',
                    _ => panic!(),
                })
                .collect();
            let template = match (base, mode, register) {
                (0x41c0, 2, _) => "f",
                (0x41c0, 5, _) | (0x41c0, 7, 0 | 2) => "ff",
                (0x41c0, 6, _) | (0x41c0, 7, 3) => "ifif",
                (0x41c0, 7, 1) => "fff",
                (0x4840, 2, _) => "fww",
                (0x4840, 5, _) | (0x4840, 7, 2) => "ffww",
                (0x4840, 6, _) | (0x4840, 7, 3) => "ififww",
                (0x4840, 7, 0) => "fwwf",
                (0x4840, 7, 1) => "ffwwf",
                (0x4e80, 2, _) => "fwwf",
                (0x4e80, 5 | 6, _) | (0x4e80, 7, 0 | 2 | 3) => "ifwwf",
                (0x4e80, 7, 1) => "ffwwf",
                (0x4ec0, 2, _) => "ff",
                (0x4ec0, 5 | 6, _) | (0x4ec0, 7, 0 | 2 | 3) => "iff",
                (0x4ec0, 7, 1) => "fff",
                _ => unreachable!(),
            };
            assert_eq!(actual, template, "op={opcode:04x}");
            if matches!(base, 0x4e80 | 0x4ec0) {
                let reads: Vec<_> = bus
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        Event::Read(a, _) => Some(a.logical_address),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    reads,
                    if mode == 7 && register == 1 {
                        vec![pc + 4, address, address + 2]
                    } else {
                        vec![address, address + 2]
                    }
                );
                for event in &bus.events {
                    if let Event::Idle(_, clocks) = event {
                        assert_eq!(
                            *clocks,
                            if mode == 6 || mode == 7 && register == 3 {
                                6
                            } else {
                                2
                            }
                        );
                    }
                }
            }
            let writes: Vec<_> = bus
                .events
                .iter()
                .filter_map(|event| match event {
                    Event::Write(a, v, _) => Some((a.logical_address, *v)),
                    _ => None,
                })
                .collect();
            if matches!(base, 0x4840 | 0x4e80) {
                let value = if base == 0x4840 {
                    address
                } else {
                    pc + if mode == 2 {
                        2
                    } else if mode == 7 && register == 1 {
                        6
                    } else {
                        4
                    }
                };
                assert_eq!(
                    writes,
                    vec![(sp - 4, (value >> 16) as u16), (sp - 2, value as u16)]
                );
            } else {
                assert!(writes.is_empty());
            }
        }
    }
}

#[test]
fn sampled_target_and_sequential_words_survive_stack_self_modification_at_the_correct_stage() {
    for (opcode, first_before_write, last_before_write) in [
        (0x4e97, true, false),
        (0x4857, true, true),
        (0x4878, true, false),
        (0x4879, true, false),
    ] {
        let (mut timed, mut bus, _) = seed(opcode, SUPERVISOR, 0x8702);
        let pc = timed.registers.pc;
        if opcode == 0x4e97 {
            timed.registers.ssp = 0x3000;
            bus.words.insert(0x3000, 0x7201);
            bus.words.insert(0x3002, 0x7402);
        } else {
            timed.registers.ssp = pc
                + if opcode == 0x4857 {
                    6
                } else if opcode == 0x4878 {
                    8
                } else {
                    10
                };
        }
        let sp = timed.registers.ssp;
        let target = if opcode == 0x4e97 {
            sp
        } else {
            pc + if opcode == 0x4857 {
                2
            } else if opcode == 0x4878 {
                4
            } else {
                6
            }
        };
        if opcode == 0x4e97 {
            // Both target words overlap the return-address writes.
            timed.registers.set_address_register(0, 0x2ffc);
            timed.pipeline.as_mut().unwrap().words[0] = 0x4e90;
            bus.words.insert(pc & BUS_ADDRESS_MASK, 0x4e90);
            bus.words.insert(0x2ffc, 0x7201);
            bus.words.insert(0x2ffe, 0x7402);
        }
        let initial = bus.words.clone();
        timed.step_instruction(&mut bus).unwrap();
        let actual_target = if opcode == 0x4e97 { 0x2ffc } else { target };
        let queue = timed.queued_words().unwrap();
        assert_eq!(
            queue[0],
            if first_before_write {
                initial[&(actual_target & BUS_ADDRESS_MASK)]
            } else {
                bus.words[&(actual_target & BUS_ADDRESS_MASK)]
            }
        );
        assert_eq!(
            queue[1],
            if last_before_write {
                initial[&(actual_target.wrapping_add(2) & BUS_ADDRESS_MASK)]
            } else {
                bus.words[&(actual_target.wrapping_add(2) & BUS_ADDRESS_MASK)]
            }
        );
    }
}

#[test]
fn control_address_trace_uses_updated_destination_or_stack_and_completed_target() {
    for opcode in [0x4ff7, 0x4877, 0x4eb7, 0x4ef7] {
        for bank in [0, SUPERVISOR] {
            let (mut timed, mut bus, _) = seed(opcode, bank | TRACE | 0x1f, 0xf802);
            bus.handler(9);
            timed.step_instruction(&mut bus).unwrap();
            let completed = timed.registers;
            assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), Some(34));
            assert_eq!(timed.registers.d, completed.d);
            assert_eq!(timed.registers.usp, completed.usp);
            assert_eq!(timed.registers.ssp, completed.ssp - 6);
            for register in 0..7 {
                assert_eq!(
                    timed.registers.address_register(register),
                    completed.address_register(register)
                );
            }
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
