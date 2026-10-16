use super::tests::seed;
use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, Event, cpu};

#[test]
fn every_transfer_failure_keeps_completed_address_alu_and_bus_effects_then_freezes() {
    for family in [8, 9, 11, 12, 13] {
        for opmode in 0..8 {
            for mode in 2..8 {
                for register in if mode == 7 {
                    vec![0, 1, 2, 3, 4]
                } else {
                    vec![7]
                } {
                    let opcode = family << 12 | 7 << 9 | opmode << 6 | mode << 3 | register;
                    let Some(operation) = decode(opcode) else {
                        continue;
                    };
                    let Binary {
                        size, destination, ..
                    } = operation;
                    for status in [0x1f, SUPERVISOR | 0x1f] {
                        let (mut completed, mut full, _) = seed(opcode, status, false, 0x8123_4567);
                        full.wait = 1;
                        completed.step_instruction(&mut full).unwrap();
                        let transfers: Vec<_> = full
                            .events
                            .iter()
                            .filter(|event| !matches!(event, Event::Idle(..)))
                            .collect();
                        let last_input = if mode == 7 && register == 4 {
                            usize::from(size == Size::Long)
                        } else {
                            transfers.iter().rposition(|event| matches!(event, Event::Read(a, _) if a.kind == AccessKind::Data)).unwrap()
                        };
                        for fail in 0..full.transfers {
                            let (mut timed, mut bus, _) = seed(opcode, status, false, 0x8123_4567);
                            let initial = timed.clone();
                            let mut expected = timed.registers;
                            let mut memory = bus.words.clone();
                            bus.fail = Some(fail);
                            bus.wait = 1;
                            assert!(matches!(
                                timed.step_instruction(&mut bus),
                                Err(TimingFault::Bus { .. })
                            ));
                            if mode == 4 || mode == 3 && (size != Size::Long || fail > 0) {
                                let delta = if size == Size::Byte { 2 } else { size.bytes() };
                                expected.set_address_register(
                                    register as usize,
                                    if mode == 4 {
                                        expected.stack_pointer().wrapping_sub(delta)
                                    } else {
                                        expected.stack_pointer().wrapping_add(delta)
                                    },
                                );
                            }
                            if fail > last_input {
                                expected.set_status_register(completed.registers.status_register());
                                match destination {
                                    Destination::Data(register) => {
                                        let result = completed.registers.d[register];
                                        expected.d[register] = if size == Size::Long
                                            && matches!(family, 8 | 9 | 12 | 13)
                                            && !(mode == 7 && operation.register == 4)
                                        {
                                            (expected.d[register] & 0xffff_0000) | (result & 0xffff)
                                        } else {
                                            result
                                        };
                                    }
                                    Destination::Address(register) => {
                                        let result =
                                            completed.registers.address_register(register).unwrap();
                                        let value = if family != 11
                                            && !(mode == 7 && operation.register == 4)
                                        {
                                            (expected.address_register(register).unwrap()
                                                & 0xffff_0000)
                                                | (result & 0xffff)
                                        } else {
                                            result
                                        };
                                        expected.set_address_register(register, value);
                                    }
                                    Destination::Memory => {}
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
                            assert_eq!(timed.queued_words(), initial.queued_words());
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
    for opcode in [0x501f, 0x515f, 0x50a7, 0x51b9] {
        let (mut completed, mut full, address) =
            seed(opcode, SUPERVISOR | 0x1f, false, 0x8123_4567);
        completed.step_instruction(&mut full).unwrap();
        for fail in 0..full.transfers {
            let (mut timed, mut bus, _) = seed(opcode, SUPERVISOR | 0x1f, false, 0x8123_4567);
            bus.fail = Some(fail);
            assert!(matches!(
                timed.step_instruction(&mut bus),
                Err(TimingFault::Bus { .. })
            ));
            if opcode == 0x50a7 && fail == 4 {
                assert_eq!(
                    bus.words[&(address.wrapping_add(2) & BUS_ADDRESS_MASK)],
                    full.words[&(address.wrapping_add(2) & BUS_ADDRESS_MASK)]
                );
                assert_eq!(bus.words[&(address & BUS_ADDRESS_MASK)], 0x8123);
            }
            let events = bus.events.clone();
            assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
            assert_eq!(bus.events, events);
        }
    }
}

#[test]
fn prefetch_samples_old_memory_before_rmw_and_word_addresses_wrap_or_fail() {
    for opcode in [0xd150, 0xc190, 0xb190, 0x5090] {
        let (mut timed, mut bus, _) = seed(opcode, SUPERVISOR, false, 0x1234_5678);
        let tail = timed.registers.pc.wrapping_add(4);
        timed.registers.a[0] = tail;
        bus.words.insert(tail & BUS_ADDRESS_MASK, 0x1234);
        bus.words
            .insert(tail.wrapping_add(2) & BUS_ADDRESS_MASK, 0x5678);
        timed.step_instruction(&mut bus).unwrap();
        assert_eq!(timed.queued_words(), Some([0x7201, 0x1234]));
        assert!(
            bus.words[&(tail & BUS_ADDRESS_MASK)] != 0x1234
                || bus.words[&(tail.wrapping_add(2) & BUS_ADDRESS_MASK)] != 0x5678
        );
    }
    let (mut timed, mut bus, _) = seed(0xd190, SUPERVISOR, false, 0);
    timed.registers.a[0] = 0xffff_fffe;
    timed.registers.d[0] = 0x0102_0304;
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
    for opcode in [0xd050, 0xb090, 0xd0d0, 0xd150, 0x5090] {
        let (mut timed, mut bus, _) = seed(opcode, SUPERVISOR, false, 0);
        timed.registers.a[0] = 0x0100_0001;
        assert_eq!(
            timed.step_instruction(&mut bus),
            Err(TimingFault::AddressError {
                address: 0x0100_0001
            })
        );
        assert!(bus.events.is_empty());
        assert_eq!(timed.step_instruction(&mut bus), Err(TimingFault::Frozen));
    }
}

#[test]
fn trace_follows_result_writes_and_post_fetch_idle_with_flags_in_saved_frame() {
    for opcode in [0xd190, 0xb090, 0xd0d0, 0x5090, 0xd0bc] {
        for status in [crate::TRACE | 0x1f, crate::TRACE | SUPERVISOR | 0x1f] {
            let (mut timed, mut bus, _) = seed(opcode, status, false, 0x8123_4567);
            bus.handler(9);
            timed.step_instruction(&mut bus).unwrap();
            assert!(timed.trace_pending());
            let flags = timed.registers.status_register();
            let sp = timed.registers.ssp;
            let boundary = bus.events.len();
            assert!(matches!(
                bus.events.last(),
                Some(Event::Write(..) | Event::Idle(..))
            ));
            assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), Some(34));
            assert_eq!(bus.words[&(sp.wrapping_sub(6) & BUS_ADDRESS_MASK)], flags);
            assert!(matches!(bus.events[boundary], Event::Idle(_, 4)));
            let events = bus.events.clone();
            assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), None);
            assert_eq!(bus.events, events);
        }
    }
    let mut timed = cpu(SUPERVISOR);
    timed.pipeline.as_mut().unwrap().words[0] = 0xd010;
    timed.registers.a[0] = 0x101;
    let mut bus = Bus::default();
    bus.words.insert(0x100, 0xa580);
    timed.step_instruction(&mut bus).unwrap();
    assert_eq!(timed.registers.d[0] & 0xff, 0x80);
    assert!(matches!(bus.events[0], Event::Read(a, _) if a.lanes == ByteLanes::Lower));
}
