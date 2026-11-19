use super::*;

const WRITES: &[(u32, u8)] = &[
    (0xff0041, 0x55),
    (0xa10009, 0xc0),
    (0xa10003, 0x80),
    (0xa1000b, 0x81),
    (0xa10005, 0x80),
    (0xa1000d, 0xff),
    (0xa10007, 0xd5),
    (0xff0041, 0xa5),
];

fn direct_request(backend: &mut EmuBackend, request: FrameInput) -> FrameResult {
    EmuThread::handle_step_frames(
        backend,
        request,
        &[],
        false,
        &mut zeff_emu_common::rewind::RewindBuffer::new(60, 10),
        &mut 10,
        &mut WorkerRuntimeFault::default(),
    )
}

fn stop_request(trace: bool) -> FrameInput {
    let mut request = input(1, true);
    request.debug_actions.add_breakpoint = Some(LOOP);
    request.debug_actions.trace_enabled = Some(trace);
    request.debug_actions.add_watchpoint = Some((0xa10003, 0xa1000d, WatchType::Write));
    request
}

fn pins(frame: &FrameResult) {
    let cpu = frame.ui_data.cpu_debug.as_ref().unwrap();
    let rows: Vec<_> = cpu
        .io_registers
        .iter()
        .map(|row| (row.address, row.value, row.writable_mask))
        .collect();
    assert_eq!(
        rows,
        [
            (0xa10001, 0xa0, 0),
            (0xa10003, 0xa2, 0),
            (0xa10005, 0xfc, 0),
            (0xa10007, 0xd5, 0),
            (0xa10009, 0xc0, 0x7f),
            (0xa1000b, 0x81, 0x7f),
            (0xa1000d, 0xff, 0x7f),
        ]
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_input_edit_actual_worker_matches_independent_core_control_then_two_fields() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for trace in [false, true] {
            let mut subject = EmuThread::spawn(
                fixture(b'U', timing, ControllerKind::SixButton, true),
                false,
            );
            let mut expected = fixture(b'U', timing, ControllerKind::SixButton, true);
            let warm = guest_call_deliver(&subject, input(2, true));
            let control_warm = direct_request(&mut expected, input(2, true));
            assert_eq!((warm.advanced_frames, control_warm.advanced_frames), (2, 2));
            assert!(warm.runtime_fault.is_none() && control_warm.runtime_fault.is_none());
            assert!(warm.audio_samples.iter().any(|sample| *sample != 0.0));
            assert_eq!(pcm(&warm), pcm(&control_warm));
            assert_eq!(capture(&subject), expected.encode_state_bytes().unwrap());
            let stopped = guest_call_deliver(&subject, stop_request(trace));
            let expected_stop = direct_request(&mut expected, stop_request(trace));
            paused(&stopped);
            paused(&expected_stop);
            assert_eq!(capture(&subject), expected.encode_state_bytes().unwrap());
            let before = stopped.ui_data.cpu_debug.as_ref().unwrap();
            let mut request = input(0, true);
            request.debug_actions.memory_writes = WRITES.to_vec();
            let edited = guest_call_deliver(&subject, request);
            assert_eq!(edited.advanced_frames, 0);
            assert!(edited.audio_samples.is_empty() && edited.runtime_fault.is_none());
            let after = edited.ui_data.cpu_debug.as_ref().unwrap();
            assert_eq!(
                (after.pc, after.cycles, after.hit_breakpoint),
                (before.pc, before.cycles, before.hit_breakpoint)
            );
            assert_eq!(after.register_lines, before.register_lines);
            assert_eq!(
                format!("{:?}", after.recent_opcodes),
                format!("{:?}", before.recent_opcodes)
            );
            assert!(after.hit_watchpoint.is_none());
            let after_trace = edited.ui_data.instruction_trace.as_ref().unwrap();
            let before_trace = stopped.ui_data.instruction_trace.as_ref().unwrap();
            assert_eq!(after_trace.entries, before_trace.entries);
            assert_eq!(
                (
                    after_trace.enabled,
                    after_trace.retained,
                    after_trace.capacity
                ),
                (
                    before_trace.enabled,
                    before_trace.retained,
                    before_trace.capacity
                )
            );
            pins(&edited);
            machine_mut(&mut expected)
                .debug_write_memory(WRITES)
                .unwrap();
            assert_eq!(capture(&subject), expected.encode_state_bytes().unwrap());
            let mut control = EmuThread::spawn(expected, false);
            assert_eq!(capture(&subject), capture(&control));
            for round in 0..2 {
                let resume = || {
                    let mut request = input(1, true);
                    if round == 0 {
                        request.debug_actions.remove_breakpoints.push(LOOP);
                        request.debug_actions.remove_watchpoints.push((
                            0xa10003,
                            0xa1000d,
                            WatchType::Write,
                        ));
                        request.debug_continue = true;
                    }
                    request
                };
                let actual = guest_call_deliver(&subject, resume());
                let expected = guest_call_deliver(&control, resume());
                assert_eq!((actual.advanced_frames, expected.advanced_frames), (1, 1));
                assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
                assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(pcm(&actual), pcm(&expected));
                assert_eq!(capture(&subject), capture(&control));
                let left = subject.shared_framebuffer().load_full().unwrap();
                let right = control.shared_framebuffer().load_full().unwrap();
                assert!(!left.is_empty());
                assert_eq!(left.dimensions(), right.dimensions());
                assert_eq!(left.as_slice(), right.as_slice());
            }
            subject.shutdown();
            control.shutdown();
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_input_editor_commit_precedes_host_buttons_and_does_not_hide_host_uart_faults() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for guarded in [false, true] {
            let words = [
                0x13fc, 0x40, 0x00a1, 9, 0x13fc, 0x20, 0x00a1, 0x13, 0x60fe, 0x4e71,
            ];
            let mut subject = EmuThread::spawn(
                guest_call_backend_with(
                    b'U',
                    timing,
                    ControllerKind::SixButton,
                    &[(0x200, &words)],
                    false,
                ),
                false,
            );
            let warm = guest_call_deliver(&subject, guest_call_input(1));
            assert_eq!(warm.advanced_frames, 1);
            assert!(warm.runtime_fault.is_none());
            let mut stop = guest_call_input(1);
            stop.debug_actions.add_breakpoint = Some(0x210);
            paused(&guest_call_deliver(&subject, stop));
            let mut request = guest_call_input(1);
            request.debug_actions.memory_writes = vec![(0xa10003, if guarded { 0x7f } else { 0 })];
            request.joypad.buttons = 2;
            request.debug_actions.remove_breakpoints.push(0x210);
            request.debug_continue = true;
            let result = guest_call_deliver(&subject, request);
            if guarded {
                assert!(result.runtime_fault.is_some());
            } else {
                assert_eq!(result.advanced_frames, 1);
                assert!(result.runtime_fault.is_none());
                assert_eq!(
                    result.ui_data.cpu_debug.as_ref().unwrap().io_registers[1].value,
                    0x33
                );
                assert!(!capture(&subject).is_empty());
            }
            subject.shutdown();
        }
    }
}
