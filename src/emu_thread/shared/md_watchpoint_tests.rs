use super::md_pc_breakpoint_tests::{
    compare, deliver, fixture, fixture_with_timing, machine, machine_mut,
};
use super::*;
use crate::debug::{DebugUiActions, WatchHitDisplay};
use zeff_emu_common::debug::WatchType;
use zeff_emu_common::time::Reset;
use zeff_md_core::TimingModel;

fn watch(start: u32, end: u32, kind: WatchType) -> DebugUiActions {
    let mut actions = DebugUiActions::none();
    actions.add_watchpoint = Some((start, end, kind));
    actions
}

fn hit(result: &FrameResult) -> &WatchHitDisplay {
    result
        .ui_data
        .cpu_debug
        .as_ref()
        .unwrap()
        .hit_watchpoint
        .as_ref()
        .unwrap()
}

fn check_hit(result: &FrameResult, address: u32, value: u8, kind: WatchType, old: Option<u8>) {
    let hit = hit(result);
    assert_eq!(
        (hit.address, hit.new_value, hit.watch_type),
        (address, Some(value), kind)
    );
    assert_eq!(hit.old_value, old);
    assert_eq!(
        hit.value_text(),
        match old {
            Some(old) => format!("{old:02X} to {value:02X}"),
            None => format!("value {value:02X}"),
        }
    );
    let cpu = result.ui_data.cpu_debug.as_ref().unwrap();
    assert_eq!(cpu.cpu_state, "Suspended");
    assert!(cpu.hit_breakpoint.is_none());
    let features = result.ui_data.core_features.as_ref().unwrap();
    assert!(features.supports_memory_watchpoints);
    assert!(features.supports_debugger && features.supports_guest_calls);
    assert!(result.ui_data.instruction_trace.is_none());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_watchpoints_stop_after_word_boundary_and_continue_on_unchanged_write_and_read() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let mut backend = fixture_with_timing(
                header,
                &[
                    0x33fc, 0xa55a, 0xff, 0x40, 0x33fc, 0xa55a, 0xff, 0x40, 0x3039, 0xff, 0x40,
                    0x60fe, 0x4e71,
                ],
                timing,
            );
            let mut expected = machine(&backend).clone();
            let mut fault = WorkerRuntimeFault::default();
            expected.step_instruction().unwrap();
            let result = deliver(
                &mut backend,
                1,
                watch(0xff0041, 0xff0041, WatchType::ReadWrite),
                false,
                false,
                &mut fault,
            );
            compare(&backend, &mut expected, &result, 0);
            assert!(backend.is_suspended());
            check_hit(&result, 0xff0041, 0x5a, WatchType::Write, Some(0));
            assert_eq!(machine(&backend).registers().pc, 0x208);
            assert_eq!(machine(&backend).peek(0xff0040), Some(0xa5));
            assert_eq!(machine(&backend).peek(0xff0041), Some(0x5a));
            assert_eq!(
                result.ui_data.cpu_debug.as_ref().unwrap().recent_opcodes[0].bytes,
                [0x33, 0xfc]
            );
            for kind in [WatchType::Write, WatchType::Read] {
                expected.step_instruction().unwrap();
                let result = deliver(
                    &mut backend,
                    1,
                    DebugUiActions::none(),
                    false,
                    true,
                    &mut fault,
                );
                compare(&backend, &mut expected, &result, 0);
                check_hit(
                    &result,
                    0xff0041,
                    0x5a,
                    kind,
                    (kind == WatchType::Write).then_some(0x5a),
                );
            }
            assert_eq!(machine(&backend).registers().d[0] & 0xffff, 0xa55a);
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_watchpoints_choose_first_physical_byte_for_long_predecrement_and_overlaps() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for (start, end, address, value) in [
            (0xff0040, 0xff0047, 0xff0040, 0x12),
            (0xff0041, 0xff0047, 0xff0041, 0x34),
            (0xff0047, 0xff0043, 0xff0043, 0x78),
        ] {
            let mut backend = fixture_with_timing(
                b'U',
                &[0x23fc, 0x1234, 0x5678, 0xff, 0x40, 0x60fe, 0x4e71],
                timing,
            );
            let mut expected = machine(&backend).clone();
            let mut fault = WorkerRuntimeFault::default();
            deliver(
                &mut backend,
                0,
                watch(0xff0040, 0xff0047, WatchType::Read),
                false,
                false,
                &mut fault,
            );
            expected.step_instruction().unwrap();
            let result = deliver(
                &mut backend,
                1,
                watch(start, end, WatchType::Write),
                false,
                false,
                &mut fault,
            );
            compare(&backend, &mut expected, &result, 0);
            check_hit(&result, address, value, WatchType::Write, Some(0));
            assert_eq!(machine(&backend).peek(0xff0043), Some(0x78));
        }

        let mut backend = fixture_with_timing(
            b'U',
            &[
                0x203c, 0x1234, 0x5678, 0x207c, 0xff, 0x44, 0x2100, 0x60fe, 0x4e71,
            ],
            timing,
        );
        machine_mut(&mut backend).step_instruction().unwrap();
        machine_mut(&mut backend).step_instruction().unwrap();
        let mut expected = machine(&backend).clone();
        expected.step_instruction().unwrap();
        let result = deliver(
            &mut backend,
            1,
            watch(0xff0040, 0xff0043, WatchType::Write),
            false,
            false,
            &mut WorkerRuntimeFault::default(),
        );
        compare(&backend, &mut expected, &result, 0);
        check_hit(&result, 0xff0042, 0x56, WatchType::Write, Some(0));
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_watchpoints_reject_invalid_ranges_preserve_odd_bytes_and_reject_running_edits() {
    let mut backend = fixture(b'U', &[0x60fe, 0x4e71]);
    let mut fault = WorkerRuntimeFault::default();
    let mut expected = machine(&backend).clone();
    let mut invalid = watch(0xff0041, 0x1000000, WatchType::ReadWrite);
    invalid.memory_writes.push((0xff0041, 0x55));
    let result = deliver(&mut backend, 0, invalid, false, false, &mut fault);
    compare(&backend, &mut expected, &result, 0);
    assert!(result.ui_data.cpu_debug.unwrap().watchpoints.is_empty());
    for _ in 0..2 {
        deliver(
            &mut backend,
            0,
            watch(0xff0043, 0xff0041, WatchType::Write),
            false,
            false,
            &mut fault,
        );
    }
    let result = deliver(
        &mut backend,
        0,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    let ranges = &result.ui_data.cpu_debug.as_ref().unwrap().watchpoints;
    assert_eq!(ranges.len(), 1);
    assert_eq!(
        (ranges[0].address, ranges[0].end_address),
        (0xff0041, 0xff0043)
    );
    let mut remove = DebugUiActions::none();
    remove
        .remove_watchpoints
        .push((0xff0043, 0xff0041, WatchType::Write));
    let result = deliver(&mut backend, 0, remove, false, false, &mut fault);
    assert!(result.ui_data.cpu_debug.unwrap().watchpoints.is_empty());
    assert_eq!(machine(&backend).peek(0xff0041), Some(0));
    expected.step_field().unwrap();
    let result = deliver(
        &mut backend,
        1,
        DebugUiActions::none(),
        false,
        false,
        &mut fault,
    );
    compare(&backend, &mut expected, &result, 1);
}

mod session;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_watchpoints_step_ram_tas_and_preserve_pc_resume_policy() {
    let mut backend = fixture(b'U', &[0x207c, 0xff, 0x41, 0x4ad0, 0x60fc, 0x4e71]);
    machine_mut(&mut backend).step_instruction().unwrap();
    let mut expected = machine(&backend).clone();
    let mut fault = WorkerRuntimeFault::default();
    let mut actions = watch(0xff0041, 0xff0041, WatchType::Write);
    actions.add_breakpoint = Some(0x206);
    let result = deliver(&mut backend, 1, actions, false, false, &mut fault);
    compare(&backend, &mut expected, &result, 0);
    assert_eq!(
        result.ui_data.cpu_debug.as_ref().unwrap().hit_breakpoint,
        Some(0x206)
    );
    assert!(
        result
            .ui_data
            .cpu_debug
            .as_ref()
            .unwrap()
            .hit_watchpoint
            .is_none()
    );
    expected.step_instruction().unwrap();
    let result = deliver(
        &mut backend,
        1,
        DebugUiActions::none(),
        false,
        true,
        &mut fault,
    );
    compare(&backend, &mut expected, &result, 0);
    check_hit(&result, 0xff0041, 0x80, WatchType::Write, None);
    assert_eq!(machine(&backend).peek(0xff0041), Some(0));
    expected.step_instruction().unwrap();
    let result = deliver(
        &mut backend,
        1,
        DebugUiActions::none(),
        false,
        true,
        &mut fault,
    );
    compare(&backend, &mut expected, &result, 0);
    assert_eq!(
        result.ui_data.cpu_debug.as_ref().unwrap().hit_breakpoint,
        Some(0x206)
    );
    expected.step_instruction().unwrap();
    let result = deliver(
        &mut backend,
        0,
        DebugUiActions::none(),
        true,
        false,
        &mut fault,
    );
    compare(&backend, &mut expected, &result, 0);
    check_hit(&result, 0xff0041, 0x80, WatchType::Write, None);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_watchpoint_miss_field_preserves_state_video_and_nonzero_pcm() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = fixture_with_timing(
            b'U',
            &[
                0x13fc, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x5239,
                0xff, 0x40, 0x60f8,
            ],
            timing,
        );
        let mut expected = machine(&backend).clone();
        expected.step_field().unwrap();
        let result = deliver(
            &mut backend,
            1,
            watch(0xff0080, 0xff0080, WatchType::ReadWrite),
            false,
            false,
            &mut WorkerRuntimeFault::default(),
        );
        compare(&backend, &mut expected, &result, 1);
        assert!(!backend.is_suspended());
        assert!(
            result
                .ui_data
                .cpu_debug
                .as_ref()
                .unwrap()
                .hit_watchpoint
                .is_none()
        );
        assert!(result.audio_samples.iter().any(|sample| *sample != 0.0));
        assert_eq!(machine(&backend).framebuffer(), expected.framebuffer());
    }
}
