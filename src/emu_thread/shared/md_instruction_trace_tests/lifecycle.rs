use super::*;
use zeff_emu_common::time::Reset;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_instruction_trace_ring_cursor_controls_and_successful_timeline_changes_are_ephemeral() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = fixture_with_timing(b'U', &[0x60fe, 0x4e71], timing);
        let raw = backend.encode_state_bytes().unwrap();
        let checkpoint = backend.encode_rewind_checkpoint().unwrap();
        let result = run(&mut backend, enabled(), false, 1, None);
        let batch = result.ui_data.instruction_trace.as_ref().unwrap();
        assert_eq!((batch.retained, batch.capacity), (1000, 1000));
        assert!(batch.oldest_sequence.unwrap() > 0);
        assert!(
            batch
                .entries
                .windows(2)
                .all(|pair| pair[1].sequence == pair[0].sequence + 1)
        );
        let cursor = batch.newest_sequence;
        let result = run(&mut backend, DebugUiActions::none(), false, 0, cursor);
        assert!(entries(&result).is_empty());
        backend.debug_suspend();
        let mut disabled = DebugUiActions::none();
        disabled.trace_enabled = Some(false);
        let result = run(&mut backend, disabled, true, 0, cursor);
        let batch = result.ui_data.instruction_trace.as_ref().unwrap();
        assert!(!batch.enabled && batch.entries.is_empty());
        assert_eq!(batch.newest_sequence, cursor);
        let mut actions = enabled();
        actions.trace_clear = true;
        actions.trace_capacity = Some(2500);
        let result = run(&mut backend, actions, true, 0, cursor);
        let batch = result.ui_data.instruction_trace.as_ref().unwrap();
        assert_eq!((batch.retained, batch.capacity), (1, 2500));
        assert_eq!(batch.entries[0].sequence, cursor.unwrap() + 1);
        for restore in 0..5 {
            let before = machine(&backend).clone();
            let before_rows = run(&mut backend, DebugUiActions::none(), false, 0, None);
            assert!(backend.load_state_from_bytes(vec![0; 32]).is_err());
            assert!(backend.restore_rewind_checkpoint(vec![0; 32]).is_err());
            let after_rows = run(&mut backend, DebugUiActions::none(), false, 0, None);
            assert_eq!(machine(&backend), &before);
            assert_eq!(entries(&before_rows), entries(&after_rows));
            if restore == 4 && timing == TimingModel::Precise {
                let EmuBackend::Md(md) = &mut backend else {
                    unreachable!()
                };
                assert!(md.try_warm_reset().is_err());
                let result = run(&mut backend, DebugUiActions::none(), false, 0, None);
                assert_eq!(entries(&after_rows), entries(&result));
                assert_eq!(machine(&backend), &before);
                break;
            }
            match restore {
                0 => {
                    backend.load_state_from_bytes(raw.clone()).unwrap();
                }
                1 => backend
                    .restore_rewind_checkpoint(checkpoint.clone())
                    .unwrap(),
                2 => {
                    let mut history = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
                    history.push(&machine(&backend).encode_frontend_state().unwrap(), &[]);
                    backend.take_rewind_audio_change();
                    assert!(matches!(
                        EmuThread::handle_rewind(
                            &mut backend,
                            &mut history,
                            &Default::default(),
                            1
                        ),
                        EmuResponse::RewindOk { .. }
                    ));
                }
                3 => backend.reset(),
                _ => {
                    let EmuBackend::Md(md) = &mut backend else {
                        unreachable!()
                    };
                    md.try_warm_reset().unwrap();
                }
            }
            let result = run(&mut backend, DebugUiActions::none(), false, 0, None);
            let batch = result.ui_data.instruction_trace.as_ref().unwrap();
            assert!(batch.enabled && batch.entries.is_empty());
            assert_eq!((batch.retained, batch.capacity), (0, 2500));
            backend.debug_suspend();
            let mut expected = machine(&backend).clone();
            expected.step_instruction().unwrap();
            let result = run(&mut backend, DebugUiActions::none(), true, 0, None);
            compare(&backend, &mut expected, &result, 0);
            assert_eq!(entries(&result).len(), 1);
            assert_eq!(
                machine(&backend).encode_state().unwrap(),
                expected.encode_state().unwrap()
            );
            assert_eq!(
                machine(&backend).encode_frontend_state().unwrap(),
                expected.encode_frontend_state().unwrap()
            );
        }
    }
}
