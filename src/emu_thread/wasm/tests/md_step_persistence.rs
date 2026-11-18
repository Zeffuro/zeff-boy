use super::*;
use crate::emu_thread::shared::md_step_persistence_tests::{backend, input, sram};
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
fn md_memory_edit_wasm_worker_accounts_only_accepted_batches_and_reports_rejection() {
    use crate::emu_thread::shared::md_step_persistence_tests::{assert_edit_result, edit_input};
    let thread = EmuThread::spawn(backend(), false);
    assert_edit_result(&deliver(&thread, edit_input(&[])), true);
    {
        let mut inner = thread.inner.borrow_mut();
        inner.rewind_buffer = RewindBuffer::new(1, 1);
        inner.rewind_buffer.push(&[1, 2, 3], &[]);
    }
    let before = thread.inner.borrow().backend.encode_state_bytes().unwrap();
    for writes in [vec![], vec![(0xff0041, 0x55), (0xa00000, 0x12)]] {
        let rejected = !writes.is_empty();
        let result = deliver(&thread, edit_input(&writes));
        assert_edit_result(&result, true);
        assert!(result.runtime_fault.is_none());
        let mut inner = thread.inner.borrow_mut();
        assert_eq!(inner.backend.encode_state_bytes().unwrap(), before);
        assert!(!inner.battery_potentially_dirty);
        assert!(inner.pending_audio_discontinuities.is_empty());
        assert_eq!(inner.rewind_buffer.peek().unwrap().state_bytes, [1, 2, 3]);
        if rejected {
            assert!(matches!(
                inner.pending_responses.pop_front(),
                Some(EmuResponse::MemoryWriteRejected(_))
            ));
        }
        assert!(inner.pending_responses.is_empty());
    }
    let result = deliver(&thread, edit_input(&[(0xff0041, 0xa5)]));
    assert_edit_result(&result, true);
    assert_eq!(
        thread.inner.borrow().pending_audio_discontinuities,
        [crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation]
    );
    assert!(thread.inner.borrow().battery_potentially_dirty);
    let mut request = edit_input(&[(0xff0042, 0x5a)]);
    request.debug_continue = true;
    let result = deliver(&thread, request);
    assert_edit_result(&result, false);
    assert_eq!(
        thread.inner.borrow().pending_audio_discontinuities,
        [crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation; 2]
    );
    thread.inner.borrow_mut().battery_potentially_dirty = false;
    let before = thread.inner.borrow().backend.encode_state_bytes().unwrap();
    let result = deliver(&thread, edit_input(&[(0xff0041, 0)]));
    assert_edit_result(&result, false);
    {
        let mut inner = thread.inner.borrow_mut();
        assert!(matches!(
            inner.pending_responses.pop_front(),
            Some(EmuResponse::MemoryWriteRejected(_))
        ));
        assert_eq!(inner.backend.encode_state_bytes().unwrap(), before);
        assert!(!inner.battery_potentially_dirty);
        assert_eq!(inner.pending_audio_discontinuities.len(), 2);
        inner.backend.debug_suspend();
        inner.runtime_fault.latch(Some("worker-only fault".into()));
    }
    for _ in 0..2 {
        let result = deliver(&thread, edit_input(&[(0xff0041, 0)]));
        assert_edit_result(&result, false);
        let mut inner = thread.inner.borrow_mut();
        assert!(matches!(
            inner.pending_responses.pop_front(),
            Some(EmuResponse::MemoryWriteRejected(_))
        ));
        assert_eq!(inner.backend.encode_state_bytes().unwrap(), before);
        assert!(!inner.battery_potentially_dirty);
        assert_eq!(inner.pending_audio_discontinuities.len(), 2);
    }
    let thread = EmuThread::spawn(backend(), false);
    let mut request = edit_input(&[(0xff0041, 0xa5)]);
    request.debug_continue = true;
    request.frames = 1;
    let result = deliver(&thread, request);
    assert!(result.runtime_fault.is_some());
    assert_edit_result(&result, false);
    {
        let mut inner = thread.inner.borrow_mut();
        assert!(inner.battery_potentially_dirty);
        let EmuBackend::Md(md) = &inner.backend else {
            unreachable!()
        };
        assert_eq!(md.core().work_ram()[0x41], 0xa5);
        assert!(inner.pending_responses.is_empty());
        inner.battery_potentially_dirty = false;
    }
    let result = deliver(&thread, edit_input(&[(0xff0041, 0)]));
    assert_edit_result(&result, false);
    let mut inner = thread.inner.borrow_mut();
    assert!(matches!(
        inner.pending_responses.pop_front(),
        Some(EmuResponse::MemoryWriteRejected(_))
    ));
    assert!(!inner.battery_potentially_dirty);
    assert!(inner.backend.encode_state_bytes().is_err());
}

