use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::{Cpu, SUPERVISOR, StepOutcome};

#[test]
fn pure_register_forms_match_existing_semantics_with_explicit_schedule() {
    let cases = [
        (0xd001, 7, 3, 4, false),
        (0xd041, 7, 3, 4, false),
        (0xd081, 7, 3, 8, false),
        (0x9041, 7, 3, 4, false),
        (0xb081, 7, 3, 6, false),
        (0xd1c1, 7, 3, 8, false),
        (0xb0c1, 7, 3, 6, false),
        (0x5280, 7, 3, 8, false),
        (0x5288, 7, 3, 8, false),
        (0x4280, 7, 3, 6, false),
        (0x4a80, 7, 3, 4, false),
        (0x4880, 7, 3, 4, false),
        (0x4840, 7, 3, 4, false),
        (0xc141, 7, 3, 6, false),
        (0xc101, 7, 3, 6, false),
        (0x8101, 7, 3, 6, false),
        (0x4800, 7, 3, 6, false),
        (0xd181, 7, 3, 8, false),
        (0x9181, 7, 3, 8, false),
        (0xe360, 7, 3, 12, false),
        (0x0300, 7, 3, 6, false),
        (0x03c0, 7, 16, 8, false),
        (0x0380, 7, 16, 10, false),
        (0xc0c1, 7, 3, 42, false),
        (0x80c1, 7, 3, 134, true),
        (0x80c1, 65536, 1, 10, true),
        (0x81c1, 7, 3, 148, true),
    ];
    for (opcode, d0, d1, cycles, late) in cases {
        for waits in [0, 1, 3] {
            let mut timed = cpu(0x201f);
            timed.registers.d[0] = d0;
            timed.registers.d[1] = d1;
            timed.pipeline.as_mut().unwrap().words = [opcode, 0x7201];
            let mut architectural = Cpu {
                registers: timed.registers,
                stopped: false,
            };
            let mut raw = crate::test_bus::TestBus::default();
            raw.word(timed.registers.pc, opcode);
            let observed = architectural.step(&mut raw);
            assert!(
                matches!(observed, StepOutcome::Executed { nominal_cycles, .. } if nominal_cycles == cycles),
                "opcode={opcode:04x}: {observed:?}, expected {cycles}"
            );
            assert_eq!(raw.transfers.len(), 1);
            let mut bus = Bus {
                wait: waits,
                ..Bus::default()
            };
            assert_eq!(
                timed.step(&mut bus).unwrap().clocks,
                u64::from(cycles + waits)
            );
            assert_eq!(timed.registers, architectural.registers);
            assert_eq!(timed.queued_words(), Some([0x7201, 0x4e71]));
            let read = bus
                .events
                .iter()
                .find_map(|event| match event {
                    Event::Read(access, start) => Some((access.logical_address, *start)),
                    _ => None,
                })
                .unwrap();
            assert_eq!(
                read,
                (0x1234_0104, if late { u64::from(cycles - 4) } else { 0 })
            );
            assert_eq!(bus.transfers, 1);
        }
    }
}

#[test]
fn side_effecting_aliases_and_exception_operands_never_touch_timed_bus() {
    for opcode in [0x4188, 0x80c8, 0x81c8, 0x5208, 0x4448, 0x4888] {
        let mut timed = cpu(0x2000);
        timed.pipeline.as_mut().unwrap().words[0] = opcode;
        let original = timed.clone();
        let mut bus = Bus::default();
        assert_eq!(
            timed.step(&mut bus),
            Err(TimingFault::UnsupportedInstruction {
                pc: 0x1234_0100,
                opcode
            })
        );
        assert_eq!(timed, original);
        assert!(bus.events.is_empty());
    }
}

#[test]
fn variable_counts_are_taken_before_aliased_register_writes() {
    for opcode in [0xe1a0, 0x01c0, 0xc0c0, 0x80c0] {
        for original in [1, 16, 31, 32, 63, 0x8001, 0xffff] {
            let mut timed = cpu(0x201f);
            timed.registers.d[0] = original;
            timed.pipeline.as_mut().unwrap().words[0] = opcode;
            let mut arch = Cpu {
                registers: timed.registers,
                stopped: false,
            };
            let mut raw = crate::test_bus::TestBus::default();
            raw.word(timed.registers.pc, opcode);
            let StepOutcome::Executed { nominal_cycles, .. } = arch.step(&mut raw) else {
                panic!("unexpected exception")
            };
            let mut bus = Bus::default();
            assert_eq!(
                timed.step(&mut bus).unwrap().clocks,
                u64::from(nominal_cycles)
            );
            assert_eq!(timed.registers, arch.registers);
            assert_eq!(bus.transfers, 1);
        }
    }
}

