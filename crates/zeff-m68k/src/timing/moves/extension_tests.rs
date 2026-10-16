use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::{Cpu, SUPERVISOR, StepOutcome};

fn extensions(mode: u16, register: u16, size: Size, source: bool) -> Vec<u16> {
    match (mode, register) {
        (5 | 6, _) | (7, 2 | 3) => vec![0x0010],
        (7, 0) => vec![if source { 0x3010 } else { 0x3018 }],
        (7, 1) => vec![0, if source { 0x3010 } else { 0x3018 }],
        (7, 4) if size == Size::Long => vec![0x8123, 0x4567],
        (7, 4) => vec![0x8001],
        _ => vec![],
    }
}

fn seed(size: Size, source: (u16, u16), dest: (u16, u16), status: u16) -> (PrefetchCpu, Bus) {
    let family = match size {
        Size::Byte => 1,
        Size::Word => 3,
        Size::Long => 2,
    };
    let opcode = (family << 12) | (dest.1 << 9) | (dest.0 << 6) | (source.0 << 3) | source.1;
    let mut words = extensions(source.0, source.1, size, true);
    words.extend(extensions(dest.0, dest.1, size, false));
    words.extend([0x7201, 0x7402]);
    let mut timed = cpu(status);
    timed.registers.d[0] = 0x8001_fff0;
    for i in 0..8 {
        timed
            .registers
            .set_address_register(i, 0x3008 + 4 * i as u32);
    }
    timed.pipeline.as_mut().unwrap().words = [opcode, words[0]];
    let mut bus = Bus::default();
    for address in (0x3000..0x3050).step_by(2) {
        bus.words.insert(address, (address as u16).wrapping_mul(13));
    }
    let pc = timed.registers.pc & BUS_ADDRESS_MASK;
    for address in (pc - 16..pc + 64).step_by(2) {
        bus.words.insert(address, 0x4e71);
    }
    bus.words.insert(pc, opcode);
    for (offset, word) in words.iter().enumerate() {
        bus.words.insert(pc + 2 + 2 * offset as u32, *word);
    }
    (timed, bus)
}