fn deliver(thread: &EmuThread, input: FrameInput) -> FrameResult {
    let writes_requested = !input.debug_actions.memory_writes.is_empty();
    thread.send(EmuCommand::StepFrames(Box::new(input)));
    let result = thread.try_recv_frame().unwrap();
    assert_eq!(result.advanced_frames, 0);
    assert_eq!(result.completed_step_requests, 1);
    if !writes_requested {
        assert!(
            thread
                .inner
                .borrow()
                .pending_audio_discontinuities
                .is_empty()
        );
    }
    result
}

#[wasm_bindgen_test]
fn md_step_persistence_wasm_worker_rearms_dirty_for_sram_and_faulting_steps() {
    let thread = EmuThread::spawn(backend(), false);
    let initial = sram(&thread.inner.borrow().backend);
    assert!(!thread.inner.borrow().battery_potentially_dirty);
    assert!(
        deliver(&thread, input(false, false))
            .runtime_fault
            .is_none()
    );
    assert!(!thread.inner.borrow().battery_potentially_dirty);
    assert!(deliver(&thread, input(true, true)).runtime_fault.is_none());
    assert!(!thread.inner.borrow().backend.is_suspended());
    assert_eq!(sram(&thread.inner.borrow().backend), initial);
    assert!(!thread.inner.borrow().battery_potentially_dirty);
    thread.inner.borrow_mut().backend.debug_suspend();

    let result = deliver(&thread, input(true, false));
    assert!(result.runtime_fault.is_none());
    assert!(thread.inner.borrow().backend.is_suspended());
    let mut expected = initial;
    expected[0] = 0xa5;
    assert_eq!(sram(&thread.inner.borrow().backend), expected);
    assert!(thread.inner.borrow().battery_potentially_dirty);

    thread.inner.borrow_mut().battery_potentially_dirty = false;
    let fault = deliver(&thread, input(true, false));
    assert!(fault.runtime_fault.is_some());
    let inner = thread.inner.borrow();
    assert!(!inner.runtime_fault.can_step());
    assert!(inner.backend.is_suspended());
    assert_eq!(sram(&inner.backend), expected);
    assert!(inner.battery_potentially_dirty);
}

#[wasm_bindgen_test]
fn md_pc_breakpoint_wasm_worker_marks_zero_field_sram_and_fault_progress_dirty() {
    let thread = EmuThread::spawn(backend(), false);
    let initial = sram(&thread.inner.borrow().backend);
    let mut request = input(false, true);
    request.frames = 5;
    request.debug_actions.add_breakpoint = Some(0x200);
    let result = deliver(&thread, request);
    assert!(result.runtime_fault.is_none());
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
    assert!(!thread.inner.borrow().battery_potentially_dirty);
    assert_eq!(sram(&thread.inner.borrow().backend), initial);

    let mut request = input(false, true);
    request.frames = 5;
    request.debug_actions.remove_breakpoints.push(0x200);
    request.debug_actions.add_one_shot_breakpoint = Some(0x208);
    let result = deliver(&thread, request);
    assert!(result.runtime_fault.is_none());
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x208)
    );
    assert_eq!(sram(&thread.inner.borrow().backend)[0], 0xa5);
    assert!(thread.inner.borrow().battery_potentially_dirty);

    thread.inner.borrow_mut().battery_potentially_dirty = false;
    let mut request = input(false, true);
    request.frames = 1;
    let result = deliver(&thread, request);
    assert!(result.runtime_fault.is_some());
    let inner = thread.inner.borrow();
    assert!(!inner.runtime_fault.can_step() && inner.backend.is_suspended());
    assert_eq!(sram(&inner.backend)[0], 0xa5);
    assert!(inner.battery_potentially_dirty);
}

