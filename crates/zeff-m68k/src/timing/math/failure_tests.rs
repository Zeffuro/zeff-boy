use super::tests::{seed, shapes};
use super::*;
use crate::timing::test_bus::Event;
use crate::{EXTEND, SUPERVISOR};

#[test]
fn every_ea_transfer_failure_keeps_only_completed_inputs_alu_and_bus_effects() {
    for (family, source, dividend) in [
        (0xc0c0, 0xfffe, 0x1234),
        (0xc1c0, 0xfffe, 0x8000),
        (0x80c0, 3, 7),
        (0x81c0, 3, 0xffff_fff9),
        (0x80c0, 1, 0x10000),
        (0x81c0, 1, 0x8000),
        (0x80c0, 0, 0x0001_0000),
        (0x81c0, 0, 0x8000_0000),
        (0x4180, 3, 2),
        (0x4180, 0, 1),
        (0x4180, 0, 0xffff),
    ] {
        for (mode, register) in shapes() {
            for bank in [0, SUPERVISOR] {
                let opcode = family | mode << 3 | register;
                let (mut completed, mut full, _) =
                    seed(opcode, bank | TRACE | 0x1f, source, dividend);
                let initial = completed.registers;
                full.wait = 3;
                completed.step_instruction(&mut full).unwrap();
                let transfers: Vec<_> = full
                    .events
                    .iter()
                    .filter(|event| !matches!(event, Event::Idle(..)))
                    .collect();
                let input_count = match (mode, register) {
                    (0, _) => 0,
                    (2..=4, _) | (7, 4) => 1,
                    (7, 1) => 3,
                    _ => 2,
                };
                for fail in 0..full.transfers {
                    let (mut timed, mut bus, _) =
                        seed(opcode, bank | TRACE | 0x1f, source, dividend);
                    let queue = timed.queued_words();
                    let mut expected = initial;
                    if matches!(mode, 3 | 4) {
                        let base = initial.address_register(usize::from(register)).unwrap();
                        expected.set_address_register(
                            usize::from(register),
                            if mode == 3 { base + 2 } else { base - 2 },
                        );
                    }
                    let old_dn = mode == 0
                        && family != 0x4180
                        && !(matches!(family, 0x80c0 | 0x81c0)
                            && initial.d[usize::from(register)] as u16 == 0);
                    if fail >= input_count && !old_dn {
                        expected.d = completed.registers.d;
                        let flags = completed.registers.status_register() & 0x1f;
                        expected.set_status_register((initial.status_register() & !0x1f) | flags);
                    }
                    let mut memory = bus.words.clone();
                    for event in transfers.iter().take(fail) {
                        if let Event::Write(access, value, _) = event {
                            memory.insert(access.bus_address, *value);
                        }
                    }
                    bus.wait = 3;
                    bus.fail = Some(fail);
                    assert!(
                        matches!(timed.step_instruction(&mut bus), Err(TimingFault::Bus { source, .. }) if source == fail)
                    );
                    assert_eq!(timed.registers, expected, "op={opcode:04x} fail={fail}");
                    assert_eq!(timed.queued_words(), queue);
                    assert_eq!(bus.words, memory);
                    assert_eq!(timed.clock().0, bus.clock);
                    assert!(!timed.trace_pending());
                    let events = bus.events.clone();
                    assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
                    assert_eq!(
                        timed.service_pending_trace(&mut bus),
                        Err(TimingFault::Frozen)
                    );
                    assert_eq!(timed.step_boundary(&mut bus), Err(TimingFault::Frozen));
                    assert_eq!(bus.events, events);
                }
            }
        }
    }
}

#[test]
fn supervisor_a7_ea_updates_supply_exception_stack_and_user_updates_leave_it_independent() {
    for family in [0x80c0, 0x81c0, 0x4180] {
        for mode in [3, 4] {
            for bank in [0, SUPERVISOR] {
                let opcode = family | mode << 3 | 7;
                let (mut timed, mut bus, address) = seed(opcode, bank | EXTEND, 0, 1);
                let old_sp = timed.registers.ssp;
                let old_usp = timed.registers.usp;
                let base = timed.registers.address_register(7).unwrap();
                let new_a7 = if mode == 3 { base + 2 } else { base - 2 };
                timed.step_instruction(&mut bus).unwrap();
                let frame_base = if bank == SUPERVISOR { new_a7 } else { old_sp };
                assert_eq!(timed.registers.ssp, frame_base - 6);
                assert_eq!(
                    timed.registers.usp,
                    if bank == 0 { new_a7 } else { old_usp }
                );
                assert!(matches!(bus.events.first(), Some(Event::Idle(_, 2))) || mode == 3);
                let data_read = bus.events.iter().position(|event| matches!(event, Event::Read(a, _) if a.logical_address == address && a.kind == AccessKind::Data)).unwrap();
                let writes: Vec<_> = bus
                    .events
                    .iter()
                    .filter_map(|event| match event {
                        Event::Write(a, value, _) => Some((a.logical_address, *value)),
                        _ => None,
                    })
                    .collect();
                assert_eq!(writes[0].0, frame_base - 2);
                assert_eq!(writes[1].0, frame_base - 6);
                assert_eq!(writes[2].0, frame_base - 4);
                assert!(
                    bus.events[..data_read]
                        .iter()
                        .all(|event| !matches!(event, Event::Write(..)))
                );
                assert_eq!(timed.registers.d[0], 1);
            }
        }
    }
}