#[test]
fn all_legal_move_address_shapes_match_full_state_memory_and_nominal_work() {
    let mut sources: Vec<_> = (0..=6).map(|mode| (mode, 0)).collect();
    sources.extend((0..=4).map(|register| (7, register)));
    let mut destinations: Vec<_> = (0..=6).map(|mode| (mode, 1)).collect();
    destinations.extend([(7, 0), (7, 1)]);
    for size in [Size::Byte, Size::Word, Size::Long] {
        for &source in &sources {
            for &dest in &destinations {
                if size == Size::Byte && (source.0 == 1 || dest.0 == 1) {
                    continue;
                }
                for status in [0x1f, SUPERVISOR | 0x1f] {
                    for wait in [0, 1] {
                        let (mut timed, mut bus) = seed(size, source, dest, status);
                        let opcode = timed.queued_words().unwrap()[0];
                        let mut raw = crate::test_bus::TestBus::default();
                        for (&address, &value) in &bus.words {
                            raw.word(address, value);
                        }
                        let mut arch = Cpu {
                            registers: timed.registers,
                            stopped: false,
                        };
                        let expected = arch.step(&mut raw);
                        bus.wait = wait;
                        let observed = timed.step_instruction(&mut bus).unwrap();
                        let waited = bus.transfers as u64 * u64::from(wait);
                        assert!(
                            matches!(expected, StepOutcome::Executed { nominal_cycles, .. } if observed.clocks == u64::from(nominal_cycles) + waited),
                            "op={opcode:04x}"
                        );
                        assert_eq!(timed.registers, arch.registers, "op={opcode:04x}");
                        for (&address, &value) in &bus.words {
                            assert_eq!(
                                value,
                                u16::from_be_bytes([raw.peek(address), raw.peek(address + 1)]),
                                "op={opcode:04x},address={address:x}"
                            );
                        }
                        assert_eq!(timed.queued_words(), Some([0x7201, 0x7402]));
                        let mut next = 0x340104;
                        for event in bus.events {
                            if let Event::Read(a, _) = event {
                                if a.kind == AccessKind::Prefetch {
                                    assert_eq!(a.bus_address, next);
                                    next += 2;
                                } else {
                                    assert_eq!(a.kind, AccessKind::Data);
                                    assert_eq!(
                                        a.function_code,
                                        if source.0 == 7 && matches!(source.1, 2 | 3) {
                                            crate::ea::program_space(&arch.registers)
                                        } else {
                                            crate::ea::data_space(&arch.registers)
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
fn absolute_long_destination_schedules_depend_on_source_bus_accesses() {
    for (source, schedule) in [
        ((0, 0), "ffwwf"),
        ((7, 4), "ffffwwf"),
        ((2, 0), "rrfwwff"),
        ((7, 2), "frrfwwff"),
        ((6, 0), "ifrrfwwff"),
    ] {
        let (mut timed, mut bus) = seed(Size::Long, source, (7, 1), 0x2000);
        timed.step_instruction(&mut bus).unwrap();
        let observed: String = bus
            .events
            .iter()
            .map(|e| match e {
                Event::Idle(_, 2) => 'i',
                Event::Read(a, _) if a.kind == AccessKind::Prefetch => 'f',
                Event::Read(_, _) => 'r',
                Event::Write(_, _, _) => 'w',
                _ => panic!(),
            })
            .collect();
        assert_eq!(observed, schedule, "source={source:?}");
    }
}

#[test]
fn pc_relative_base_is_extension_address_and_index_ignores_later_cpu_bits() {
    for indexed in [false, true] {
        for ignored in 0..8_u16 {
            let source = (7, if indexed { 3 } else { 2 });
            let (mut timed, mut bus) = seed(Size::Word, source, (0, 1), 0);
            timed.registers.d[0] = 0x2000_0004;
            let extension = if indexed {
                0x0006 | (ignored << 8)
            } else {
                0xfffe
            };
            timed.pipeline.as_mut().unwrap().words[1] = extension;
            let address = if indexed { 0x1234_010c } else { 0x1234_0100 };
            bus.words.insert(address & BUS_ADDRESS_MASK, 0x8123);
            timed.step_instruction(&mut bus).unwrap();
            assert_eq!(timed.registers.d[1], 0x8123);
            assert!(bus.events.iter().any(|e| matches!(e, Event::Read(a, _) if a.kind == AccessKind::Data && a.logical_address == address && a.function_code == FunctionCode::UserProgram)));
        }
    }
}

#[test]
fn extension_failure_freezes_and_late_refill_keeps_completed_destination_writes() {
    let (mut complete, mut full) = seed(Size::Long, (3, 0), (7, 1), 0xa000);
    complete.step_instruction(&mut full).unwrap();
    assert_eq!(full.transfers, 7);
    for fail in 0..7 {
        let (mut timed, mut bus) = seed(Size::Long, (3, 0), (7, 1), 0xa000);
        let old_pc = timed.registers.pc;
        let original = bus.words.clone();
        bus.fail = Some(fail);
        bus.wait = 1;
        assert!(matches!(timed.step(&mut bus), Err(TimingFault::Bus { .. })));
        assert_eq!(timed.registers.pc, old_pc);
        assert_eq!(
            timed.registers.a[0],
            if fail == 0 { 0x3008 } else { 0x300c }
        );
        assert!(!timed.trace_pending());
        for (write_index, address) in [(3, 0x3018), (4, 0x301a)] {
            assert_eq!(
                bus.words[&address],
                if fail > write_index {
                    full.words[&address]
                } else {
                    original[&address]
                }
            );
        }
        let before = bus.events.clone();
        assert_eq!(timed.step(&mut bus), Err(TimingFault::Frozen));
        assert_eq!(bus.events, before);
    }
}

#[test]
fn updated_a7_supplies_destination_displacement_and_long_index_in_both_banks() {
    for status in [0x1f, SUPERVISOR | 0x1f] {
        for mode in [5, 6] {
            let (initial, mut initial_bus) = seed(Size::Long, (3, 7), (mode, 7), status);
            let mut initial = initial;
            initial.registers.set_address_register(7, 0x1200_3010);
            initial.pipeline.as_mut().unwrap().words[1] = if mode == 6 { 0xf800 } else { 0x0010 };
            initial_bus.words.insert(0x3010, 0x8123);
            initial_bus.words.insert(0x3012, 0x4567);
            let expected_address = if mode == 6 { 0x2400_6028 } else { 0x1200_3024 };
            let mut timed = initial.clone();
            let mut bus = Bus {
                words: initial_bus.words.clone(),
                ..Bus::default()
            };
            timed.step(&mut bus).unwrap();
            assert_eq!(timed.registers.stack_pointer(), 0x1200_3014);
            assert!(bus.events.iter().any(
                |e| matches!(e, Event::Write(a, 0x8123, _) if a.logical_address == expected_address)
            ));
            assert!(bus.events.iter().any(|e| matches!(e, Event::Write(a, 0x4567, _) if a.logical_address == expected_address + 2)));
            let mut failed = initial;
            initial_bus.fail = Some(2);
            assert!(matches!(
                failed.step(&mut initial_bus),
                Err(TimingFault::Bus { .. })
            ));
            assert_eq!(failed.registers.stack_pointer(), 0x1200_3014);
            assert!(
                !initial_bus
                    .events
                    .iter()
                    .any(|e| matches!(e, Event::Write(..)))
            );
            assert_eq!(
                failed.registers.usp,
                if status & SUPERVISOR == 0 {
                    0x1200_3014
                } else {
                    0x2000
                }
            );
            assert_eq!(
                failed.registers.ssp,
                if status & SUPERVISOR != 0 {
                    0x1200_3014
                } else {
                    0x1000
                }
            );
        }
    }
}

#[test]
fn absolute_long_self_modification_preserves_source_specific_opcode_refill_order() {
    for source in [(0, 0), (2, 0), (7, 4)] {
        let (mut timed, mut bus) = seed(Size::Word, source, (7, 1), 0x2000);
        let source_extensions = u32::from(source == (7, 4));
        let destination_extension = timed.registers.pc + 2 + source_extensions * 2;
        let next_pc = destination_extension + 4;
        bus.words.insert(
            destination_extension & BUS_ADDRESS_MASK,
            (next_pc >> 16) as u16,
        );
        bus.words.insert(
            (destination_extension + 2) & BUS_ADDRESS_MASK,
            next_pc as u16,
        );
        if source_extensions == 0 {
            timed.pipeline.as_mut().unwrap().words[1] = (next_pc >> 16) as u16;
        } else {
            timed.pipeline.as_mut().unwrap().words[1] = 0xbabe;
        }
        timed.registers.d[0] = 0xbabe;
        bus.words.insert(0x3008, 0xbabe);
        timed.step_instruction(&mut bus).unwrap();
        assert_eq!(timed.registers.pc, next_pc);
        assert_eq!(bus.words[&(next_pc & BUS_ADDRESS_MASK)], 0xbabe);
        assert_eq!(
            timed.queued_words(),
            Some([if source == (2, 0) { 0xbabe } else { 0x7201 }, 0x7402])
        );
    }
}
