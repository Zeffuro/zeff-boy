use super::*;
use crate::SUPERVISOR;
use crate::test_bus::TestBus;
use crate::timing::test_bus::{Bus, Event, cpu};

pub(super) fn seed(opcode: u16, sr: u16, extension: u16, wait: u32) -> (PrefetchCpu, Bus) {
    let mut cpu = cpu(sr);
    for register in 0..7 {
        cpu.registers
            .set_address_register(register, 0x1234_3000 + register as u32 * 16);
        cpu.registers.d[register] = 0xabba_9000 + register as u32;
    }
    cpu.pipeline.as_mut().unwrap().words = [opcode, extension];
    let mut bus = Bus {
        wait,
        ..Bus::default()
    };
    bus.words.extend([
        (0x340102, extension),
        (0x340104, 0x7201),
        (0x340106, 0x7602),
    ]);
    bus.handler(7);
    bus.handler(8);
    bus.handler(9);
    if opcode & 0xfff8 == 0x4e58 {
        let frame = cpu
            .registers
            .address_register(usize::from(opcode & 7))
            .unwrap();
        bus.words.extend([
            (frame & BUS_ADDRESS_MASK, 0x5678),
            (frame.wrapping_add(2) & BUS_ADDRESS_MASK, 0x4000),
        ]);
    } else if opcode == 0x4e77 {
        let sp = cpu.registers.stack_pointer();
        bus.words.extend([
            (sp, extension),
            (sp + 2, 0x1234),
            (sp + 4, 0x4000),
            (0x344000, 0x7403),
            (0x344002, 0x7604),
        ]);
    }
    (cpu, bus)
}

