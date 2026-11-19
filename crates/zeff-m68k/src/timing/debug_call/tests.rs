use super::*;
use crate::timing::test_bus::{Bus, Event, cpu};
use crate::timing::{InterruptSamples, Pipeline, TimedOutcome};
use crate::{EXTEND, NEGATIVE, SUPERVISOR};

pub(super) fn fixture(status: u16, words: &[u16]) -> (PrefetchCpu, Bus) {
    let cpu = cpu(status);
    let mut bus = Bus::default();
    for (index, &word) in words.iter().enumerate() {
        bus.words.insert(0x400 + index as u32 * 2, word);
    }
    (cpu, bus)
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn entry_matches_real_jsr_an_with_waits_banks_wrap_and_stack_aliases() {
    for bank in [0, SUPERVISOR] {
        for wait in [0, 1, 3] {
            for sp in [0x3000u32, 0, 2] {
                for target in [0x400, sp.wrapping_sub(4), sp.wrapping_sub(6)] {
                    let (mut actual, mut bus) = fixture(bank | 0x0715, &[]);
                    actual.registers.set_address_register(7, sp);
                    actual.registers.a[0] = target;
                    for (address, word) in [(target, 0x7201), (target.wrapping_add(2), 0x7402)] {
                        bus.words
                            .insert(address & super::super::BUS_ADDRESS_MASK, word);
                    }
                    bus.wait = wait;
                    let original_pc = actual.registers.pc;
                    let mut normal = actual.clone();
                    normal.registers.pc = original_pc.wrapping_sub(2);
                    normal.pipeline =
                        Some(Pipeline::unknown(normal.registers.pc, [0x4e90, 0x4e71]));
                    let mut normal_bus = bus.clone();
                    let context = actual.begin_debug_call(&mut bus, target).unwrap();
                    let instruction = normal.step_instruction(&mut normal_bus).unwrap();
                    assert_eq!(instruction.opcode, 0x4e90);
                    assert_eq!(instruction.clocks, 16 + 4 * u64::from(wait));
                    assert_eq!(actual, normal);
                    assert_eq!(bus, normal_bus);
                    assert_eq!(actual.clock().0, 16 + 4 * u64::from(wait));
                    assert_eq!(context.pc, original_pc);
                    assert_eq!(context.sp, sp);
                    assert_eq!(actual.registers.stack_pointer(), sp.wrapping_sub(4));
                    let shape: String = bus
                        .events
                        .iter()
                        .map(|event| match event {
                            Event::Read(..) => 'f',
                            Event::Write(..) => 'w',
                            _ => panic!("unexpected entry event"),
                        })
                        .collect();
                    assert_eq!(shape, "fwwf");
                    assert_eq!(actual.queued_words().unwrap()[0], 0x7201);
                    let expected_second = if target == sp.wrapping_sub(4) {
                        original_pc as u16
                    } else if target == sp.wrapping_sub(6) {
                        (original_pc >> 16) as u16
                    } else {
                        0x7402
                    };
                    assert_eq!(actual.queued_words().unwrap()[1], expected_second);
                    for (index, event) in bus.events.iter().enumerate() {
                        let (access, at, address) = match event {
                            Event::Read(access, at) => (
                                access,
                                at,
                                if index == 0 {
                                    target
                                } else {
                                    target.wrapping_add(2)
                                },
                            ),
                            Event::Write(access, value, at) => {
                                assert_eq!(
                                    *value,
                                    if index == 1 {
                                        (original_pc >> 16) as u16
                                    } else {
                                        original_pc as u16
                                    }
                                );
                                (access, at, sp.wrapping_sub(if index == 1 { 4 } else { 2 }))
                            }
                            _ => unreachable!(),
                        };
                        assert_eq!(access.logical_address, address);
                        assert_eq!(*at, index as u64 * (4 + u64::from(wait)));
                    }
                }
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn genuine_rts_preserves_result_registers_ccr_and_changed_return_prefetch() {
    for bank in [0, SUPERVISOR] {
        for trace in [0, TRACE] {
            let (mut actual, mut bus) =
                fixture(bank | trace | 0x0315, &[0x70ff, 0x2240, 0x4e75, 0x4e71]);
            let original = actual.clone();
            let context = actual.begin_debug_call(&mut bus, 0x400).unwrap();
            assert!(!actual.debug_call_returned(&context));
            let return_pc = original.registers.pc;
            bus.words
                .insert(return_pc & super::super::BUS_ADDRESS_MASK, 0x7405);
            bus.words.insert(
                return_pc.wrapping_add(2) & super::super::BUS_ADDRESS_MASK,
                0x7606,
            );
            for opcode in [0x70ff, 0x2240, 0x4e75] {
                actual.interrupt_input().set_level(6);
                let TimedOutcome::Instruction(step) = actual.step_boundary(&mut bus).unwrap()
                else {
                    panic!("callee interrupted");
                };
                assert_eq!(step.opcode, opcode);
                assert_eq!(step.exception, None);
            }
            assert!(actual.debug_call_returned(&context));
            let completed_clock = actual.clock();
            let completed_bus = bus.clone();
            actual.finish_debug_call(context).unwrap();
            assert_eq!(actual.registers.pc, return_pc);
            assert_eq!(
                actual.registers.stack_pointer(),
                original.registers.stack_pointer()
            );
            assert_eq!(actual.registers.d[0], u32::MAX);
            assert_eq!(actual.registers.a[1], u32::MAX);
            assert_eq!(
                actual.registers.status_register(),
                bank | trace | 0x0300 | EXTEND | NEGATIVE
            );
            assert_eq!(actual.queued_words(), Some([0x7405, 0x7606]));
            assert_eq!(actual.interrupts, original.interrupts);
            assert_eq!(actual.clock(), completed_clock);
            assert_eq!(bus, completed_bus);
            assert!(!actual.trace_pending());
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn saved_observed_accept_deny_and_legacy_decisions_survive_changed_live_levels() {
    for observation in [None, Some((4, 4)), Some((4, 6))] {
        for live in [4, 6] {
            let (mut actual, mut bus) = fixture(SUPERVISOR, &[0x7001, 0x4e75]);
            actual.interrupts.set_level(3);
            actual.interrupts.observe(
                observation.map(|(first, second)| InterruptSamples::new(first, second).unwrap()),
                0,
            );
            let saved = actual.interrupts;
            let context = actual.begin_debug_call(&mut bus, 0x400).unwrap();
            for _ in 0..2 {
                actual.interrupt_input().set_level(live);
                assert!(matches!(
                    actual.step_boundary(&mut bus).unwrap(),
                    TimedOutcome::Instruction(_)
                ));
            }
            assert!(actual.debug_call_returned(&context));
            actual.finish_debug_call(context).unwrap();
            assert_eq!(actual.interrupts, saved);
            actual.interrupt_input().set_level(live);
            let expected = match observation {
                Some((4, 4)) => Some(4),
                Some((4, 6)) => None,
                None => Some(live),
                _ => unreachable!(),
            };
            for level in [4, 6] {
                bus.handler(24 + level);
            }
            let outcome = actual.step_boundary(&mut bus).unwrap();
            match expected {
                Some(level) => assert!(
                    matches!(outcome, TimedOutcome::Interrupt { level: taken, .. } if taken == level)
                ),
                None => {
                    assert!(matches!(outcome, TimedOutcome::Instruction(_)));
                    assert!(
                        matches!(actual.step_boundary(&mut bus).unwrap(), TimedOutcome::Interrupt { level: taken, .. } if taken == live)
                    );
                }
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn callee_status_writes_keep_normal_interrupt_and_trace_behavior() {
    for changed_sr in [SUPERVISOR | 0x15, SUPERVISOR | TRACE | 0x15] {
        for live in [0, 4] {
            let (mut actual, mut bus) = fixture(
                SUPERVISOR | TRACE | 0x0300,
                &[0x46fc, changed_sr, 0x4e75, 0x4e71],
            );
            let context = actual.begin_debug_call(&mut bus, 0x400).unwrap();
            assert!(matches!(
                actual.step_boundary(&mut bus).unwrap(),
                TimedOutcome::Instruction(_)
            ));
            assert_eq!(actual.registers.status_register(), changed_sr);
            actual.interrupt_input().set_level(live);
            if live != 0 {
                bus.handler(28);
                assert!(matches!(
                    actual.step_boundary(&mut bus).unwrap(),
                    TimedOutcome::Interrupt { level: 4, .. }
                ));
                assert!(!actual.debug_call_returned(&context));
            } else if changed_sr & TRACE != 0 {
                bus.handler(9);
                assert!(
                    matches!(actual.step_boundary(&mut bus).unwrap(), TimedOutcome::Instruction(step) if step.exception == Some(9))
                );
                assert!(!actual.debug_call_returned(&context));
            } else {
                assert!(matches!(
                    actual.step_boundary(&mut bus).unwrap(),
                    TimedOutcome::Instruction(_)
                ));
                actual.finish_debug_call(context).unwrap();
                assert_eq!(
                    actual.registers.status_register(),
                    SUPERVISOR | TRACE | 0x0315
                );
            }
        }
    }
}
