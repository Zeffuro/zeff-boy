use super::tests::{seed, shapes, transfers};
use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, Event};

fn frozen(cpu: &mut PrefetchCpu, bus: &mut Bus) {
    let state = cpu.clone();
    let events = bus.events.clone();
    assert_eq!(cpu.step_instruction(bus), Err(TimingFault::Frozen));
    assert_eq!(cpu.service_pending_trace(bus), Err(TimingFault::Frozen));
    assert_eq!(cpu.step_boundary(bus), Err(TimingFault::Frozen));
    assert_eq!(*cpu, state);
    assert_eq!(bus.events, events);
    assert_eq!(cpu.clock().0, bus.clock);
}

#[test]
fn every_status_transfer_failure_preserves_ea_bank_status_and_completed_writes() {
    for family in [0x40c0, 0x44c0, 0x46c0] {
        for shape in shapes(family != 0x40c0) {
            for supervisor in [0, SUPERVISOR] {
                let sr = supervisor | TRACE | 0x071a;
                let (mut completed, mut full, _, _) = seed(family, shape, sr, 0x0011, 0xf810);
                full.wait = 3;
                completed.step_instruction(&mut full).unwrap();
                let events = transfers(&full);
                let privilege = family == 0x46c0 && supervisor == 0;
                let operand_read = events
                    .iter()
                    .position(|(access, _)| access.kind == AccessKind::Data);
                let input_count = if shape.0 == 0 {
                    0
                } else if shape == (7, 4) {
                    1
                } else {
                    operand_read.unwrap_or(usize::MAX).saturating_add(1)
                };
                for fail in 0..events.len() {
                    let (mut cpu, mut bus, _, _) = seed(family, shape, sr, 0x0011, 0xf810);
                    let initial = cpu.registers;
                    let queue = cpu.queued_words();
                    let mut expected = initial;
                    let (mode, register) = shape;
                    if !privilege {
                        if matches!(mode, 3 | 4) {
                            let base = initial.address_register(register).unwrap();
                            expected.set_address_register(
                                register,
                                if mode == 3 { base + 2 } else { base - 2 },
                            );
                        }
                        if family == 0x40c0 && mode == 0 {
                            expected.d[register] = (initial.d[register] & 0xffff_0000)
                                | u32::from(initial.status_register());
                        } else if family != 0x40c0 && fail >= input_count {
                            expected.set_status_register(completed.registers.status_register());
                        }
                    }
                    let mut memory = bus.words.clone();
                    for (access, value) in events.iter().take(fail) {
                        if let Some(value) = value {
                            memory.insert(access.bus_address, *value);
                        }
                    }
                    bus.wait = 3;
                    bus.fail = Some(fail);
                    assert!(
                        matches!(cpu.step_instruction(&mut bus), Err(TimingFault::Bus { source, .. }) if source == fail)
                    );
                    assert_eq!(
                        cpu.registers, expected,
                        "{family:04x}/{mode}/{register}/{supervisor:04x}/failure{fail}"
                    );
                    assert_eq!(cpu.queued_words(), queue);
                    assert_eq!(bus.words, memory);
                    assert_eq!(bus.transfers, fail + 1);
                    assert!(!cpu.trace_pending());
                    frozen(&mut cpu, &mut bus);
                }
            }
        }
    }
}

#[test]
fn invalid_status_eas_are_unsupported_before_privilege_or_operand_callbacks() {
    for family in [0x40c0, 0x44c0, 0x46c0] {
        for supervisor in [0, SUPERVISOR] {
            for (mode, register) in
                [(1, 0), (1, 7), (7, 5), (7, 6), (7, 7)]
                    .into_iter()
                    .chain(if family == 0x40c0 {
                        vec![(7, 2), (7, 3), (7, 4)]
                    } else {
                        vec![]
                    })
            {
                let mut cpu = crate::timing::test_bus::cpu(supervisor | TRACE);
                let opcode = family | mode << 3 | register;
                cpu.pipeline.as_mut().unwrap().words = [opcode, 0xffff];
                let initial = cpu.clone();
                let mut bus = Bus::default();
                assert_eq!(
                    cpu.step_instruction(&mut bus),
                    Err(TimingFault::UnsupportedInstruction {
                        pc: cpu.registers.pc,
                        opcode
                    })
                );
                assert_eq!(cpu, initial);
                assert!(bus.events.is_empty());
            }
        }
    }
    for opcode in 0x42c0..=0x42ff {
        let mut cpu = crate::timing::test_bus::cpu(SUPERVISOR);
        cpu.pipeline.as_mut().unwrap().words[0] = opcode;
        let mut bus = Bus::default();
        assert!(matches!(
            cpu.step_instruction(&mut bus),
            Err(TimingFault::UnsupportedInstruction { .. })
        ));
        assert!(bus.events.is_empty());
    }
}

