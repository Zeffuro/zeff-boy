use super::super::{EmuLoop, EmuLoopConfig};
use crate::emu_thread::shared::md_step_persistence_tests::{backend, input, sram};
use crate::emu_thread::{EmuCommand, FrameInput, FrameResult};
use std::time::{Duration, Instant};

fn worker() -> EmuLoop {
    worker_with_responses().0
}

fn worker_with_responses() -> (
    EmuLoop,
    crossbeam_channel::Receiver<crate::emu_thread::EmuResponse>,
) {
    let (_commands, cmd_rx) = crossbeam_channel::unbounded();
    let (frame_tx, frame_rx) = crossbeam_channel::bounded(2);
    let (resp_tx, responses) = crossbeam_channel::unbounded();
    (
        EmuLoop::new(
            backend(),
            cmd_rx,
            frame_tx,
            frame_rx,
            resp_tx,
            EmuLoopConfig {
                shared_framebuffer: crate::emu_thread::types::new_shared_framebuffer(),
                save_recovery_on_shutdown: false,
                recovery: None,
            },
        ),
        responses,
    )
}

#[test]
fn md_memory_edit_native_worker_accounts_only_accepted_batches_and_reports_rejection() {
    use crate::emu_thread::EmuResponse;
    use crate::emu_thread::shared::md_step_persistence_tests::{assert_edit_result, edit_input};
    let (mut worker, responses) = worker_with_responses();
    let start = Instant::now();
    worker.battery_flush = crate::emu_thread::persistence::BatteryFlushSchedule::new(start);
    assert_edit_result(&deliver(&mut worker, edit_input(&[])), true);
    worker.rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
    worker.rewind_buffer.push(&[1, 2, 3], &[]);
    let before = worker.backend.encode_state_bytes().unwrap();
    for writes in [vec![], vec![(0xff0041, 0x55), (0xa00000, 0x12)]] {
        let rejected = !writes.is_empty();
        let result = deliver(&mut worker, edit_input(&writes));
        assert_edit_result(&result, true);
        assert!(result.runtime_fault.is_none());
        assert_eq!(worker.backend.encode_state_bytes().unwrap(), before);
        assert_eq!(worker.battery_flush.wait_timeout(start), None);
        assert_eq!(worker.rewind_buffer.peek().unwrap().state_bytes, [1, 2, 3]);
        if rejected {
            assert!(matches!(
                responses.try_recv(),
                Ok(EmuResponse::MemoryWriteRejected(_))
            ));
        }
        assert!(responses.is_empty());
    }
    let result = deliver(&mut worker, edit_input(&[(0xff0041, 0xa5)]));
    assert_edit_result(&result, true);
    assert_eq!(
        worker.pending_audio_discontinuities,
        [crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation]
    );
    assert!(worker.battery_flush.wait_timeout(start).is_some());
    assert!(responses.is_empty());
    let mut request = edit_input(&[(0xff0042, 0x5a)]);
    request.debug_continue = true;
    let result = deliver(&mut worker, request);
    assert_edit_result(&result, false);
    assert_eq!(
        worker.pending_audio_discontinuities,
        [crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation; 2]
    );
    worker.battery_flush.finish_attempt(start, true);
    let before = worker.backend.encode_state_bytes().unwrap();
    let result = deliver(&mut worker, edit_input(&[(0xff0041, 0)]));
    assert_edit_result(&result, false);
    assert!(matches!(
        responses.try_recv(),
        Ok(EmuResponse::MemoryWriteRejected(_))
    ));
    assert_eq!(worker.backend.encode_state_bytes().unwrap(), before);
    assert_eq!(worker.battery_flush.wait_timeout(start), None);
    assert_eq!(worker.pending_audio_discontinuities.len(), 2);
    worker.backend.debug_suspend();
    worker.runtime_fault.latch(Some("worker-only fault".into()));
    for _ in 0..2 {
        let result = deliver(&mut worker, edit_input(&[(0xff0041, 0)]));
        assert_edit_result(&result, false);
        assert!(matches!(
            responses.try_recv(),
            Ok(EmuResponse::MemoryWriteRejected(_))
        ));
        assert_eq!(worker.backend.encode_state_bytes().unwrap(), before);
        assert_eq!(worker.battery_flush.wait_timeout(start), None);
        assert_eq!(worker.pending_audio_discontinuities.len(), 2);
    }
    let (mut worker, responses) = worker_with_responses();
    worker.battery_flush = crate::emu_thread::persistence::BatteryFlushSchedule::new(start);
    let mut request = edit_input(&[(0xff0041, 0xa5)]);
    request.debug_continue = true;
    request.frames = 1;
    let result = deliver(&mut worker, request);
    assert!(result.runtime_fault.is_some());
    assert_edit_result(&result, false);
    assert!(worker.battery_flush.wait_timeout(start).is_some());
    let crate::emu_backend::EmuBackend::Md(md) = &worker.backend else {
        unreachable!()
    };
    assert_eq!(md.core().work_ram()[0x41], 0xa5);
    assert!(responses.is_empty());
    worker.battery_flush.finish_attempt(start, true);
    let result = deliver(&mut worker, edit_input(&[(0xff0041, 0)]));
    assert_edit_result(&result, false);
    assert!(matches!(
        responses.try_recv(),
        Ok(EmuResponse::MemoryWriteRejected(_))
    ));
    assert_eq!(worker.battery_flush.wait_timeout(start), None);
    assert!(worker.backend.encode_state_bytes().is_err());
}