#[test]
fn aliased_stack_vector_and_code_reads_observe_prior_writes_and_twenty_four_bit_wrap() {
    let (mut timed, mut bus, _) = seed(0x80d8, SUPERVISOR | EXTEND, 0, 1);
    timed.registers.ssp = 26;
    timed.registers.set_address_register(0, 0xffff_fffe);
    bus.words.insert(0x00ff_fffe, 0);
    // The saved SR becomes vector5.high and saved PC.high becomes vector5.low.
    let saved_sr = SUPERVISOR | EXTEND | crate::ZERO;
    let target = (u32::from(saved_sr) << 16) | (timed.registers.pc >> 16);
    bus.words.insert(target & BUS_ADDRESS_MASK, 0x4e71);
    bus.words
        .insert(target.wrapping_add(2) & BUS_ADDRESS_MASK, 0x7201);
    timed.step_instruction(&mut bus).unwrap();
    assert_eq!(timed.registers.address_register(0), Some(0));
    assert_eq!(timed.registers.pc, target);
    assert_eq!(timed.queued_words(), Some([0x4e71, 0x7201]));
    assert_eq!(bus.words[&20], saved_sr);
    assert_eq!(bus.words[&22], 0x1234);
    let (mut code, mut bus, _) = seed(0x81c1, SUPERVISOR, 0, 1);
    code.registers.ssp = 0x1234_5006;
    code.step_instruction(&mut bus).unwrap();
    assert_eq!(
        code.queued_words(),
        Some([SUPERVISOR | crate::ZERO, 0x1234])
    );
    assert_eq!(code.registers.d[0], 1);
}

#[test]
fn separate_trace_entry_failures_freeze_after_the_completed_group_two_frame() {
    for opcode in [0x80c1, 0x81c1, 0x4181] {
        for bank in [0, SUPERVISOR] {
            for failed in 0..7 {
                let (mut timed, mut bus, _) = seed(opcode, bank | TRACE | EXTEND, 0, 1);
                timed.step_instruction(&mut bus).unwrap();
                let prefix = timed.clone();
                let old_memory = bus.words.clone();
                let initial_transfers = bus.transfers;
                bus.fail = Some(initial_transfers + failed);
                assert!(matches!(
                    timed.service_pending_trace(&mut bus),
                    Err(TimingFault::Bus { .. })
                ));
                assert_eq!(timed.registers, prefix.registers);
                assert_eq!(timed.queued_words(), prefix.queued_words());
                assert!(timed.trace_pending());
                let sp = prefix.registers.ssp;
                let mut expected = old_memory;
                for (index, address, value) in [
                    (0, sp - 2, prefix.registers.pc as u16),
                    (1, sp - 6, prefix.registers.status_register()),
                    (2, sp - 4, (prefix.registers.pc >> 16) as u16),
                ] {
                    if failed > index {
                        expected.insert(address & BUS_ADDRESS_MASK, value);
                    }
                }
                assert_eq!(bus.words, expected);
                let events = bus.events.clone();
                assert_eq!(timed.step_boundary(&mut bus), Err(TimingFault::Frozen));
                assert_eq!(bus.events, events);
            }
        }
    }
}

#[test]
fn invalid_eas_reject_before_callbacks_and_odd_operands_freeze_before_a_bus_cycle() {
    for family in [0xc0c0, 0xc1c0, 0x80c0, 0x81c0, 0x4180] {
        for ea in (8..16).chain(61..64) {
            let (mut timed, mut bus, _) = seed(family | 0x10, SUPERVISOR, 3, 7);
            let opcode = family | ea;
            timed.pipeline.as_mut().unwrap().words[0] = opcode;
            let initial = timed.clone();
            assert_eq!(
                timed.step_instruction(&mut bus),
                Err(TimingFault::UnsupportedInstruction {
                    pc: timed.registers.pc,
                    opcode
                })
            );
            assert_eq!(timed, initial);
            assert!(bus.events.is_empty());
        }
        for mode in [2, 3, 4] {
            let (mut timed, mut bus, _) = seed(family | mode << 3 | 7, 0, 3, 7);
            timed.registers.usp = 0x2001;
            let address = if mode == 4 { 0x1fff } else { 0x2001 };
            assert_eq!(
                timed.step_instruction(&mut bus),
                Err(TimingFault::AddressError { address })
            );
            assert_eq!(
                timed.registers.usp,
                if mode == 3 { 0x2003 } else { address }
            );
            assert_eq!(bus.transfers, 0);
            assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
        }
    }
}

#[test]
fn newly_admitted_dn_internal_overflow_retains_selected_flags_and_freezes() {
    for (opcode, source, dividend) in [(0x4181, 3, 2), (0x80c1, 0, 1), (0x81c1, 0, 1)] {
        let (mut timed, mut bus, _) = seed(opcode, TRACE | EXTEND | 0x0f, source, dividend);
        timed.clock = CpuClocks(u64::MAX - 2);
        bus.clock = timed.clock.0;
        let initial = timed.clone();
        assert_eq!(
            timed.step_instruction(&mut bus),
            Err(TimingFault::ClockOverflow)
        );
        assert_eq!(timed.registers.d, initial.registers.d);
        assert_eq!(timed.registers.pc, initial.registers.pc);
        assert_eq!(timed.registers.ssp, initial.registers.ssp);
        assert_eq!(timed.registers.usp, initial.registers.usp);
        assert_eq!(
            timed.registers.status_register(),
            TRACE | EXTEND | if opcode == 0x4181 { 0 } else { crate::ZERO }
        );
        assert_eq!(timed.queued_words(), initial.queued_words());
        assert!(bus.events.is_empty());
        assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
    }
}
