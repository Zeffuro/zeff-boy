use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_watchpoint_hits_reset_only_after_accepted_state_rewind_and_resets() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend =
            fixture_with_timing(b'U', &[0x13fc, 0x55, 0xff, 0x41, 0x60fe, 0x4e71], timing);
        let initial = machine(&backend).encode_state().unwrap();
        let rewind = backend.encode_rewind_checkpoint().unwrap();
        let mut fault = WorkerRuntimeFault::default();
        let result = deliver(
            &mut backend,
            1,
            watch(0xff0041, 0xff0041, WatchType::Write),
            false,
            false,
            &mut fault,
        );
        check_hit(&result, 0xff0041, 0x55, WatchType::Write, Some(0));
        let before = machine(&backend).clone();
        assert!(backend.load_state_from_bytes(vec![0; 64]).is_err());
        assert_eq!(machine(&backend), &before);
        let result = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        check_hit(&result, 0xff0041, 0x55, WatchType::Write, Some(0));
        backend.load_state_from_bytes(initial.clone()).unwrap();
        let result = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
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
        assert_eq!(
            result.ui_data.cpu_debug.as_ref().unwrap().watchpoints.len(),
            1
        );
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
        assert!(
            result
                .ui_data
                .cpu_debug
                .as_ref()
                .unwrap()
                .hit_watchpoint
                .is_none()
        );
        assert_eq!(
            result.ui_data.cpu_debug.as_ref().unwrap().watchpoints.len(),
            1
        );
        deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
        );
        let EmuBackend::Md(md) = &mut backend else {
            unreachable!()
        };
        let reset = md.try_warm_reset();
        if timing == TimingModel::Precise {
            assert!(reset.is_err());
            let result = deliver(
                &mut backend,
                0,
                DebugUiActions::none(),
                false,
                false,
                &mut fault,
            );
            check_hit(&result, 0xff0041, 0x55, WatchType::Write, Some(0));
            backend.reset();
        } else {
            reset.unwrap();
        }
        let result = deliver(
            &mut backend,
            0,
            DebugUiActions::none(),
            false,
            false,
            &mut fault,
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
        assert_eq!(
            result.ui_data.cpu_debug.as_ref().unwrap().watchpoints.len(),
            1
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_watchpoints_exclude_fetches_stop_polls_and_include_irq_stack_service() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut backend = fixture_with_timing(b'U', &[0x4e72, 0x2000, 0x60fe, 0x4e71], timing);
        let mut expected = machine(&backend).clone();
        expected.step_field().unwrap();
        let mut fault = WorkerRuntimeFault::default();
        let result = deliver(
            &mut backend,
            1,
            watch(0, 0xffff, WatchType::ReadWrite),
            false,
            false,
            &mut fault,
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
        machine_mut(&mut backend).set_interrupt_level(6);
        expected.set_interrupt_level(6);
        expected.clear_frame_ready();
        expected.step_instruction().unwrap();
        let result = deliver(
            &mut backend,
            1,
            watch(0xfffff8, 0xfffffb, WatchType::Write),
            false,
            false,
            &mut fault,
        );
        compare(&backend, &mut expected, &result, 0);
        assert_eq!(machine(&backend).registers().pc, 0x300);
        assert!((0xfffff8..=0xfffffb).contains(&hit(&result).address));
        assert_eq!(hit(&result).watch_type, WatchType::Write);
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_watchpoint_success_before_late_fault_is_retained_and_failed_access_is_not_a_hit() {
    for (address, expected_hit) in [(0x7ffffe, Some((0x7ffffe, Some(0x12)))), (0x800000, None)] {
        let mut backend = fixture(
            b'U',
            &[
                0x23fc,
                0x1234,
                0x5678,
                (address >> 16) as u16,
                address as u16,
                0x4e71,
                0x4e71,
            ],
        );
        let mut expected = machine(&backend).clone();
        assert!(expected.step_instruction().is_err());
        let mut fault = WorkerRuntimeFault::default();
        let result = deliver(
            &mut backend,
            1,
            watch(0, 0xffffff, WatchType::ReadWrite),
            false,
            false,
            &mut fault,
        );
        assert!(result.runtime_fault.is_some());
        assert_eq!(machine(&backend), &expected);
        let cpu = result.ui_data.cpu_debug.as_ref().unwrap();
        assert_eq!(
            cpu.hit_watchpoint
                .as_ref()
                .map(|hit| (hit.address, hit.new_value)),
            expected_hit
        );
        assert!(cpu.recent_opcodes.is_empty());
        let before = machine(&backend).clone();
        let result = deliver(
            &mut backend,
            1,
            DebugUiActions::none(),
            true,
            true,
            &mut fault,
        );
        assert!(result.runtime_fault.is_none());
        assert!(!fault.can_step());
        assert_eq!(machine(&backend), &before);
        assert_eq!(
            result
                .ui_data
                .cpu_debug
                .as_ref()
                .unwrap()
                .hit_watchpoint
                .as_ref()
                .map(|hit| (hit.address, hit.new_value)),
            expected_hit
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_watchpoint_clr_read_has_unknown_value_after_the_actual_write_completes() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for kind in [WatchType::Read, WatchType::Write, WatchType::ReadWrite] {
            let mut backend = fixture_with_timing(
                b'U',
                &[
                    0x13fc, 0x55, 0xff, 0x41, 0x4239, 0xff, 0x41, 0x4e72, 0x2700, 0x4e71,
                ],
                timing,
            );
            machine_mut(&mut backend).step_instruction().unwrap();
            let mut expected = machine(&backend).clone();
            expected.step_instruction().unwrap();
            let result = deliver(
                &mut backend,
                1,
                watch(0xff0041, 0xff0041, kind),
                false,
                false,
                &mut WorkerRuntimeFault::default(),
            );
            compare(&backend, &mut expected, &result, 0);
            assert!(backend.is_suspended());
            assert_eq!(machine(&backend).registers().pc, 0x20e);
            assert_eq!(machine(&backend).peek(0xff0041), Some(0));
            assert_eq!(
                result.ui_data.cpu_debug.as_ref().unwrap().recent_opcodes[0].bytes,
                [0x42, 0x39]
            );
            if kind == WatchType::Write {
                check_hit(&result, 0xff0041, 0, WatchType::Write, Some(0x55));
            } else {
                assert_eq!(hit(&result).address, 0xff0041);
                assert_eq!(hit(&result).watch_type, WatchType::Read);
                assert_eq!(hit(&result).old_value, None);
                assert_eq!(hit(&result).new_value, None);
                assert_eq!(hit(&result).value_text(), "value unavailable");
            }
        }

        let mut backend = fixture_with_timing(
            b'U',
            &[
                0x13fc, 0x55, 0xff, 0x41, 0x4239, 0xff, 0x41, 0x4e72, 0x2700, 0x4e71,
            ],
            timing,
        );
        machine_mut(&mut backend).step_instruction().unwrap();
        let mut expected = machine(&backend).clone();
        expected.step_field().unwrap();
        let result = deliver(
            &mut backend,
            1,
            watch(0xff0040, 0xff0040, WatchType::ReadWrite),
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
        assert_eq!(machine(&backend).peek(0xff0041), Some(0));
    }
}