fn deliver(worker: &mut EmuLoop, input: FrameInput) -> FrameResult {
    let writes_requested = !input.debug_actions.memory_writes.is_empty();
    assert!(worker.handle_command(EmuCommand::StepFrames(Box::new(input))));
    let result = worker.drain_rx.try_recv().unwrap();
    assert_eq!(result.advanced_frames, 0);
    assert_eq!(result.completed_step_requests, 1);
    if !writes_requested {
        assert!(worker.pending_audio_discontinuities.is_empty());
    }
    result
}

#[test]
fn md_step_persistence_native_worker_rearms_dirty_for_sram_and_faulting_steps() {
    let mut worker = worker();
    let initial = sram(&worker.backend);
    let start = Instant::now();
    let interval = crate::emu_thread::persistence::BATTERY_FLUSH_INTERVAL;
    worker.battery_flush = crate::emu_thread::persistence::BatteryFlushSchedule::new(start);
    assert!(
        deliver(&mut worker, input(false, false))
            .runtime_fault
            .is_none()
    );
    assert_eq!(worker.battery_flush.wait_timeout(start), None);
    assert!(
        deliver(&mut worker, input(true, true))
            .runtime_fault
            .is_none()
    );
    assert!(!worker.backend.is_suspended());
    assert_eq!(sram(&worker.backend), initial);
    assert_eq!(worker.battery_flush.wait_timeout(start), None);
    worker.backend.debug_suspend();

    let result = deliver(&mut worker, input(true, false));
    assert!(result.runtime_fault.is_none());
    assert!(worker.backend.is_suspended());
    let mut expected = initial;
    expected[0] = 0xa5;
    assert_eq!(sram(&worker.backend), expected);
    assert_eq!(worker.battery_flush.wait_timeout(start), Some(interval));
    assert!(worker.battery_flush.is_due(start + interval));

    worker.battery_flush.finish_attempt(start + interval, true);
    assert_eq!(worker.battery_flush.wait_timeout(start + interval), None);
    let fault = deliver(&mut worker, input(true, false));
    assert!(fault.runtime_fault.is_some());
    assert!(!worker.runtime_fault.can_step());
    assert!(worker.backend.is_suspended());
    assert_eq!(sram(&worker.backend), expected);
    assert!(worker.battery_flush.is_due(start + interval + interval));
    assert_eq!(
        worker
            .battery_flush
            .wait_timeout(start + interval + interval),
        Some(Duration::ZERO)
    );
}