pub(super) fn oracle(cpu: &PrefetchCpu, bus: &Bus) -> (Registers, TestBus, u32) {
    let mut registers = cpu.registers;
    let mut raw = TestBus::default();
    for (&address, &word) in &bus.words {
        raw.word(address, word);
    }
    let opcode = cpu.pipeline.unwrap().words[0];
    let pc = registers.pc;
    let step = crate::instruction::execute(&mut registers, &mut raw, pc, opcode).unwrap();
    (registers, raw, step.nominal_cycles)
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

pub(super) fn check_clock(bus: &Bus) {
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
fn frame_and_usp_routes_preserve_full_state_banks_flags_and_waits() {
    for supervisor in [0, SUPERVISOR] {
        for trace in [0, TRACE] {
            for ccr in 0..32 {
                for wait in [0, 1, 3] {
                    for opcode in (0x4e50..=0x4e6f).chain([0x4e77]) {
                        for extension in [0, 0xfffc, 0x7ffe, 0x8000, 0xffff] {
                            let sr = supervisor | trace | 0x0500 | ccr;
                            let (mut cpu, mut bus) = seed(opcode, sr, extension, wait);
                            let initial = cpu.registers;
                            let (expected, raw, cycles) = oracle(&cpu, &bus);
                            let result = cpu.step_instruction(&mut bus).unwrap();
                            assert_eq!(
                                cpu.registers, expected,
                                "{opcode:04x}/{sr:04x}/{extension:04x}"
                            );
                            assert_eq!(
                                result.clocks,
                                u64::from(cycles) + bus.transfers as u64 * u64::from(wait)
                            );
                            let privilege = opcode & 0xfff0 == 0x4e60 && supervisor == 0;
                            assert_eq!(result.exception, if privilege { Some(8) } else { None });
                            assert_eq!(cpu.trace_pending(), trace != 0 && !privilege);
                            for (&address, &word) in &bus.words {
                                assert_eq!(
                                    word,
                                    u16::from_be_bytes([
                                        raw.peek(address),
                                        raw.peek(address.wrapping_add(1))
                                    ])
                                );
                            }
                            if !privilege {
                                let fc = crate::ea::data_space(&initial);
                                for (access, _) in transfers(&bus) {
                                    assert_eq!(access.lanes, ByteLanes::Both);
                                    assert_eq!(
                                        access.function_code,
                                        if access.kind == AccessKind::Stack {
                                            fc
                                        } else {
                                            crate::ea::program_space(&initial)
                                        }
                                    );
                                }
                                assert_eq!(
                                    cpu.queued_words(),
                                    Some(if opcode == 0x4e77 {
                                        [0x7403, 0x7604]
                                    } else if opcode & 0xfff8 == 0x4e50 {
                                        [0x7201, 0x7602]
                                    } else {
                                        [extension, 0x7201]
                                    })
                                );
                            }
                            check_clock(&bus);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn frame_transfer_addresses_orders_and_immutable_alias_inputs_are_exact() {
    for supervisor in [0, SUPERVISOR] {
        for register in 0..8 {
            for family in [0x4e50, 0x4e58] {
                let (mut cpu, mut bus) = seed(family | register, supervisor | 0x1f, 0xfff8, 1);
                let initial = cpu.registers;
                let sp = initial.stack_pointer();
                let old = initial.address_register(usize::from(register)).unwrap();
                cpu.step_instruction(&mut bus).unwrap();
                let events = transfers(&bus);
                let addresses: Vec<_> = events
                    .iter()
                    .map(|(access, _)| access.logical_address)
                    .collect();
                if family == 0x4e50 {
                    assert_eq!(addresses, [initial.pc + 4, sp - 4, sp - 2, initial.pc + 6]);
                    assert_eq!(events[1].1, Some((old >> 16) as u16));
                    assert_eq!(events[2].1, Some(old as u16));
                    assert_eq!(
                        cpu.registers.address_register(usize::from(register)),
                        Some(if register == 7 { sp - 12 } else { sp - 4 })
                    );
                } else {
                    assert_eq!(addresses, [old, old + 2, initial.pc + 4]);
                    assert_eq!(
                        cpu.registers.stack_pointer(),
                        if register == 7 { 0x5678_4000 } else { old + 4 }
                    );
                }
            }
        }
        for ccr in 0..32 {
            let (mut cpu, mut bus) =
                seed(0x4e77, supervisor | TRACE | 0x0700 | 0x1f, 0xffe0 | ccr, 3);
            let sp = cpu.registers.stack_pointer();
            cpu.step_instruction(&mut bus).unwrap();
            assert_eq!(
                transfers(&bus)
                    .iter()
                    .map(|(access, _)| access.logical_address)
                    .collect::<Vec<_>>(),
                [sp, sp + 2, sp + 4, 0x1234_4000, 0x1234_4002]
            );
            assert_eq!(
                cpu.registers.status_register(),
                supervisor | TRACE | 0x0700 | ccr
            );
        }
    }
    let (mut cpu, mut bus) = seed(0x4e57, SUPERVISOR, 0xfffc, 0);
    cpu.registers.ssp = cpu.registers.pc + 8;
    let sp = cpu.registers.ssp;
    cpu.step_instruction(&mut bus).unwrap();
    assert_eq!(cpu.queued_words(), Some([0x7201, sp as u16]));
    assert_eq!(cpu.registers.ssp, sp - 8);
}

#[test]
fn trapv_uses_supervisor_prefetch_and_preserves_entry_trace_for_both_v_paths() {
    for supervisor in [0, SUPERVISOR] {
        for trace in [0, TRACE] {
            for ccr in 0..32 {
                for wait in [0, 1, 3] {
                    let (mut cpu, mut bus) =
                        seed(0x4e76, supervisor | trace | 0x0500 | ccr, 0x7401, wait);
                    let sr = cpu.registers.status_register();
                    let (expected, raw, cycles) = oracle(&cpu, &bus);
                    let result = cpu.step_instruction(&mut bus).unwrap();
                    assert_eq!(cpu.registers, expected);
                    assert_eq!(
                        result.clocks,
                        u64::from(cycles) + bus.transfers as u64 * u64::from(wait)
                    );
                    assert_eq!(
                        result.exception,
                        if ccr & OVERFLOW != 0 { Some(7) } else { None }
                    );
                    assert_eq!(cpu.trace_pending(), trace != 0);
                    let events = transfers(&bus);
                    assert_eq!(events[0].0.logical_address, 0x1234_0104);
                    assert_eq!(
                        events[0].0.function_code,
                        if ccr & OVERFLOW != 0 {
                            FunctionCode::SupervisorProgram
                        } else {
                            crate::ea::program_space(&cpu.registers)
                        }
                    );
                    if ccr & OVERFLOW != 0 {
                        assert_eq!(
                            events
                                .iter()
                                .map(|(access, _)| access.logical_address)
                                .collect::<Vec<_>>(),
                            [
                                0x1234_0104,
                                0xffe,
                                0xffa,
                                0xffc,
                                28,
                                30,
                                0x1234_5000,
                                0x1234_5002
                            ]
                        );
                        assert_eq!(bus.words[&0xffa], sr);
                        assert_eq!(bus.words[&0xffe], 0x0102);
                        assert_eq!(bus.words[&0xffc], 0x1234);
                        assert_eq!(
                            bus.events
                                .iter()
                                .filter(|event| matches!(event, Event::Idle(_, _)))
                                .count(),
                            1
                        );
                        assert_eq!(cpu.queued_words(), Some([0x4e71, 0x7201]));
                    }
                    for (&address, &word) in &bus.words {
                        assert_eq!(
                            word,
                            u16::from_be_bytes([
                                raw.peek(address),
                                raw.peek(address.wrapping_add(1))
                            ])
                        );
                    }
                    check_clock(&bus);
                }
            }
        }
    }
}

#[test]
fn completed_frame_routes_trace_the_completed_pc_status_and_active_stack() {
    for supervisor in [0, SUPERVISOR] {
        for opcode in [
            0x4e50, 0x4e57, 0x4e58, 0x4e5f, 0x4e60, 0x4e68, 0x4e6f, 0x4e77, 0x4e76,
        ] {
            let (mut cpu, mut bus) = seed(opcode, supervisor | TRACE | 0x12, 0xfffc, 0);
            let first = cpu.step_instruction(&mut bus).unwrap();
            let registers = cpu.registers;
            if first.exception == Some(8) {
                assert_eq!(cpu.service_pending_trace(&mut bus).unwrap(), None);
                continue;
            }
            let sp = registers.ssp;
            let start = cpu.clock();
            assert_eq!(
                cpu.step_instruction(&mut bus),
                Err(TimingFault::PendingTrace)
            );
            assert_eq!(cpu.service_pending_trace(&mut bus).unwrap(), Some(34));
            assert_eq!(cpu.clock().0, start.0 + 34);
            assert_eq!(
                bus.words[&(sp.wrapping_sub(2) & BUS_ADDRESS_MASK)],
                registers.pc as u16
            );
            assert_eq!(
                bus.words[&(sp.wrapping_sub(6) & BUS_ADDRESS_MASK)],
                registers.status_register()
            );
            assert_eq!(cpu.registers.ssp, sp.wrapping_sub(6));
            assert!(!cpu.trace_pending());
        }
    }
}

#[test]
fn rtr_and_link_wrap_logical_addresses_and_mask_only_external_addresses() {
    for supervisor in [0, SUPERVISOR] {
        let (mut cpu, mut bus) = seed(0x4e77, supervisor | 0x1f, 0, 0);
        cpu.registers.set_address_register(7, 0xffff_fffc);
        bus.words
            .extend([(0xfffffc, 0x8010), (0xfffffe, 0xffff), (0, 0xfffe)]);
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(cpu.registers.pc, 0xffff_fffe);
        assert_eq!(cpu.registers.stack_pointer(), 2);
        assert_eq!(cpu.registers.status_register(), supervisor | 0x10);
        assert_eq!(
            transfers(&bus)
                .iter()
                .map(|(access, _)| access.logical_address)
                .collect::<Vec<_>>(),
            [0xffff_fffc, 0xffff_fffe, 0, 0xffff_fffe, 0]
        );
        let (mut cpu, mut bus) = seed(0x4e57, supervisor, 0xfffc, 0);
        cpu.registers.set_address_register(7, 2);
        cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(cpu.registers.stack_pointer(), 0xffff_fffa);
        let events = transfers(&bus);
        assert_eq!(events[1].0.logical_address, 0xffff_fffe);
        assert_eq!(events[1].0.bus_address, 0xfffffe);
        assert_eq!(events[2].0.logical_address, 0);
    }
}