#[wasm_bindgen_test]
fn md_pc_breakpoint_wasm_worker_marks_write_then_fault_in_one_request_dirty() {
    let thread = EmuThread::spawn(backend(), false);
    let initial = sram(&thread.inner.borrow().backend);
    assert_ne!(initial[0], 0xa5);
    assert!(!thread.inner.borrow().battery_potentially_dirty);
    let mut request = input(false, true);
    request.frames = 1;
    request.debug_actions.add_breakpoint = Some(0x300);
    let result = deliver(&thread, request);
    assert!(result.runtime_fault.is_some());
    let inner = thread.inner.borrow();
    assert!(!inner.runtime_fault.can_step() && inner.backend.is_suspended());
    let mut expected = initial;
    expected[0] = 0xa5;
    assert_eq!(sram(&inner.backend), expected);
    assert!(inner.battery_potentially_dirty);
}

#[wasm_bindgen_test]
fn md_input_edit_wasm_worker_retains_audio_dirty_rewind_and_fault_policy() {
    use crate::emu_thread::shared::md_step_persistence_tests::{assert_edit_result, edit_input};
    let thread = EmuThread::spawn(backend(), false);
    assert_edit_result(&deliver(&thread, edit_input(&[])), true);
    {
        let mut inner = thread.inner.borrow_mut();
        inner.rewind_buffer = RewindBuffer::new(1, 1);
        inner.rewind_buffer.push(&[1, 2, 3], &[]);
    }
    let original = thread.inner.borrow().backend.encode_state_bytes().unwrap();
    let bad = [
        (0xff0041, 0x55),
        (0xa10009, 0x40),
        (0xa10003, 0),
        (0xa10002, 0),
    ];
    assert_edit_result(&deliver(&thread, edit_input(&bad)), true);
    {
        let mut inner = thread.inner.borrow_mut();
        assert!(matches!(
            inner.pending_responses.pop_front(),
            Some(EmuResponse::MemoryWriteRejected(_))
        ));
        assert_eq!(inner.backend.encode_state_bytes().unwrap(), original);
        assert!(inner.pending_audio_discontinuities.is_empty());
        assert!(!inner.battery_potentially_dirty);
    }
    let good = [(0xff0041, 0xa5), (0xa10009, 0x40), (0xa10003, 0)];
    for count in 1..=2 {
        assert_edit_result(&deliver(&thread, edit_input(&good)), true);
        let inner = thread.inner.borrow();
        assert_eq!(
            inner.pending_audio_discontinuities,
            vec![crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation; count]
        );
        assert!(inner.battery_potentially_dirty);
        assert_eq!(inner.rewind_buffer.peek().unwrap().state_bytes, [1, 2, 3]);
        assert!(inner.pending_responses.is_empty());
    }
    let before = thread.inner.borrow().backend.encode_state_bytes().unwrap();
    {
        let mut inner = thread.inner.borrow_mut();
        inner.battery_potentially_dirty = false;
        inner.runtime_fault.latch(Some("worker-only fault".into()));
    }
    assert_edit_result(&deliver(&thread, edit_input(&good)), false);
    let mut inner = thread.inner.borrow_mut();
    assert!(matches!(
        inner.pending_responses.pop_front(),
        Some(EmuResponse::MemoryWriteRejected(_))
    ));
    assert_eq!(inner.backend.encode_state_bytes().unwrap(), before);
    assert_eq!(inner.pending_audio_discontinuities.len(), 2);
    assert!(!inner.battery_potentially_dirty);
    {
        use crate::emu_thread::{MdTimedPortFailureKind, MdTimedPortRequest};
        use zeff_md_core::emulator::DebugPortWrite;
        let timed = |id, writes| {
            let mut request = edit_input(&[]);
            request.debug_actions.md_timed_ports = Some(MdTimedPortRequest { id, writes });
            request
        };
        let thread = EmuThread::spawn(backend(), false);
        assert_edit_result(&deliver(&thread, edit_input(&[])), true);
        let (checkpoint, registers, queue, original) = {
            let mut inner = thread.inner.borrow_mut();
            let EmuBackend::Md(md) = &inner.backend else {
                unreachable!()
            };
            let checkpoint = md.core().encode_debugger_checkpoint().unwrap();
            let registers = md.core().registers();
            let queue = md.core().queued_words();
            let original = inner.backend.encode_state_bytes().unwrap();
            inner.rewind_buffer = RewindBuffer::new(1, 1);
            inner.rewind_buffer.push(&checkpoint, &[]);
            (checkpoint, registers, queue, original)
        };
        for (id, writes) in [
            (100, vec![]),
            (
                101,
                vec![DebugPortWrite::Byte {
                    address: 0xc00004,
                    value: 4,
                }],
            ),
            (
                102,
                vec![DebugPortWrite::Word {
                    address: 0xc00004,
                    value: 0x8104,
                }],
            ),
        ] {
            thread.send(EmuCommand::StepFrames(Box::new(timed(id, writes))));
            let frame = thread.try_recv_frame().unwrap();
            assert_eq!(
                (frame.advanced_frames, frame.completed_step_requests),
                (0, 1)
            );
            assert!(frame.audio_semantic_frames.is_empty() && frame.runtime_fault.is_none());
            let Some(EmuResponse::MdTimedPortsFinished(reply)) = thread.try_recv_response() else {
                panic!("missing timed-port reply");
            };
            assert_eq!(reply.id, id);
            assert_eq!(reply.delivered_pcm_samples, frame.audio_samples.len());
            match id {
                100 => {
                    let report = reply.result.unwrap();
                    assert_eq!(
                        (
                            report.writes,
                            report.cpu_clocks,
                            report.master_ticks,
                            report.instructions,
                            report.fields_crossed,
                        ),
                        (0, 0, 0, 0, 0)
                    );
                }
                101 => {
                    let error = reply.result.unwrap_err();
                    assert_eq!(
                        (error.kind, error.index),
                        (MdTimedPortFailureKind::Port, Some(0))
                    );
                }
                _ => {
                    let report = reply.result.unwrap();
                    assert_eq!((report.writes, report.instructions), (1, 0));
                    assert!(report.cpu_clocks > 0 && report.master_ticks > 0);
                }
            }
            let inner = thread.inner.borrow();
            let EmuBackend::Md(md) = &inner.backend else {
                unreachable!()
            };
            assert_eq!(md.core().registers(), registers);
            assert_eq!(md.core().queued_words(), queue);
            assert_eq!(inner.rewind_buffer.peek().unwrap().state_bytes, checkpoint);
            if id == 102 {
                assert_eq!(
                    inner.pending_audio_discontinuities,
                    [crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation]
                );
                assert!(inner.battery_potentially_dirty);
            } else {
                assert_eq!(inner.backend.encode_state_bytes().unwrap(), original);
                assert!(inner.pending_audio_discontinuities.is_empty());
                assert!(!inner.battery_potentially_dirty);
            }
            assert!(inner.pending_responses.is_empty() && inner.pending_frames.is_empty());
        }
        thread.send(EmuCommand::StepFrames(Box::new(input(true, false))));
        let next = thread.try_recv_frame().unwrap();
        assert_eq!((next.advanced_frames, next.completed_step_requests), (0, 1));
        assert!(next.runtime_fault.is_none());
        {
            let inner = thread.inner.borrow();
            let EmuBackend::Md(md) = &inner.backend else {
                unreachable!()
            };
            assert_eq!(md.core().vdp_registers()[1], Some(4));
            assert_eq!(inner.rewind_buffer.peek().unwrap().state_bytes, checkpoint);
            assert!(inner.pending_responses.is_empty());
        }
        let before = {
            let mut inner = thread.inner.borrow_mut();
            inner.battery_potentially_dirty = false;
            inner.runtime_fault.latch(Some("timed prior fault".into()));
            inner.backend.encode_state_bytes().unwrap()
        };
        for id in [103, 104] {
            let input = timed(
                id,
                vec![DebugPortWrite::Word {
                    address: 0xc00004,
                    value: 0x8104,
                }],
            );
            thread.send(EmuCommand::StepFrames(Box::new(input)));
            let frame = thread.try_recv_frame().unwrap();
            assert_eq!(
                (frame.advanced_frames, frame.completed_step_requests),
                (0, 1)
            );
            assert_eq!(
                frame.runtime_fault.as_deref(),
                (id == 103).then_some("timed prior fault")
            );
            let Some(EmuResponse::MdTimedPortsFinished(reply)) = thread.try_recv_response() else {
                panic!("missing faulted timed-port reply");
            };
            assert_eq!(reply.id, id);
            let error = reply.result.unwrap_err();
            assert_eq!(
                (error.kind, error.index),
                (MdTimedPortFailureKind::Lifecycle, None)
            );
            let inner = thread.inner.borrow();
            assert!(!inner.runtime_fault.can_step());
            assert_eq!(inner.backend.encode_state_bytes().unwrap(), before);
            assert_eq!(inner.rewind_buffer.peek().unwrap().state_bytes, checkpoint);
            assert_eq!(
                inner.pending_audio_discontinuities,
                [crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation]
            );
            assert!(!inner.battery_potentially_dirty);
            assert!(inner.pending_responses.is_empty() && inner.pending_frames.is_empty());
        }
        thread.send(EmuCommand::Rewind(1));
        assert!(matches!(
            thread.try_recv_response(),
            Some(EmuResponse::RewindOk { .. })
        ));
        let (before, markers) = {
            let mut inner = thread.inner.borrow_mut();
            assert!(inner.runtime_fault.can_step() && !inner.backend.is_suspended());
            assert!(inner.battery_potentially_dirty);
            assert_eq!(
                inner.pending_audio_discontinuities,
                [
                    crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation,
                    crate::audio_recorder::AudioTimelineDiscontinuity::Rewind,
                ]
            );
            assert_eq!(
                inner.backend.encode_rewind_checkpoint().unwrap(),
                checkpoint
            );
            inner.battery_potentially_dirty = false;
            (
                inner.backend.encode_state_bytes().unwrap(),
                inner.pending_audio_discontinuities.clone(),
            )
        };
        let input = timed(
            105,
            vec![DebugPortWrite::Word {
                address: 0xc00004,
                value: 0x8104,
            }],
        );
        thread.send(EmuCommand::StepFrames(Box::new(input)));
        let frame = thread.try_recv_frame().unwrap();
        assert_eq!(
            (frame.advanced_frames, frame.completed_step_requests),
            (0, 1)
        );
        assert!(frame.runtime_fault.is_none());
        let Some(EmuResponse::MdTimedPortsFinished(reply)) = thread.try_recv_response() else {
            panic!("missing running timed-port reply");
        };
        assert_eq!(reply.id, 105);
        let error = reply.result.unwrap_err();
        assert_eq!(
            (error.kind, error.index),
            (MdTimedPortFailureKind::Lifecycle, None)
        );
        let inner = thread.inner.borrow();
        assert_eq!(inner.backend.encode_state_bytes().unwrap(), before);
        assert!(!inner.backend.is_suspended());
        assert_eq!(inner.pending_audio_discontinuities, markers);
        assert!(!inner.battery_potentially_dirty);
        assert!(inner.pending_responses.is_empty() && inner.pending_frames.is_empty());
    }
}