#[test]
fn early_and_late_prefetch_failure_retains_elapsed_time_without_replay() {
    for (opcode, before_fetch) in [(0xd081, 0), (0x80c1, 130)] {
        let mut timed = cpu(0x2000);
        timed.pipeline.as_mut().unwrap().words[0] = opcode;
        timed.registers.d[0] = 7;
        timed.registers.d[1] = 3;
        let original = timed.registers;
        let mut bus = Bus {
            fail: Some(0),
            wait: 3,
            ..Bus::default()
        };
        assert!(matches!(timed.step(&mut bus), Err(TimingFault::Bus { .. })));
        assert_eq!(timed.clock().0, before_fetch + 7);
        assert_eq!(timed.registers, original);
        let events = bus.events.clone();
        assert_eq!(timed.step(&mut bus), Err(TimingFault::Frozen));
        assert_eq!(bus.events, events);
    }
}

fn compare(mut timed: PrefetchCpu, opcode: u16) {
    timed.pipeline.as_mut().unwrap().words[0] = opcode;
    let mut arch = Cpu {
        registers: timed.registers,
        stopped: false,
    };
    let mut raw = crate::test_bus::TestBus::default();
    raw.word(timed.registers.pc, opcode);
    let result = arch.step(&mut raw);
    let StepOutcome::Executed { nominal_cycles, .. } = result else {
        panic!("opcode={opcode:04x}: {result:?}")
    };
    let mut bus = Bus::default();
    let step = timed
        .step(&mut bus)
        .unwrap_or_else(|error| panic!("opcode={opcode:04x}: {error:?}"));
    assert_eq!(timed.registers, arch.registers, "opcode={opcode:04x}");
    assert_eq!(
        step.clocks,
        u64::from(nominal_cycles),
        "opcode={opcode:04x}"
    );
    assert_eq!(bus.transfers, 1);
}

#[test]
fn all_admitted_opcode_encodings_are_register_only_in_both_banks() {
    for opcode in 0..=u16::MAX {
        if route(opcode).is_none() {
            continue;
        }
        for supervisor in [0, SUPERVISOR] {
            let mut timed = cpu(supervisor | 0x1f);
            for (index, value) in timed.registers.d.iter_mut().enumerate() {
                *value = 0x8001_0001 + index as u32;
            }
            for (index, value) in timed.registers.a.iter_mut().enumerate() {
                *value = 0x8002_0001 + index as u32;
            }
            timed.registers.usp = 0xabcd_0007;
            timed.registers.ssp = 0xbcde_0008;
            compare(timed, opcode);
        }
    }
}

#[test]
fn a7_arithmetic_and_exchange_use_the_active_bank() {
    for supervisor in [0, SUPERVISOR] {
        for opcode in [0xd0cf, 0x9fc8, 0xb1cf, 0xc14f, 0xc18f, 0xdfc0] {
            let mut timed = cpu(supervisor | 0x1f);
            timed.registers.d[0] = 0x8001_ffff;
            timed.registers.a[0] = 0x1234_5678;
            timed.registers.usp = 0x2345_8000;
            timed.registers.ssp = 0x3456_7fff;
            compare(timed, opcode);
        }
    }
}

#[test]
fn counts_sign_quadrants_and_sticky_zero_cover_extreme_operands() {
    for opcode in [
        0xe1a0, 0xe020, 0xe060, 0xe0a0, 0xe038, 0xe078, 0xe0b8, 0xc1c0, 0x81c1, 0xd180, 0x9180,
    ] {
        for source in [0, 1, 8, 16, 31, 32, 63, 0x7fff, 0x8000, 0xffff] {
            for destination in [0, 1, 0xffff_ffff, i32::MIN as u32, 0x7fff_ffff] {
                for flags in [0, crate::ZERO | crate::EXTEND] {
                    if opcode == 0x81c1 && source == 0 {
                        continue;
                    }
                    let mut timed = cpu(0x2000 | flags);
                    timed.registers.d[0] = destination;
                    timed.registers.d[1] = source;
                    compare(timed, opcode);
                }
            }
        }
    }
}

#[test]
fn trace_follows_postprefetch_internal_work() {
    let mut timed = cpu(0xa000);
    timed.pipeline.as_mut().unwrap().words[0] = 0xd081;
    timed.registers.d[0] = 1;
    timed.registers.d[1] = 2;
    let mut bus = Bus::default();
    bus.handler(9);
    let step = timed.step(&mut bus).unwrap();
    assert_eq!(step.clocks, 42);
    assert_eq!(step.exception, Some(9));
    assert!(matches!(bus.events[0], Event::Read(_, 0)));
    assert_eq!(bus.events[1], Event::Idle(4, 4));
    assert_eq!(bus.events[2], Event::Idle(8, 4));
    assert!(matches!(bus.events[3], Event::Write(_, _, 12)));
    assert_eq!(timed.registers.d[0], 3);
}
