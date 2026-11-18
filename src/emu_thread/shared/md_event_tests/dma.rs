use super::*;
use zeff_emu_common::debug::TraceBoundaryKind;
use zeff_emu_common::time::MachineTiming;

pub(in crate::emu_thread::shared) fn dma_actions(enabled: bool, trace: bool) -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    actions.event_breakpoint_changes = vec![(DebugEvent::Dma, enabled)];
    actions.trace_enabled = Some(trace);
    actions
}

fn program(tail: &[u16]) -> Vec<u16> {
    let mut program = Vec::new();
    for value in [0x80, 0x10, 0x90] {
        program.extend([0x13fc, value, 0x00c0, 0x0011]);
    }
    for word in [
        0x8114, 0x8f02, 0x9308, 0x9400, 0x9500, 0x9600, 0x9700, 0x4400, 0x0080,
    ] {
        program.extend([0x33fc, word, 0x00c0, 0x0004]);
    }
    program.extend_from_slice(tail);
    program
}

pub(in crate::emu_thread::shared) fn before_arm(
    header: u8,
    timing: TimingModel,
    tail: &[u16],
) -> EmuBackend {
    let mut backend = fixture_with_timing(header, &program(tail), timing);
    for _ in 0..11 {
        machine_mut(&mut backend).step_instruction().unwrap();
    }
    assert_eq!(machine(&backend).vdp_registers()[19], Some(8));
    backend
}

fn before_fill_tail(timing: TimingModel, tail: &[u16]) -> EmuBackend {
    let mut words = Vec::new();
    for word in [
        0x8114, 0x8f01, 0x9380, 0x9400, 0x9500, 0x9600, 0x9780, 0x4400, 0x0080,
    ] {
        words.extend([0x33fc, word, 0x00c0, 4]);
    }
    words.extend([0x33fc, 0, 0x00c0, 0]);
    words.extend_from_slice(tail);
    let mut backend = fixture_with_timing(b'U', &words, timing);
    for _ in 0..10 {
        machine_mut(&mut backend).step_instruction().unwrap();
    }
    backend
}

fn traced(
    backend: &mut EmuBackend,
    frames: usize,
    actions: DebugUiActions,
    step: bool,
    resume: bool,
    fault: &mut WorkerRuntimeFault,
) -> FrameResult {
    exceptions::traced(backend, frames, actions, step, resume, fault)
}

