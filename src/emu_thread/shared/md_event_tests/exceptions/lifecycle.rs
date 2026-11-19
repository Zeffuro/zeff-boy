use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_exception_definitions_survive_timelines_while_hits_and_trace_follow_commit() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = trap(b'U', timing);
        let raw = backend.encode_state_bytes().unwrap();
        let checkpoint = backend.encode_rewind_checkpoint().unwrap();
        let mut fault = WorkerRuntimeFault::default();
        for restore in 0..4 {
            let mut actions = exception(true);
            actions.trace_enabled = Some(true);
            let result = traced(&mut backend, 1, actions, false, false, &mut fault);
            assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Exception));
            assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Exception]);
            let frozen = machine(&backend).clone();
            let rows = result.ui_data.instruction_trace.unwrap().entries;
            assert!(backend.load_state_from_bytes(vec![0; 32]).is_err());
            assert!(backend.restore_rewind_checkpoint(vec![0; 32]).is_err());
            let result = traced(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert_eq!(machine(&backend), &frozen);
            assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Exception));
            assert_eq!(
                result.ui_data.instruction_trace.as_ref().unwrap().entries,
                rows
            );
            if restore == 3 && timing == TimingModel::Precise {
                let EmuBackend::Md(md) = &mut backend else {
                    unreachable!()
                };
                assert!(md.try_warm_reset().is_err());
                let result = traced(
                    &mut backend,
                    0,
                    DebugUiActions::none(),
                    false,
                    false,
                    &mut fault,
                );
                assert_eq!(machine(&backend), &frozen);
                assert_eq!(cpu(&result).hit_event, Some(DebugEvent::Exception));
                assert_eq!(
                    result.ui_data.instruction_trace.as_ref().unwrap().entries,
                    rows
                );
                break;
            }
            match restore {
                0 => {
                    backend.load_state_from_bytes(raw.clone()).unwrap();
                }
                1 => backend
                    .restore_rewind_checkpoint(checkpoint.clone())
                    .unwrap(),
                2 => backend.reset(),
                _ => {
                    let EmuBackend::Md(md) = &mut backend else {
                        unreachable!()
                    };
                    md.try_warm_reset().unwrap();
                }
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
            assert_eq!(cpu(&result).event_breakpoints, [DebugEvent::Exception]);
            assert!(cpu(&result).recent_opcodes.is_empty());
            assert!(
                result
                    .ui_data
                    .instruction_trace
                    .as_ref()
                    .unwrap()
                    .entries
                    .is_empty()
            );
        }
    }
}