#[test]
fn md_pc_breakpoint_native_worker_marks_zero_field_sram_and_fault_progress_dirty() {
    let mut worker = worker();
    let start = Instant::now();
    let interval = crate::emu_thread::persistence::BATTERY_FLUSH_INTERVAL;
    worker.battery_flush = crate::emu_thread::persistence::BatteryFlushSchedule::new(start);
    let initial = sram(&worker.backend);
    let mut request = input(false, true);
    request.frames = 5;
    request.debug_actions.add_breakpoint = Some(0x200);
    let result = deliver(&mut worker, request);
    assert!(result.runtime_fault.is_none());
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x200)
    );
    assert_eq!(worker.battery_flush.wait_timeout(start), None);
    assert_eq!(sram(&worker.backend), initial);

    let mut request = input(false, true);
    request.frames = 5;
    request.debug_actions.remove_breakpoints.push(0x200);
    request.debug_actions.add_one_shot_breakpoint = Some(0x208);
    let result = deliver(&mut worker, request);
    assert!(result.runtime_fault.is_none());
    assert_eq!(
        result.ui_data.cpu_debug.unwrap().hit_breakpoint,
        Some(0x208)
    );
    assert_eq!(sram(&worker.backend)[0], 0xa5);
    assert_eq!(worker.battery_flush.wait_timeout(start), Some(interval));

    worker.battery_flush.finish_attempt(start + interval, true);
    assert_eq!(worker.battery_flush.wait_timeout(start + interval), None);
    let mut request = input(false, true);
    request.frames = 1;
    let result = deliver(&mut worker, request);
    assert!(result.runtime_fault.is_some());
    assert!(!worker.runtime_fault.can_step());
    assert!(worker.backend.is_suspended());
    assert_eq!(sram(&worker.backend)[0], 0xa5);
    assert!(worker.battery_flush.is_due(start + interval + interval));
}

#[test]
fn md_pc_breakpoint_native_worker_marks_write_then_fault_in_one_request_dirty() {
    let mut worker = worker();
    let start = Instant::now();
    let interval = crate::emu_thread::persistence::BATTERY_FLUSH_INTERVAL;
    worker.battery_flush = crate::emu_thread::persistence::BatteryFlushSchedule::new(start);
    let initial = sram(&worker.backend);
    assert_ne!(initial[0], 0xa5);
    let mut request = input(false, true);
    request.frames = 1;
    request.debug_actions.add_breakpoint = Some(0x300);
    let result = deliver(&mut worker, request);
    assert!(result.runtime_fault.is_some());
    assert!(!worker.runtime_fault.can_step() && worker.backend.is_suspended());
    let mut expected = initial;
    expected[0] = 0xa5;
    assert_eq!(sram(&worker.backend), expected);
    assert_eq!(worker.battery_flush.wait_timeout(start), Some(interval));
    assert!(worker.battery_flush.is_due(start + interval));
}