fn advance_reference(expected: &mut zeff_md_core::Emulator, ticks: u64) -> TimedOutcome {
    let mut last = None;
    for _ in 0..512 {
        let now = expected.timing_snapshot().now().get();
        if now == ticks {
            return last.expect("stop did not execute a boundary");
        }
        assert!(
            now < ticks,
            "worker stopped inside an ordinary machine boundary"
        );
        last = Some(expected.step_instruction().unwrap().cpu);
    }
    panic!("unbounded worker stop reference");
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_dma_event_worker_stops_at_real_boundary_and_preserves_continuation() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            for trace in [false, true] {
                let mut backend = before_arm(header, timing, &[0x4e71, 0x60fe, 0x4e71]);
                let mut expected = machine(&backend).clone();
                let mut fault = WorkerRuntimeFault::default();
                let result = traced(
                    &mut backend,
                    2,
                    dma_actions(true, trace),
                    false,
                    false,
                    &mut fault,
                );
                assert!(backend.is_suspended());
                assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Dma));
                let last = advance_reference(
                    &mut expected,
                    machine(&backend).timing_snapshot().now().get(),
                );
                assert!(matches!(last, TimedOutcome::Instruction(_)));
                compare(&backend, &mut expected, &result, 0);
                assert_eq!(
                    machine(&backend).encode_frontend_state().unwrap(),
                    expected.encode_frontend_state().unwrap()
                );
                assert!(machine(&backend).vdp_registers()[19].unwrap() < 8);
                assert!(
                    result
                        .ui_data
                        .core_features
                        .as_ref()
                        .unwrap()
                        .supports_debugger
                );
                if trace {
                    let rows = &result.ui_data.instruction_trace.as_ref().unwrap().entries;
                    assert_eq!(rows.last().unwrap().event, Some(DebugEvent::Dma));
                    assert_eq!(
                        rows.last().unwrap().boundary.unwrap().kind,
                        TraceBoundaryKind::Instruction
                    );
                    assert!(!rows.last().unwrap().instruction_bytes().is_empty());
                }
                let frozen = machine(&backend).encode_frontend_state().unwrap();
                let paused = traced(
                    &mut backend,
                    1,
                    DebugUiActions::none(),
                    false,
                    false,
                    &mut fault,
                );
                assert!(paused.runtime_fault.is_none());
                assert_eq!(machine(&backend).encode_frontend_state().unwrap(), frozen);
                assert_eq!(cpu(&paused).hit_event, Some(DebugEvent::Dma));
                expected.step_field().unwrap();
                let result = traced(
                    &mut backend,
                    1,
                    dma_actions(false, trace),
                    false,
                    true,
                    &mut fault,
                );
                compare(&backend, &mut expected, &result, 1);
                assert_eq!(cpu(&result).hit_event, None);
                assert_eq!(
                    &machine(&backend).vdp().memory().vram()[0x400..0x408],
                    &[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]
                );
                assert!(!result.audio_samples.is_empty());
            }
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_dma_event_timeline_replacements_clear_hits_without_fake_activity() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for restore in 0..3 {
            let mut backend = before_arm(b'U', timing, &[0x4e71, 0x60fe, 0x4e71]);
            let state = backend.encode_state_bytes().unwrap();
            let rewind = backend.encode_rewind_checkpoint().unwrap();
            let mut fault = WorkerRuntimeFault::default();
            let result = traced(
                &mut backend,
                1,
                dma_actions(true, true),
                false,
                false,
                &mut fault,
            );
            assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Dma));
            let frozen = machine(&backend).encode_frontend_state().unwrap();
            assert!(backend.load_state_from_bytes(vec![0; 32]).is_err());
            assert!(backend.restore_rewind_checkpoint(vec![0; 32]).is_err());
            assert_eq!(machine(&backend).encode_frontend_state().unwrap(), frozen);
            let result = traced(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Dma));
            match restore {
                0 => {
                    backend.load_state_from_bytes(state).unwrap();
                }
                1 => backend.restore_rewind_checkpoint(rewind).unwrap(),
                _ => backend.reset(),
            }
            let result = traced(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert_eq!(cpu(&result).hit_event, None);
            assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Dma]);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_dma_late_unmapped_fetch_keeps_real_prefix_but_has_no_success_hit() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut found = None;
        for lead in 0..32 {
            let mut tail = vec![0x4e71; lead];
            tail.extend([0x4ef9, 0x00b0, 0, 0x4e71]);
            let mut backend = before_fill_tail(timing, &tail);
            for _ in 0..lead {
                machine_mut(&mut backend).step_instruction().unwrap();
            }
            let mut expected = machine(&backend).clone();
            let epoch = expected.debug_dma_activity_epoch();
            if expected.step_instruction().is_err() && expected.debug_dma_activity_epoch() != epoch
            {
                found = Some((backend, expected));
                break;
            }
        }
        let (mut backend, mut expected) = found.expect("no late fault with real DMA prefix");
        let mut fault = WorkerRuntimeFault::default();
        let result = traced(
            &mut backend,
            1,
            dma_actions(true, true),
            false,
            false,
            &mut fault,
        );
        assert!(result.runtime_fault.is_some());
        let pcm = expected.drain_audio_samples();
        assert!(
            result
                .audio_samples
                .iter()
                .map(|sample| sample.to_bits())
                .eq(pcm.iter().map(|sample| sample.to_bits()))
        );
        assert_eq!(machine(&backend), &expected);
        assert_eq!(cpu(&result).hit_event, None);
        let row = result
            .ui_data
            .instruction_trace
            .as_ref()
            .unwrap()
            .entries
            .last()
            .unwrap();
        assert!(row.boundary.unwrap().machine_fault);
        assert_eq!(row.event, Some(DebugEvent::Dma));
        assert!(cpu(&result).recent_opcodes.is_empty());
        let frozen = machine(&backend).clone();
        let result = traced(
            &mut backend,
            1,
            dma_actions(true, true),
            true,
            true,
            &mut fault,
        );
        assert!(result.runtime_fault.is_none());
        assert!(!fault.can_step());
        assert_eq!(machine(&backend), &frozen);
        assert_eq!(cpu(&result).hit_event, None);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_dma_stop_poll_trace_has_no_borrowed_opcode_or_history() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = before_fill_tail(timing, &[0x4e72, 0x2700, 0x60fe, 0x4e71]);
        machine_mut(&mut backend).step_instruction().unwrap();
        assert!(machine(&backend).main_cpu_stopped());
        let mut expected = machine(&backend).clone();
        let result = traced(
            &mut backend,
            1,
            dma_actions(true, true),
            false,
            false,
            &mut WorkerRuntimeFault::default(),
        );
        assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Dma));
        assert_eq!(
            advance_reference(
                &mut expected,
                machine(&backend).timing_snapshot().now().get()
            ),
            TimedOutcome::Stopped
        );
        compare(&backend, &mut expected, &result, 0);
        assert!(cpu(&result).recent_opcodes.is_empty());
        let rows = &result.ui_data.instruction_trace.as_ref().unwrap().entries;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].event, Some(DebugEvent::Dma));
        assert_eq!(rows[0].boundary.unwrap().kind, TraceBoundaryKind::Stopped);
        assert!(rows[0].instruction_bytes().is_empty());
        assert_eq!(rows[0].boundary.unwrap().cpu_clocks, 0);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_dma_cpu_event_and_watch_coexist_with_explicit_event_and_trace_priority() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for exception in [false, true] {
            for cpu_event_enabled in [false, true] {
                let event = if exception {
                    DebugEvent::Exception
                } else {
                    DebugEvent::Interrupt
                };
                let mut backend = before_fill_tail(
                    timing,
                    if exception {
                        &[0x4e40, 0x60fe, 0x4e71]
                    } else {
                        &[0x46fc, 0x2000, 0x60fe, 0x4e71]
                    },
                );
                if exception {
                    machine_mut(&mut backend)
                        .install_rom_patches(&super::super::md_rom_cheat_tests::codes(
                            "000080:0000+000082:0300",
                        ))
                        .unwrap();
                } else {
                    machine_mut(&mut backend).step_instruction().unwrap();
                    assert!(machine_mut(&mut backend).set_interrupt_level(6));
                }
                let mut expected = machine(&backend).clone();
                let epoch = expected.debug_dma_activity_epoch();
                let outcome = expected.step_instruction().unwrap().cpu;
                assert!(if exception {
                    matches!(outcome, TimedOutcome::Instruction(step) if step.exception == Some(32))
                } else {
                    matches!(outcome, TimedOutcome::Interrupt { .. })
                });
                assert_ne!(expected.debug_dma_activity_epoch(), epoch);
                let mut actions = dma_actions(true, true);
                actions
                    .event_breakpoint_changes
                    .push((event, cpu_event_enabled));
                actions.add_watchpoint =
                    Some((0xfffffa, 0xfffffb, zeff_emu_common::debug::WatchType::Write));
                let result = traced(
                    &mut backend,
                    1,
                    actions,
                    false,
                    false,
                    &mut WorkerRuntimeFault::default(),
                );
                compare(&backend, &mut expected, &result, 0);
                assert_eq!(
                    cpu(&result).hit_event,
                    Some(if cpu_event_enabled {
                        event
                    } else {
                        DebugEvent::Dma
                    })
                );
                assert!(cpu(&result).hit_watchpoint.is_some());
                let rows = &result.ui_data.instruction_trace.as_ref().unwrap().entries;
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].event, Some(event));
                assert_eq!(
                    rows[0].boundary.unwrap().kind,
                    if exception {
                        TraceBoundaryKind::Instruction
                    } else {
                        TraceBoundaryKind::Interrupt
                    }
                );
                if exception {
                    assert_eq!(rows[0].instruction_bytes(), &[0x4e, 0x40]);
                } else {
                    assert!(rows[0].instruction_bytes().is_empty());
                }
            }
        }
    }
}
