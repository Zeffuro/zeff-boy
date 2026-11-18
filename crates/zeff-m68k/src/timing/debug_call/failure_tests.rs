use super::tests::fixture;
use super::*;
use crate::SUPERVISOR;
use crate::bus::{AccessKind, BusAccess, ByteLanes, Direction, FunctionCode};
use crate::timing::physical::{
    CriticalFault, EntryKind, Group0Capture, ImportedGroup0Boundary, ProcessingClass, SswProfile,
    TimingQualification,
};
use crate::timing::test_bus::{Bus, Event};
use crate::timing::{CpuClocks, Pipeline, TimedOutcome};

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn admission_rejections_leave_cpu_bus_and_clocks_untouched() {
    for bank in [0, SUPERVISOR] {
        for case in 0..16 {
            let (mut actual, mut bus) = fixture(bank | TRACE | 0x0300, &[0x4e75]);
            let mut target = 0x400;
            let access = BusAccess {
                logical_address: 0x100,
                bus_address: 0x100,
                function_code: FunctionCode::SupervisorProgram,
                kind: AccessKind::Prefetch,
                lanes: ByteLanes::Both,
            };
            let cause = CriticalFault::BusAbort {
                access,
                direction: Direction::Read,
            };
            let expected = match case {
                0 => {
                    actual.frozen = true;
                    DebugCallRejection::Frozen
                }
                1 => {
                    actual.stopped = true;
                    DebugCallRejection::Stopped
                }
                2 => {
                    actual.pending_trace = true;
                    DebugCallRejection::PendingTrace
                }
                3 => {
                    actual.pipeline = None;
                    DebugCallRejection::Unprimed
                }
                4 => {
                    actual.pipeline.as_mut().unwrap().address += 2;
                    DebugCallRejection::InvalidQueue
                }
                5 => {
                    actual.registers.pc |= 1;
                    DebugCallRejection::InvalidQueue
                }
                6 => {
                    target |= 1;
                    DebugCallRejection::OddTarget
                }
                7 => {
                    actual
                        .registers
                        .set_address_register(7, actual.registers.stack_pointer() | 1);
                    DebugCallRejection::OddStack
                }
                8 => {
                    actual.interrupts.set_level(7);
                    DebugCallRejection::Level7Interrupt
                }
                9 => {
                    actual.interrupts.set_level(7);
                    actual.interrupts.set_level(0);
                    DebugCallRejection::Level7Interrupt
                }
                10 => {
                    actual.physical_state =
                        PhysicalEntryState::Critical(EntryKind::ExternalWarmReset);
                    DebugCallRejection::NonOrdinaryBoundary
                }
                11 => {
                    actual.physical_inspection = PhysicalInspection::Unavailable;
                    DebugCallRejection::NonOrdinaryBoundary
                }
                12 => {
                    actual.imported_boundary = Some(ImportedGroup0Boundary {
                        registers: actual.registers,
                        clock: actual.clock(),
                        capture: Group0Capture {
                            vector: 2,
                            internal_pc: actual.registers.pc,
                            phase_sr: actual.registers.status_register(),
                            ird: 0x4e71,
                            logical_address: 0x100,
                            function_code: FunctionCode::SupervisorProgram,
                            direction: Direction::Read,
                            processing_class: ProcessingClass::InstructionOrGroup2,
                            ssw_profile: SswProfile::SelectedIrdUpperBits,
                        },
                    });
                    DebugCallRejection::NonOrdinaryBoundary
                }
                13 => {
                    actual.physical_state = PhysicalEntryState::DoubleFaultHalted {
                        cause,
                        timing: TimingQualification::Qualified,
                    };
                    DebugCallRejection::NonOrdinaryBoundary
                }
                14 => {
                    actual.physical_state = PhysicalEntryState::FaultUnqualified { cause };
                    DebugCallRejection::NonOrdinaryBoundary
                }
                15 => {
                    assert!(actual.interrupts.restore_observation(true, 3));
                    DebugCallRejection::NonOrdinaryBoundary
                }
                _ => unreachable!(),
            };
            let before = actual.clone();
            let before_bus = bus.clone();
            assert_eq!(actual.check_debug_call(target), Err(expected));
            assert!(matches!(actual.begin_debug_call(&mut bus, target),
                Err(TimingFault::DebugCallRejected(reason)) if reason == expected));
            assert_eq!(actual, before);
            assert_eq!(bus, before_bus);
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn reset_style_register_initialization_and_real_priming_admit_without_an_instruction() {
    let mut actual = PrefetchCpu::default();
    actual.registers.pc = 0x100;
    actual.registers.ssp = 0xff0000;
    let mut bus = Bus::default();
    actual.prime(&mut bus).unwrap();
    assert_eq!(actual.clock(), CpuClocks(8));
    assert_eq!(actual.check_debug_call(0x400), Ok(()));
    let before = actual.clone();
    let before_bus = bus.clone();
    assert_eq!(actual.check_debug_call(0x400), Ok(()));
    assert_eq!(actual, before);
    assert_eq!(bus, before_bus);
    let _context = actual.begin_debug_call(&mut bus, 0x400).unwrap();
    assert_eq!(actual.clock(), CpuClocks(24));
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn every_entry_transfer_failure_keeps_real_jsr_partial_state_and_bus_effects() {
    for bank in [0, SUPERVISOR] {
        for wait in [0, 3] {
            for fail in 0..4 {
                let (mut actual, mut bus) = fixture(bank | 0x0315 | TRACE, &[0x7201, 0x7402]);
                let initial = actual.clone();
                let original_pc = actual.registers.pc;
                let original_sp = actual.registers.stack_pointer();
                bus.wait = wait;
                bus.fail = Some(fail);
                let mut normal = initial.clone();
                normal.registers.pc = original_pc - 2;
                normal.registers.set_status_register(bank | 0x0715);
                normal.pipeline = Some(Pipeline::unknown(normal.registers.pc, [0x4e90, 0x4e71]));
                normal.registers.a[0] = 0x400;
                actual.registers.a[0] = 0x400;
                let mut normal_bus = bus.clone();
                let expected = normal.step_instruction(&mut normal_bus).unwrap_err();
                assert!(
                    matches!(actual.begin_debug_call(&mut bus, 0x400), Err(ref error) if *error == expected)
                );
                assert_eq!(bus, normal_bus);
                assert_eq!(actual.clock(), normal.clock());
                assert_eq!(actual.clock().0, (fail as u64 + 1) * (4 + u64::from(wait)));
                assert!(actual.frozen);
                assert_eq!(actual.registers.pc, original_pc);
                assert_eq!(actual.pipeline, initial.pipeline);
                assert_eq!(normal.pipeline.unwrap().words, [0x4e90, 0x4e71]);
                assert_eq!(
                    actual.registers.stack_pointer(),
                    if fail == 0 {
                        original_sp
                    } else {
                        original_sp.wrapping_sub(4)
                    }
                );
                assert_eq!(
                    actual.registers.stack_pointer(),
                    normal.registers.stack_pointer()
                );
                assert_eq!(actual.registers.d, normal.registers.d);
                assert_eq!(actual.registers.a, normal.registers.a);
                assert_eq!(actual.registers.status_register(), bank | 0x0715);
                assert_eq!(actual.queued_words(), initial.queued_words());
                assert!(!actual.pending_trace);
                let writes: Vec<_> = bus
                    .events
                    .iter()
                    .filter_map(|event| {
                        if let Event::Write(access, value, _) = event {
                            Some((access.bus_address, *value))
                        } else {
                            None
                        }
                    })
                    .collect();
                if fail >= 2 {
                    let address = original_sp.wrapping_sub(4) & super::super::BUS_ADDRESS_MASK;
                    assert_eq!(bus.words[&address], (original_pc >> 16) as u16);
                }
                assert_eq!(writes.len(), fail.min(2));
                assert!(matches!(
                    actual.step_boundary(&mut bus),
                    Err(TimingFault::Frozen)
                ));
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn finish_rejects_wrong_pc_stack_bank_and_unready_return_without_mutation() {
    for case in 0..7 {
        let (mut actual, mut bus) = fixture(SUPERVISOR | TRACE | 0x0315, &[0x4e75]);
        let context = actual.begin_debug_call(&mut bus, 0x400).unwrap();
        assert!(matches!(
            actual.step_boundary(&mut bus).unwrap(),
            TimedOutcome::Instruction(_)
        ));
        assert!(actual.debug_call_returned(&context));
        let expected = match case {
            0 => {
                actual.registers.pc += 2;
                actual.pipeline.as_mut().unwrap().address = actual.registers.pc;
                DebugCallRejection::ReturnBoundaryMismatch
            }
            1 => {
                actual.registers.ssp += 2;
                DebugCallRejection::ReturnBoundaryMismatch
            }
            2 => {
                actual.registers.usp = actual.registers.ssp;
                actual.registers.set_status_register(0x0700);
                DebugCallRejection::ReturnBoundaryMismatch
            }
            3 => {
                actual.pending_trace = true;
                DebugCallRejection::PendingTrace
            }
            4 => {
                actual.stopped = true;
                DebugCallRejection::Stopped
            }
            5 => {
                actual.pipeline = None;
                DebugCallRejection::Unprimed
            }
            6 => {
                actual.interrupts.set_level(7);
                DebugCallRejection::Level7Interrupt
            }
            _ => unreachable!(),
        };
        let before = actual.clone();
        let before_bus = bus.clone();
        assert!(!actual.debug_call_returned(&context));
        assert_eq!(actual, before);
        assert_eq!(actual.finish_debug_call(context), Err(expected));
        assert_eq!(actual, before);
        assert_eq!(bus, before_bus);
    }
}