#[test]
fn md_input_edit_native_worker_retains_audio_dirty_rewind_and_fault_policy() {
    use crate::emu_thread::EmuResponse;
    use crate::emu_thread::shared::md_step_persistence_tests::{assert_edit_result, edit_input};
    let (mut worker, responses) = worker_with_responses();
    let start = Instant::now();
    worker.battery_flush = crate::emu_thread::persistence::BatteryFlushSchedule::new(start);
    assert_edit_result(&deliver(&mut worker, edit_input(&[])), true);
    worker.rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
    worker.rewind_buffer.push(&[1, 2, 3], &[]);
    let original = worker.backend.encode_state_bytes().unwrap();
    let bad = [
        (0xff0041, 0x55),
        (0xa10009, 0x40),
        (0xa10003, 0),
        (0xa10002, 0),
    ];
    assert_edit_result(&deliver(&mut worker, edit_input(&bad)), true);
    assert!(matches!(
        responses.try_recv(),
        Ok(EmuResponse::MemoryWriteRejected(_))
    ));
    assert_eq!(worker.backend.encode_state_bytes().unwrap(), original);
    assert!(worker.pending_audio_discontinuities.is_empty());
    assert_eq!(worker.battery_flush.wait_timeout(start), None);
    let good = [(0xff0041, 0xa5), (0xa10009, 0x40), (0xa10003, 0)];
    for count in 1..=2 {
        assert_edit_result(&deliver(&mut worker, edit_input(&good)), true);
        assert_eq!(
            worker.pending_audio_discontinuities,
            vec![crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation; count]
        );
        assert!(worker.battery_flush.wait_timeout(start).is_some());
        assert_eq!(worker.rewind_buffer.peek().unwrap().state_bytes, [1, 2, 3]);
        assert!(responses.is_empty());
    }
    let before = worker.backend.encode_state_bytes().unwrap();
    worker.battery_flush.finish_attempt(start, true);
    worker.runtime_fault.latch(Some("worker-only fault".into()));
    assert_edit_result(&deliver(&mut worker, edit_input(&good)), false);
    assert!(matches!(
        responses.try_recv(),
        Ok(EmuResponse::MemoryWriteRejected(_))
    ));
    assert_eq!(worker.backend.encode_state_bytes().unwrap(), before);
    assert_eq!(worker.pending_audio_discontinuities.len(), 2);
    assert_eq!(worker.battery_flush.wait_timeout(start), None);
    {
        use crate::emu_backend::EmuBackend;
        use crate::emu_thread::{MdTimedPortFailureKind, MdTimedPortRequest};
        use zeff_md_core::emulator::DebugPortWrite;
        let timed = |id, writes| {
            let mut request = edit_input(&[]);
            request.debug_actions.md_timed_ports = Some(MdTimedPortRequest { id, writes });
            request
        };
        let (mut worker, responses) = worker_with_responses();
        worker.battery_flush = crate::emu_thread::persistence::BatteryFlushSchedule::new(start);
        assert_edit_result(&deliver(&mut worker, edit_input(&[])), true);
        let EmuBackend::Md(md) = &worker.backend else {
            unreachable!()
        };
        let checkpoint = md.core().encode_debugger_checkpoint().unwrap();
        let registers = md.core().registers();
        let queue = md.core().queued_words();
        let original = worker.backend.encode_state_bytes().unwrap();
        worker.rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
        worker.rewind_buffer.push(&checkpoint, &[]);
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
            assert!(worker.handle_command(EmuCommand::StepFrames(Box::new(timed(id, writes)))));
            let frame = worker.drain_rx.try_recv().unwrap();
            assert_eq!(
                (frame.advanced_frames, frame.completed_step_requests),
                (0, 1)
            );
            assert!(frame.audio_semantic_frames.is_empty() && frame.runtime_fault.is_none());
            let Ok(EmuResponse::MdTimedPortsFinished(reply)) = responses.try_recv() else {
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
            let EmuBackend::Md(md) = &worker.backend else {
                unreachable!()
            };
            assert_eq!(md.core().registers(), registers);
            assert_eq!(md.core().queued_words(), queue);
            assert_eq!(worker.rewind_buffer.peek().unwrap().state_bytes, checkpoint);
            if id == 102 {
                assert_eq!(
                    worker.pending_audio_discontinuities,
                    [crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation]
                );
                assert_eq!(
                    worker.battery_flush.wait_timeout(start),
                    Some(crate::emu_thread::persistence::BATTERY_FLUSH_INTERVAL)
                );
            } else {
                assert_eq!(worker.backend.encode_state_bytes().unwrap(), original);
                assert!(worker.pending_audio_discontinuities.is_empty());
                assert_eq!(worker.battery_flush.wait_timeout(start), None);
            }
            assert!(responses.is_empty() && worker.drain_rx.is_empty());
        }
        assert!(worker.handle_command(EmuCommand::StepFrames(Box::new(input(true, false)))));
        let next = worker.drain_rx.try_recv().unwrap();
        assert_eq!((next.advanced_frames, next.completed_step_requests), (0, 1));
        assert!(next.runtime_fault.is_none() && responses.is_empty());
        let EmuBackend::Md(md) = &worker.backend else {
            unreachable!()
        };
        assert_eq!(md.core().vdp_registers()[1], Some(4));
        assert_eq!(worker.rewind_buffer.peek().unwrap().state_bytes, checkpoint);
        worker.battery_flush.finish_attempt(start, true);
        let before = worker.backend.encode_state_bytes().unwrap();
        worker.runtime_fault.latch(Some("timed prior fault".into()));
        for id in [103, 104] {
            let input = timed(
                id,
                vec![DebugPortWrite::Word {
                    address: 0xc00004,
                    value: 0x8104,
                }],
            );
            assert!(worker.handle_command(EmuCommand::StepFrames(Box::new(input))));
            let frame = worker.drain_rx.try_recv().unwrap();
            assert_eq!(
                (frame.advanced_frames, frame.completed_step_requests),
                (0, 1)
            );
            assert_eq!(
                frame.runtime_fault.as_deref(),
                (id == 103).then_some("timed prior fault")
            );
            let Ok(EmuResponse::MdTimedPortsFinished(reply)) = responses.try_recv() else {
                panic!("missing faulted timed-port reply");
            };
            assert_eq!(reply.id, id);
            let error = reply.result.unwrap_err();
            assert_eq!(
                (error.kind, error.index),
                (MdTimedPortFailureKind::Lifecycle, None)
            );
            assert!(!worker.runtime_fault.can_step());
            assert_eq!(worker.backend.encode_state_bytes().unwrap(), before);
            assert_eq!(worker.rewind_buffer.peek().unwrap().state_bytes, checkpoint);
            assert_eq!(
                worker.pending_audio_discontinuities,
                [crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation]
            );
            assert_eq!(worker.battery_flush.wait_timeout(start), None);
            assert!(responses.is_empty() && worker.drain_rx.is_empty());
        }
        assert!(worker.handle_command(EmuCommand::Rewind(1)));
        assert!(matches!(
            responses.try_recv(),
            Ok(EmuResponse::RewindOk { .. })
        ));
        assert!(worker.runtime_fault.can_step() && !worker.backend.is_suspended());
        assert!(worker.battery_flush.wait_timeout(start).is_some());
        assert_eq!(
            worker.pending_audio_discontinuities,
            [
                crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation,
                crate::audio_recorder::AudioTimelineDiscontinuity::Rewind,
            ]
        );
        assert_eq!(
            worker.backend.encode_rewind_checkpoint().unwrap(),
            checkpoint
        );
        worker.battery_flush.finish_attempt(start, true);
        let before = worker.backend.encode_state_bytes().unwrap();
        let markers = worker.pending_audio_discontinuities.clone();
        let input = timed(
            105,
            vec![DebugPortWrite::Word {
                address: 0xc00004,
                value: 0x8104,
            }],
        );
        assert!(worker.handle_command(EmuCommand::StepFrames(Box::new(input))));
        let frame = worker.drain_rx.try_recv().unwrap();
        assert_eq!(
            (frame.advanced_frames, frame.completed_step_requests),
            (0, 1)
        );
        assert!(frame.runtime_fault.is_none());
        let Ok(EmuResponse::MdTimedPortsFinished(reply)) = responses.try_recv() else {
            panic!("missing running timed-port reply");
        };
        assert_eq!(reply.id, 105);
        let error = reply.result.unwrap_err();
        assert_eq!(
            (error.kind, error.index),
            (MdTimedPortFailureKind::Lifecycle, None)
        );
        assert_eq!(worker.backend.encode_state_bytes().unwrap(), before);
        assert!(!worker.backend.is_suspended());
        assert_eq!(worker.pending_audio_discontinuities, markers);
        assert_eq!(worker.battery_flush.wait_timeout(start), None);
        assert!(responses.is_empty() && worker.drain_rx.is_empty());
    }
}
