use super::*;
use zeff_emu_common::debug::TraceBoundaryKind;

fn prepared(program: &[u16], vector: u8, timing: TimingModel, header: u8) -> EmuBackend {
    let mut backend = fixture_with_timing(header, program, timing);
    let address = u32::from(vector) * 4;
    machine_mut(&mut backend)
        .install_rom_patches(&super::super::super::md_rom_cheat_tests::codes(&format!(
            "{address:06X}:0000+{:06X}:0300",
            address + 2
        )))
        .unwrap();
    backend
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_exception_events_service_traps_illegal_divide_chk_privilege_and_trace_vectors() {
    let mut cases: Vec<(Vec<u16>, usize, u8)> = (0..16)
        .map(|n| (vec![0x4e40 | n, 0x60fe, 0x4e71], 0, 32 + n as u8))
        .collect();
    cases.extend([
        (vec![0x4afc, 0x60fe, 0x4e71], 0, 4),
        (vec![0xa123, 0x60fe, 0x4e71], 0, 10),
        (vec![0xf123, 0x60fe, 0x4e71], 0, 11),
        (vec![0x80fc, 0, 0x60fe, 0x4e71], 0, 5),
        (vec![0x41bc, 0xffff, 0x60fe, 0x4e71], 0, 6),
        (vec![0x003c, 2, 0x4e76, 0x60fe, 0x4e71], 1, 7),
        (vec![0x46fc, 0, 0x4e70, 0x60fe, 0x4e71], 1, 8),
        (vec![0x46fc, 0xa000, 0x4e71, 0x60fe], 1, 9),
    ]);
    for (program, setup, vector) in cases {
        for header in *b"UJE" {
            for timing in [TimingModel::Standard, TimingModel::Precise] {
                let mut backend = prepared(&program, vector, timing, header);
                for _ in 0..setup {
                    machine_mut(&mut backend).step_instruction().unwrap();
                }
                let mut expected = machine(&backend).clone();
                assert!(
                    matches!(expected.step_instruction().unwrap().cpu,
                    TimedOutcome::Instruction(step) if step.exception == Some(vector)),
                    "{program:04x?}"
                );
                let mut fault = WorkerRuntimeFault::default();
                backend.debug_suspend();
                let result = traced(&mut backend, 0, exception(true), true, false, &mut fault);
                compare(&backend, &mut expected, &result, 0);
                assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Exception));
                assert_eq!(cpu(&result).recent_opcodes.len(), 1);
                assert!(
                    cpu(&result).recent_opcodes[0]
                        .detail
                        .as_ref()
                        .unwrap()
                        .contains(&format!("exception {vector}"))
                );
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_exception_pending_trace_has_no_opcode_and_trace_after_trap_reports_last_vector() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for pending in [false, true] {
            let mut backend = if pending {
                fixture_with_timing(b'U', &[0x46fc, 0xa000, 0x4e71, 0x60fe], timing)
            } else {
                prepared(&[0x46fc, 0xa000, 0x4e40, 0x60fe], 32, timing, b'U')
            };
            machine_mut(&mut backend).step_instruction().unwrap();
            if pending {
                super::super::super::md_pc_breakpoint_tests::trace_state::restore_pending_trace(
                    machine_mut(&mut backend),
                );
            }
            backend.debug_suspend();
            let mut expected = machine(&backend).clone();
            let outcome = expected.step_instruction().unwrap().cpu;
            assert!(if pending {
                matches!(outcome, TimedOutcome::Trace { .. })
            } else {
                matches!(outcome, TimedOutcome::Instruction(step) if step.exception == Some(9))
            });
            let mut actions = exception(true);
            actions.trace_enabled = Some(true);
            let result = traced(
                &mut backend,
                0,
                actions,
                true,
                false,
                &mut WorkerRuntimeFault::default(),
            );
            compare(&backend, &mut expected, &result, 0);
            assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Exception));
            let trace = &result.ui_data.instruction_trace.as_ref().unwrap().entries;
            assert_eq!(trace.len(), 1);
            assert_eq!(trace[0].event, Some(DebugEvent::Exception));
            let boundary = trace[0].boundary.unwrap();
            assert_eq!(boundary.final_exception, Some(9));
            assert!(!boundary.machine_fault);
            assert_eq!(
                boundary.kind,
                if pending {
                    TraceBoundaryKind::Trace
                } else {
                    TraceBoundaryKind::Instruction
                }
            );
            assert_eq!(
                trace[0].instruction_bytes(),
                if pending { &[][..] } else { &[0x4e, 0x40][..] }
            );
            assert_eq!(cpu(&result).recent_opcodes.len(), usize::from(!pending));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_exception_event_ignores_irq_stop_and_ordinary_boundaries() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = fixture_with_timing(b'U', &[0x4e72, 0x2000, 0x60fe, 0x4e71], timing);
        let mut expected = machine(&backend).clone();
        expected.step_field().unwrap();
        let mut fault = WorkerRuntimeFault::default();
        let result = traced(&mut backend, 1, exception(true), false, false, &mut fault);
        compare(&backend, &mut expected, &result, 1);
        assert!(expected.main_cpu_stopped());
        assert!(!backend.is_suspended());
        assert_eq!(cpu(&result).hit_event, None);
        machine_mut(&mut backend).set_interrupt_level(6);
        expected.set_interrupt_level(6);
        assert!(matches!(
            expected.step_instruction().unwrap().cpu,
            TimedOutcome::Interrupt { .. }
        ));
        backend.debug_suspend();
        let result = traced(
            &mut backend,
            0,
            DebugUiActions::none(),
            true,
            false,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        assert_eq!(cpu(&result).hit_event, None);
        expected.step_instruction().unwrap();
        let result = traced(
            &mut backend,
            0,
            DebugUiActions::none(),
            true,
            false,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        assert_eq!(cpu(&result).hit_event, None);
        assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Exception]);
    }
}
