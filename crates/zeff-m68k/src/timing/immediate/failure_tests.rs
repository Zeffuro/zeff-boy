use super::tests::seed;
use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, Event, cpu};

#[test]
fn every_ordinary_transfer_failure_preserves_only_completed_work_and_freezes() {
    for family in [0, 0x0200, 0x0400, 0x0600, 0x0a00, 0x0c00] {
        for size_bits in 0..3 {
            for mode in [0, 2, 3, 4, 5, 6, 7] {
                for register in if mode == 7 { vec![0, 1] } else { vec![7] } {
                    let opcode = family | size_bits << 6 | mode << 3 | register;
                    let size = crate::instruction::size(opcode).unwrap();
                    for status in [0x1f, SUPERVISOR | 0x1f] {
                        let (mut completed, mut full, _) =
                            seed(opcode, status, false, (0x8123_4567, 0x0102_0304));
                        full.wait = 1;
                        completed.step_instruction(&mut full).unwrap();
                        let transfers: Vec<_> = full
                            .events
                            .iter()
                            .filter(|event| !matches!(event, Event::Idle(..)))
                            .collect();
                        let first_read = transfers.iter().position(|event| matches!(event, Event::Read(a, _) if a.kind == AccessKind::Data));
                        let last_input = transfers.iter().rposition(|event| matches!(event, Event::Read(a, _) if a.kind == AccessKind::Data)).unwrap_or(usize::from(size == Size::Long));
                        for fail in 0..full.transfers {
                            let (mut timed, mut bus, _) =
                                seed(opcode, status, false, (0x8123_4567, 0x0102_0304));
                            let initial_queue = timed.queued_words();
                            let mut expected = timed.registers;
                            let mut memory = bus.words.clone();
                            bus.fail = Some(fail);
                            bus.wait = 1;
                            assert!(matches!(
                                timed.step_instruction(&mut bus),
                                Err(TimingFault::Bus { .. })
                            ));
                            if matches!(mode, 3 | 4)
                                && fail
                                    >= first_read.unwrap()
                                        + usize::from(mode == 3 && size == Size::Long)
                            {
                                expected
                                    .set_address_register(7, completed.registers.stack_pointer());
                            }
                            if fail > last_input {
                                expected.set_status_register(completed.registers.status_register());
                                if mode == 0 {
                                    expected.d[register as usize] =
                                        completed.registers.d[register as usize];
                                }
                            }
                            for event in transfers.iter().take(fail) {
                                if let Event::Write(a, value, _) = event {
                                    let old = memory[&a.bus_address];
                                    memory.insert(
                                        a.bus_address,
                                        match a.lanes {
                                            ByteLanes::Both => *value,
                                            ByteLanes::Upper => (old & 0xff) | (value & 0xff00),
                                            ByteLanes::Lower => (old & 0xff00) | (value & 0xff),
                                        },
                                    );
                                }
                            }
                            assert_eq!(timed.registers, expected, "op={opcode:04x} fail={fail}");
                            assert_eq!(timed.queued_words(), initial_queue);
                            assert_eq!(bus.words, memory);
                            assert_eq!(timed.clock().0, bus.clock);
                            assert!(!timed.trace_pending());
                            let events = bus.events.clone();
                            assert_eq!(timed.step_boundary(&mut bus), Err(TimingFault::Frozen));
                            assert_eq!(bus.events, events);
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn ccr_transfer_failures_have_the_verified_commit_boundary_and_no_retry() {
    for opcode in [0x003c, 0x023c, 0x0a3c] {
        for status in [crate::TRACE | 0x11, crate::TRACE | SUPERVISOR | 0x11] {
            for fail in 0..3 {
                let mut timed = cpu(status);
                timed.pipeline.as_mut().unwrap().words = [opcode, 0x00ff];
                let initial = timed.clone();
                let mut bus = Bus {
                    fail: Some(fail),
                    wait: 3,
                    ..Bus::default()
                };
                assert!(matches!(
                    timed.step_instruction(&mut bus),
                    Err(TimingFault::Bus { .. })
                ));
                let expected_ccr = match opcode {
                    0x003c => 0x1f,
                    0x023c => 0x11,
                    _ => 0x0e,
                };
                assert_eq!(
                    timed.registers.status_register(),
                    if fail == 0 {
                        status
                    } else {
                        (status & !0x1f) | expected_ccr
                    }
                );
                assert_eq!(timed.registers.pc, initial.registers.pc);
                assert_eq!(timed.registers.ssp, initial.registers.ssp);
                assert_eq!(timed.registers.usp, initial.registers.usp);
                assert_eq!(timed.queued_words(), initial.queued_words());
                assert!(!timed.trace_pending());
                assert_eq!(timed.clock().0, bus.clock);
                let events = bus.events.clone();
                assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
                assert_eq!(bus.events, events);
            }
        }
    }
}

#[test]
fn illegal_eas_reserved_sizes_and_non_ccr_forms_reject_before_callbacks() {
    for family in [0, 0x0200, 0x0400, 0x0600, 0x0a00, 0x0c00] {
        for size in 0..4 {
            for operand in 0..64 {
                let opcode = family | size << 6 | operand;
                if matches!(opcode, 0x003c | 0x023c | 0x0a3c | 0x007c | 0x027c | 0x0a7c) {
                    continue;
                }
                let mode = operand >> 3;
                if size < 3 && mode != 1 && (mode != 7 || operand & 7 <= 1) {
                    continue;
                }
                for status in [0, SUPERVISOR] {
                    let mut timed = cpu(status);
                    timed.pipeline.as_mut().unwrap().words[0] = opcode;
                    let initial = timed.clone();
                    let mut bus = Bus::default();
                    assert!(
                        matches!(
                            timed.step_instruction(&mut bus),
                            Err(TimingFault::UnsupportedInstruction { .. })
                        ),
                        "op={opcode:04x}"
                    );
                    assert_eq!(timed, initial);
                    assert!(bus.events.is_empty());
                }
            }
        }
    }
    for opcode in [0x007c, 0x027c, 0x0a7c] {
        let mut timed = cpu(crate::TRACE | 0x1f);
        timed.pipeline.as_mut().unwrap().words = [opcode, 0xffff];
        let mut bus = Bus::default();
        bus.handler(8);
        let observed = timed.step_instruction(&mut bus).unwrap();
        assert_eq!(observed.exception, Some(8));
        assert_eq!(observed.clocks, 34);
        assert!(!timed.trace_pending());
        assert!(
            !bus.events.iter().any(
                |event| matches!(event, Event::Read(a, _) if a.logical_address == 0x1234_0104)
            )
        );
    }
}

#[test]
fn self_modifying_tail_wrap_odd_address_and_trace_preserve_stream_contract() {
    let (mut timed, mut bus, _) = seed(0x0650, SUPERVISOR, false, (0x1234, 1));
    let tail = timed.registers.pc.wrapping_add(6);
    timed.registers.a[0] = tail;
    bus.words.insert(tail & BUS_ADDRESS_MASK, 0x1234);
    timed.step_instruction(&mut bus).unwrap();
    assert_eq!(timed.queued_words(), Some([0x7201, 0x1234]));
    assert_eq!(bus.words[&(tail & BUS_ADDRESS_MASK)], 0x1235);
    let (mut timed, mut bus, _) = seed(0x0690, SUPERVISOR, false, (0, 0x0102_0304));
    timed.registers.a[0] = 0xffff_fffe;
    bus.words.insert(0xfffffe, 0x1122);
    bus.words.insert(0, 0x3344);
    timed.step_instruction(&mut bus).unwrap();
    let writes: Vec<_> = bus
        .events
        .iter()
        .filter_map(|event| match event {
            Event::Write(a, word, _) => Some((a.logical_address, a.bus_address, *word)),
            _ => None,
        })
        .collect();
    assert_eq!(writes, [(0, 0, 0x3648), (0xffff_fffe, 0xfffffe, 0x1224)]);
    let (mut timed, mut bus, _) = seed(0x0650, SUPERVISOR, false, (0, 1));
    timed.registers.a[0] = 0x0100_0001;
    assert_eq!(
        timed.step_instruction(&mut bus),
        Err(TimingFault::AddressError {
            address: 0x0100_0001
        })
    );
    assert_eq!(bus.transfers, 1);
    assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
    for opcode in [0x0690, 0x0c90, 0x0680, 0x0c80, 0x003c] {
        let mut timed;
        let mut bus;
        if opcode == 0x003c {
            timed = cpu(crate::TRACE | SUPERVISOR | 0x11);
            timed.pipeline.as_mut().unwrap().words = [opcode, 0x000f];
            bus = Bus::default();
        } else {
            (timed, bus, _) = seed(
                opcode,
                crate::TRACE | SUPERVISOR | 0x11,
                false,
                (0x8123_4567, 0x0102_0304),
            );
        }
        bus.handler(9);
        timed.step_instruction(&mut bus).unwrap();
        assert!(timed.trace_pending());
        let flags = timed.registers.status_register();
        let sp = timed.registers.ssp;
        let boundary = bus.events.len();
        assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), Some(34));
        assert!(matches!(bus.events[boundary], Event::Idle(_, 4)));
        assert_eq!(bus.words[&(sp.wrapping_sub(6) & BUS_ADDRESS_MASK)], flags);
        let events = bus.events.clone();
        assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), None);
        assert_eq!(bus.events, events);
    }
}