#[test]
fn user_to_sr_privilege_never_fetches_extensions_reads_operands_or_updates_a7() {
    for shape in shapes(true) {
        let (mut cpu, mut bus, _, _) = seed(0x46c0, shape, TRACE | 0x071f, 0xffff, 0xf810);
        let initial = cpu.registers;
        let result = cpu.step_instruction(&mut bus).unwrap();
        assert_eq!(result.exception, Some(8));
        assert_eq!(result.clocks, 34);
        assert!(!cpu.trace_pending());
        assert_eq!(cpu.registers.usp, initial.usp);
        assert_eq!(cpu.registers.a, initial.a);
        assert_eq!(bus.words[&0xffa], initial.status_register());
        assert_eq!(bus.words[&0xffe], initial.pc as u16);
        assert_eq!(bus.words[&0xffc], (initial.pc >> 16) as u16);
        assert!(
            transfers(&bus)
                .iter()
                .all(|(access, _)| !matches!(access.kind, AccessKind::Data)
                    && access.logical_address != initial.pc + 4)
        );
    }
}

#[test]
fn odd_status_sources_freeze_before_data_callback_but_retain_pre_postincrement() {
    for family in [0x40c0, 0x44c0, 0x46c0] {
        for supervisor in [0, SUPERVISOR] {
            if family == 0x46c0 && supervisor == 0 {
                continue;
            }
            for mode in [2, 3, 4] {
                let (mut cpu, mut bus, _, _) =
                    seed(family, (mode, 7), supervisor | TRACE | 0x1f, 0x0010, 0x1010);
                cpu.registers.set_address_register(7, 0x3001);
                let initial = cpu.registers;
                let address = if mode == 4 { 0x2fff } else { 0x3001 };
                assert_eq!(
                    cpu.step_instruction(&mut bus),
                    Err(TimingFault::AddressError { address })
                );
                let mut expected = initial;
                if mode == 3 {
                    expected.set_address_register(7, 0x3003);
                }
                if mode == 4 {
                    expected.set_address_register(7, 0x2fff);
                }
                assert_eq!(cpu.registers, expected);
                assert_eq!(bus.transfers, 0);
                assert_eq!(bus.events.len(), usize::from(mode == 4));
                frozen(&mut cpu, &mut bus);
            }
        }
    }
}

#[test]
fn status_idle_overflow_retains_only_the_status_committed_before_that_idle() {
    let (mut cpu, mut bus, _, _) = seed(0x44c0, (0, 0), SUPERVISOR | TRACE | 0x1f, 0, 0x1010);
    let initial = cpu.registers;
    cpu.clock = CpuClocks(u64::MAX - 1);
    bus.clock = cpu.clock.0;
    assert_eq!(
        cpu.step_instruction(&mut bus),
        Err(TimingFault::ClockOverflow)
    );
    assert_eq!(cpu.registers, initial);
    assert!(bus.events.is_empty());
    frozen(&mut cpu, &mut bus);
    let (mut cpu, mut bus, _, _) = seed(0x44c0, (0, 0), SUPERVISOR | TRACE | 0x1f, 0, 0x1010);
    cpu.clock = CpuClocks(u64::MAX - 3);
    bus.clock = cpu.clock.0;
    assert_eq!(
        cpu.step_instruction(&mut bus),
        Err(TimingFault::ClockOverflow)
    );
    assert_eq!(cpu.registers.status_register(), SUPERVISOR | TRACE);
    assert_eq!(bus.events, [Event::Idle(u64::MAX - 3, 2)]);
    frozen(&mut cpu, &mut bus);
}
