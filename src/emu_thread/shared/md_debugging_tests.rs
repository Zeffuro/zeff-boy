use super::md_pc_breakpoint_tests::{
    compare, deliver, fixture, fixture_with_timing, machine, machine_mut,
};
use super::*;
use crate::debug::DebugUiActions;
use zeff_emu_common::time::Reset;
use zeff_md_core::TimingModel;

fn history(result: &FrameResult) -> Vec<(u32, Vec<u8>)> {
    result
        .ui_data
        .cpu_debug
        .as_ref()
        .unwrap()
        .recent_opcodes
        .iter()
        .map(|entry| (entry.address, entry.bytes.clone()))
        .collect()
}

fn hits(result: &FrameResult) -> (u64, u64) {
    let conditions = &result
        .ui_data
        .cpu_debug
        .as_ref()
        .unwrap()
        .breakpoint_hit_conditions;
    assert_eq!(conditions.len(), 1);
    (conditions[0].hits, conditions[0].target_hits)
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_debug_history_and_hit_counts_reach_worker_and_reset_only_on_new_timeline() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend =
                fixture_with_timing(header, &[0x5239, 0xff, 0x40, 0x60f8, 0x4e71], timing);
            let initial = machine(&backend).encode_state().unwrap();
            let rewind = backend.encode_rewind_checkpoint().unwrap();
            let mut expected = machine(&backend).clone();
            let mut fault = WorkerRuntimeFault::default();
            let mut actions = DebugUiActions::none();
            actions.add_breakpoint_after = Some((0x200, 3));
            for _ in 0..4 {
                expected.step_instruction().unwrap();
            }
            let result = deliver(&mut backend, 1, actions, false, false, &mut fault);
            compare(&backend, &mut expected, &result, 0);
            assert!(backend.is_suspended());
            assert_eq!(machine(&backend).peek(0xff0040), Some(2));
            assert_eq!(hits(&result), (3, 3));
            assert_eq!(
                history(&result),
                vec![
                    (0x206, vec![0x60, 0xf8]),
                    (0x200, vec![0x52, 0x39]),
                    (0x206, vec![0x60, 0xf8]),
                    (0x200, vec![0x52, 0x39])
                ]
            );
            let features = result.ui_data.core_features.as_ref().unwrap();
            assert!(features.supports_opcode_history && features.supports_pc_breakpoints);
            assert!(features.supports_debugger && features.supports_guest_calls);
            assert!(result.ui_data.instruction_trace.is_none());
            let before = history(&result);
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert_eq!(history(&result), before);
            assert_eq!(hits(&result), (3, 3));
            for _ in 0..2 {
                expected.step_instruction().unwrap();
            }
            let result = deliver(
                &mut backend,
                1,
                DebugUiActions::none(),
                false,
                true,
                &mut fault,
            );
            compare(&backend, &mut expected, &result, 0);
            assert_eq!(machine(&backend).peek(0xff0040), Some(3));
            assert_eq!(hits(&result), (4, 3));
            assert_eq!(history(&result).len(), 6);
            let before = history(&result);
            assert!(backend.load_state_from_bytes(vec![0; 64]).is_err());
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert_eq!(history(&result), before);
            assert_eq!(hits(&result), (4, 3));
            backend.load_state_from_bytes(initial).unwrap();
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert!(history(&result).is_empty());
            assert_eq!(hits(&result), (0, 3));
            deliver(
                &mut backend,
                1,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            backend.restore_rewind_checkpoint(rewind).unwrap();
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert!(history(&result).is_empty());
            assert_eq!(hits(&result), (0, 3));
            deliver(
                &mut backend,
                1,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            if timing == TimingModel::Standard {
                let EmuBackend::Md(md) = &mut backend else {
                    unreachable!()
                };
                md.try_warm_reset().unwrap();
            } else {
                let before = machine(&backend).clone();
                let EmuBackend::Md(md) = &mut backend else {
                    unreachable!()
                };
                assert!(md.try_warm_reset().is_err());
                assert_eq!(machine(&backend), &before);
                let result = deliver(
                    &mut backend,
                    0,
                    DebugUiActions::none(),
                    false,
                    false,
                    &mut fault,
                );
                assert_eq!(history(&result).len(), 4);
                assert_eq!(hits(&result), (3, 3));
                backend.reset();
            }
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            assert!(history(&result).is_empty());
            assert_eq!(hits(&result), (0, 3));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_debug_history_opt_in_ring_preserves_field_state_video_and_nonzero_pcm() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let words = [
            0x13fc, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x5239,
            0xff, 0x40, 0x60f8,
        ];
        let mut backend = fixture_with_timing(b'U', &words, timing);
        let mut expected = machine(&backend).clone();
        let mut fault = WorkerRuntimeFault::default();
        expected.step_field().unwrap();
        let result = deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        assert!(result.audio_samples.iter().any(|sample| *sample != 0.0));
        compare(&backend, &mut expected, &result, 1);
        assert_eq!(machine(&backend).framebuffer(), expected.framebuffer());
        let before = history(&result);
        assert_eq!(before.len(), 32);
        assert!(
            before
                .iter()
                .all(|(pc, bytes)| (*pc == 0x218 && bytes == &[0x52, 0x39])
                    || (*pc == 0x21e && bytes == &[0x60, 0xf8]))
        );
        let mut input = md_audio_tests::input(1, false);
        input.snapshot.want_debug_info = false;
        expected.step_field().unwrap();
        let result = EmuThread::handle_step_frames(
            &mut backend,
            input,
            &[],
            false,
            &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
            &mut 0,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 1);
        assert!(result.ui_data.cpu_debug.is_none());
        let result = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        assert_eq!(history(&result), before);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_debug_history_uses_retired_prefetch_words_and_keeps_faulting_opcodes_out() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = fixture_with_timing(b'U', &[0x4ef9, 0xff, 0x100, 0x4e71], timing);
        let patches = zeff_md_core::cheats::parse_raw_cheats("FF0100:7001+FF0102:60FC").unwrap();
        assert_eq!(
            machine_mut(&mut backend).apply_ram_cheats(&patches).applied,
            4
        );
        backend.debug_suspend();
        let mut fault = WorkerRuntimeFault::default();
        let result = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            true,
            false,
            &mut fault,
        );
        assert!(result.runtime_fault.is_none());
        assert_eq!(history(&result), vec![(0x200, vec![0x4e, 0xf9])]);
        let patches = zeff_md_core::cheats::parse_raw_cheats("FF0100:7002").unwrap();
        machine_mut(&mut backend).apply_ram_cheats(&patches);
        assert_eq!(machine(&backend).queued_words().unwrap()[0], 0x7001);
        assert_eq!(machine(&backend).peek(0xff0101), Some(2));
        let result = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            true,
            false,
            &mut fault,
        );
        assert_eq!(history(&result)[0], (0xff0100, vec![0x70, 1]));
        assert_eq!(machine(&backend).registers().d[0], 1);
        deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            true,
            false,
            &mut fault,
        );
        let result = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            true,
            false,
            &mut fault,
        );
        assert_eq!(history(&result)[0], (0xff0100, vec![0x70, 2]));
        assert_eq!(machine(&backend).registers().d[0], 2);
    }
    let mut backend = fixture(b'U', &[0x7001, 0x3039, 0x40, 0]);
    let mut fault = WorkerRuntimeFault::default();
    let result = deliver(
        &mut backend,
        1,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    assert!(result.runtime_fault.is_some());
    assert_eq!(history(&result), vec![(0x200, vec![0x70, 1])]);
    let before = machine(&backend).clone();
    let result = deliver(
        &mut backend,
        1,
        DebugUiActions::none(),
        true,
        true,
        &mut fault,
    );
    assert_eq!(machine(&backend), &before);
    assert_eq!(history(&result), vec![(0x200, vec![0x70, 1])]);
}
