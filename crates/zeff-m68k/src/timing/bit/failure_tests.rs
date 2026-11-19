use super::tests::{opcodes, seed};
use super::*;
use crate::SUPERVISOR;
use crate::timing::test_bus::{Bus, Event, cpu};

#[test]
fn every_bit_transfer_failure_preserves_operand_updates_result_and_completed_writes() {
    for opcode in opcodes()
        .into_iter()
        .filter(|opcode| opcode & 7 == 7 || opcode & 0x38 == 0x38)
    {
        let mode = (opcode >> 3) & 7;
        let register = opcode & 7;
        for status in [0x1f, SUPERVISOR | 0x1f] {
            let (mut completed, mut full, _) = seed(opcode, status, false, (0x8001_00a5, 17));
            full.wait = 1;
            completed.step_instruction(&mut full).unwrap();
            let transfers: Vec<_> = full
                .events
                .iter()
                .filter(|event| !matches!(event, Event::Idle(..)))
                .collect();
            let data_read = transfers
                .iter()
                .position(|event| matches!(event, Event::Read(a, _) if a.kind == AccessKind::Data));
            let last_input = data_read.unwrap_or(0);
            for fail in 0..full.transfers {
                let (mut timed, mut bus, _) = seed(opcode, status, false, (0x8001_00a5, 17));
                let queue = timed.queued_words();
                let mut expected = timed.registers;
                let mut memory = bus.words.clone();
                bus.fail = Some(fail);
                bus.wait = 1;
                assert!(matches!(
                    timed.step_instruction(&mut bus),
                    Err(TimingFault::Bus { .. })
                ));
                if matches!(mode, 3 | 4) && fail >= data_read.unwrap() {
                    expected.set_address_register(7, completed.registers.stack_pointer());
                }
                if fail > last_input {
                    expected.set_status_register(completed.registers.status_register());
                    if mode == 0 {
                        expected.d[register as usize] = completed.registers.d[register as usize];
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
                assert_eq!(timed.queued_words(), queue);
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

#[test]
fn unsupported_bit_shapes_are_rejected_before_callbacks() {
    for base in [0x0800, 0x0700] {
        for operation in 0..4 {
            for mode in [1, 7] {
                for register in 0..8 {
                    let valid = mode == 7
                        && register
                            <= if operation == 0 {
                                if base == 0x0800 { 3 } else { 4 }
                            } else {
                                1
                            };
                    if valid || (base == 0x0700 && mode == 1) {
                        continue;
                    }
                    let opcode = base | operation << 6 | mode << 3 | register;
                    let mut timed = cpu(SUPERVISOR);
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
}

#[test]
fn aliased_count_index_and_self_modifying_byte_tail_use_saved_inputs() {
    let (mut timed, mut bus, _) = seed(0x0377, SUPERVISOR | 0x1f, false, (8, 3));
    timed.registers.d[1] = 3;
    timed.pipeline.as_mut().unwrap().words[1] = 0x1001;
    let base = timed.registers.stack_pointer();
    bus.words.insert(0x340102, 0x1001);
    bus.words.insert((base + 4) & BUS_ADDRESS_MASK, 0x085a);
    timed.step_instruction(&mut bus).unwrap();
    assert_eq!(timed.registers.d[1], 3);
    assert_eq!(timed.registers.stack_pointer(), base);
    assert_eq!(timed.registers.status_register(), SUPERVISOR | 0x1b);
    assert_eq!(bus.words[&((base + 4) & BUS_ADDRESS_MASK)], 0x005a);
    let (mut timed, mut bus, _) = seed(0x07d0, SUPERVISOR, false, (0, 7));
    let tail = timed.registers.pc + 4;
    timed.registers.a[0] = tail;
    bus.words.insert(tail & BUS_ADDRESS_MASK, 0x1234);
    timed.step_instruction(&mut bus).unwrap();
    assert_eq!(timed.queued_words(), Some([0x7201, 0x1234]));
    assert_eq!(bus.words[&(tail & BUS_ADDRESS_MASK)], 0x9234);
    let (mut timed, mut bus, _) = seed(0x0750, SUPERVISOR, false, (0x80, 7));
    timed.registers.a[0] = 0xffff_ffff;
    bus.words.insert(0xfffffe, 0xa580);
    timed.step_instruction(&mut bus).unwrap();
    assert_eq!(bus.words[&0xfffffe], 0xa500);
    assert!(
        matches!(bus.events.last(), Some(Event::Write(a, 0, _)) if a.logical_address == 0xffff_ffff && a.lanes == ByteLanes::Lower)
    );
}

#[test]
fn bit_trace_waits_for_writes_and_variable_register_idle_then_saves_only_z_change() {
    for opcode in [0x07d0, 0x083a, 0x083b, 0x0887, 0x073c] {
        for status in [crate::TRACE | 0x1b, crate::TRACE | SUPERVISOR | 0x1b] {
            let (mut timed, mut bus, _) = seed(opcode, status, false, (0, 17));
            bus.handler(9);
            timed.step_instruction(&mut bus).unwrap();
            assert!(timed.trace_pending());
            assert_eq!(timed.registers.status_register(), status | crate::ZERO);
            let sp = timed.registers.ssp;
            let boundary = bus.events.len();
            assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), Some(34));
            assert_eq!(
                bus.words[&(sp.wrapping_sub(6) & BUS_ADDRESS_MASK)],
                status | crate::ZERO
            );
            assert!(matches!(bus.events[boundary], Event::Idle(_, 4)));
            let events = bus.events.clone();
            assert_eq!(timed.service_pending_trace(&mut bus).unwrap(), None);
            assert_eq!(bus.events, events);
        }
    }
}
