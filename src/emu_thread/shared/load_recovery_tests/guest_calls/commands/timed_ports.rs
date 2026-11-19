use super::*;
use crate::emu_thread::{MdTimedPortFailureKind, MdTimedPortReply, MdTimedPortRequest};
use zeff_md_core::emulator::{DebugPortError, DebugPortWrite};

fn control(value: u16) -> DebugPortWrite {
    DebugPortWrite::Word {
        address: 0xc00004,
        value,
    }
}

fn port_owner(header: u8, timing: TimingModel, pad: ControllerKind) -> Owner {
    let backend = fixture(
        header,
        timing,
        pad,
        &[
            (4, &[0, 0x1900]),
            (0x1900, &[0x33fc, 0x8104, 0x00c0, 4, 0x4ef9, 0, 0x200]),
        ],
    );
    assert_eq!(machine(&backend).registers().pc, 0x1900);
    let owner = Owner::new(backend);
    assert_eq!(machine(&owner.backend).vdp_registers()[1], Some(4));
    owner
}

fn request(id: u64, writes: Vec<DebugPortWrite>) -> FrameInput {
    let input = guest_call_input(0);
    crate::app::md_timed_ports::isolated_input(
        MdTimedPortRequest { id, writes },
        input.snapshot,
        input.buffers,
    )
}

fn reply(thread: &EmuThread, id: u64) -> MdTimedPortReply {
    let EmuResponse::MdTimedPortsFinished(reply) = receive_response(thread) else {
        panic!("expected a typed timed-port response");
    };
    assert_eq!(reply.id, id);
    reply
}

fn snapshot(owner: &mut Owner) -> FrameResult {
    EmuThread::build_inert_frame_result(
        &mut owner.backend,
        guest_call_input(0),
        owner.rewind.fill_ratio(),
        &mut owner.fault,
    )
}

fn compare(actual: &FrameResult, expected: &FrameResult) {
    paused(actual);
    paused(expected);
    assert_eq!(pcm(actual), pcm(expected));
    assert!(actual.audio_semantic_frames.is_empty());
    assert_eq!(history(actual), history(expected));
    let a = actual.ui_data.cpu_debug.as_ref().unwrap();
    let b = expected.ui_data.cpu_debug.as_ref().unwrap();
    assert_eq!(a.pc, b.pc);
    assert_eq!(a.cycles, b.cycles);
    assert_eq!(a.register_lines, b.register_lines);
    assert_eq!(a.hit_breakpoint, b.hit_breakpoint);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_timed_port_worker_literal_word_matches_direct_core_and_next_instruction() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for trace in [false, true] {
            let mut actual = port_owner(b'U', timing, ControllerKind::ThreeButton);
            let mut expected = port_owner(b'U', timing, ControllerKind::ThreeButton);
            for owner in [&mut actual, &mut expected] {
                let mut actions = DebugUiActions::none();
                actions.trace_enabled = Some(trace);
                configure(&mut owner.backend, &actions);
            }
            let registers = machine(&expected.backend).registers();
            let queue = machine(&expected.backend).queued_words();
            let fields = machine(&expected.backend).frame_count();
            let queued = machine(&expected.backend).clone().drain_audio_samples();
            assert!(queued.iter().any(|sample| *sample != 0.0));
            let writes = vec![control(0x8f02)];
            let direct = machine_mut(&mut expected.backend)
                .debug_write_ports(&writes)
                .unwrap();
            let mut worker = EmuThread::spawn(actual.backend, false);
            let frame = guest_call_deliver(&worker, request(7, writes));
            let reply = reply(&worker, 7);
            let result = reply.result.unwrap();
            assert_eq!((result.writes, result.instructions), (1, 0));
            assert_eq!(result.cpu_clocks, direct.cpu_clocks.0);
            assert_eq!(result.master_ticks, direct.elapsed.get());
            assert_eq!(
                result.fields_crossed,
                machine(&expected.backend).frame_count() - fields
            );
            assert_eq!(reply.queued_pcm_samples_before, Some(queued.len()));
            assert_eq!(reply.delivered_pcm_samples, frame.audio_samples.len());
            assert_eq!(machine(&expected.backend).registers(), registers);
            assert_eq!(machine(&expected.backend).queued_words(), queue);
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
            let mut restored = machine(&expected.backend).clone();
            let mut control_worker = EmuThread::spawn(expected.backend, false);
            let mut step = guest_call_input(0);
            step.debug_step = true;
            let next = guest_call_deliver(&worker, step);
            let mut step = guest_call_input(0);
            step.debug_step = true;
            let expected_next = guest_call_deliver(&control_worker, step);
            compare(&next, &expected_next);
            let state = capture(&worker);
            assert_eq!(state, capture(&control_worker));
            restored.decode_state(&state).unwrap();
            assert_eq!(
                restored.vdp_registers()[15],
                Some(2),
                "{timing:?}, trace={trace}"
            );
            worker.shutdown();
            control_worker.shutdown();
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_timed_port_worker_conflicts_and_empty_lists_preserve_state_and_deliver_old_pcm() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let actual = port_owner(b'U', timing, ControllerKind::SixButton);
        let mut expected = port_owner(b'U', timing, ControllerKind::SixButton);
        let mut worker = EmuThread::spawn(actual.backend, false);
        for variant in 0..4 {
            let mut input = request(20 + variant, vec![control(0x8f02)]);
            input.joypad.buttons = 0xff;
            input.audio.skip_audio = true;
            input.rewind_seconds = 123;
            match variant {
                0 => input.debug_continue = true,
                1 => input.debug_step = true,
                2 => input.debug_actions.memory_writes.push((0xff0041, 0xa5)),
                _ => input.debug_actions.apu_channel_mutes = Some(vec![true; 10]),
            }
            let frame = guest_call_deliver(&worker, input);
            let response = reply(&worker, 20 + variant);
            let error = response.result.unwrap_err();
            assert_eq!(error.kind, MdTimedPortFailureKind::Conflict);
            assert_eq!(error.index, None);
            assert_eq!(response.delivered_pcm_samples, frame.audio_samples.len());
            if variant == 0 {
                assert!(frame.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(
                    response.queued_pcm_samples_before,
                    Some(frame.audio_samples.len())
                );
            }
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
        }
        let frame = guest_call_deliver(
            &worker,
            request(
                25,
                vec![
                    control(0x8f02),
                    DebugPortWrite::Byte {
                        address: 0xc00004,
                        value: 2,
                    },
                ],
            ),
        );
        let error = reply(&worker, 25).result.unwrap_err();
        assert_eq!(error.kind, MdTimedPortFailureKind::Port);
        assert_eq!(error.index, Some(1));
        compare(&frame, &snapshot(&mut expected));
        assert_eq!(
            capture(&worker),
            expected.backend.encode_state_bytes().unwrap()
        );
        let frame = guest_call_deliver(&worker, request(24, Vec::new()));
        let outcome = reply(&worker, 24).result.unwrap();
        assert_eq!(
            (
                outcome.writes,
                outcome.cpu_clocks,
                outcome.master_ticks,
                outcome.instructions,
                outcome.fields_crossed
            ),
            (0, 0, 0, 0, 0)
        );
        compare(&frame, &snapshot(&mut expected));
        assert_eq!(
            capture(&worker),
            expected.backend.encode_state_bytes().unwrap()
        );
        worker.shutdown();
    }
    let backend = || {
        let emu = zeff_sega8_core::emulator::Emulator::new_with_hint(
            crate::emu_thread::contract_tests::SMS_AUDIO_ROM,
            44_100,
            zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        )
        .unwrap();
        EmuBackend::from_sega8(emu, "timed-ports.sms".into())
    };
    let capture = |thread: &EmuThread| {
        thread.send(EmuCommand::CaptureStateBytes);
        let EmuResponse::StateCaptured(bytes) = receive_response(thread) else {
            panic!("missing Sega8 state capture");
        };
        assert_eq!(&bytes[..8], b"ZBSEGA8\0");
        assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 12);
        bytes
    };
    let mut worker = EmuThread::spawn(backend(), false);
    let mut control_worker = EmuThread::spawn(backend(), false);
    let warm = guest_call_deliver(&worker, guest_call_input(1));
    let expected = guest_call_deliver(&control_worker, guest_call_input(1));
    assert_eq!((warm.advanced_frames, expected.advanced_frames), (1, 1));
    assert!(warm.audio_samples.iter().any(|sample| *sample != 0.0));
    assert_eq!(pcm(&warm), pcm(&expected));
    assert_eq!(capture(&worker), capture(&control_worker));
    let frame = guest_call_deliver(&worker, request(50, vec![control(0x8f02)]));
    let expected = guest_call_deliver(&control_worker, guest_call_input(0));
    let response = reply(&worker, 50);
    let error = response.result.unwrap_err();
    assert_eq!(
        (error.kind, error.index),
        (MdTimedPortFailureKind::Context, None)
    );
    assert_eq!(response.queued_pcm_samples_before, None);
    assert_eq!(response.delivered_pcm_samples, frame.audio_samples.len());
    assert_eq!(
        (frame.advanced_frames, frame.completed_step_requests),
        (0, 1)
    );
    assert!(frame.audio_semantic_frames.is_empty() && frame.runtime_fault.is_none());
    assert_eq!(pcm(&frame), pcm(&expected));
    assert_eq!(capture(&worker), capture(&control_worker));
    let future = guest_call_deliver(&worker, guest_call_input(1));
    let expected = guest_call_deliver(&control_worker, guest_call_input(1));
    assert_eq!((future.advanced_frames, expected.advanced_frames), (1, 1));
    assert!(future.audio_samples.iter().any(|sample| *sample != 0.0));
    assert_eq!(pcm(&future), pcm(&expected));
    assert_eq!(capture(&worker), capture(&control_worker));
    assert_eq!(
        worker.shared_framebuffer().load_full(),
        control_worker.shared_framebuffer().load_full()
    );
    worker.shutdown();
    control_worker.shutdown();
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_timed_port_worker_discarded_device_fault_reports_index_without_poisoning_worker() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let actual = port_owner(b'U', timing, ControllerKind::ThreeButton);
        let mut expected = port_owner(b'U', timing, ControllerKind::ThreeButton);
        let mut writes = vec![
            DebugPortWrite::Byte {
                address: 0xa11100,
                value: 1,
            },
            control(if timing == TimingModel::Standard {
                0x8c80
            } else {
                0x8c02
            }),
        ];
        if timing == TimingModel::Precise {
            writes.extend([control(0x8f02); 64]);
        }
        let before = machine(&expected.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let DebugPortError::Execution { index, .. } = machine_mut(&mut expected.backend)
            .debug_write_ports(&writes)
            .unwrap_err()
        else {
            panic!("fixture did not reach a real device fault");
        };
        assert!(index >= 1);
        assert_eq!(
            machine(&expected.backend)
                .encode_debugger_checkpoint()
                .unwrap(),
            before
        );
        let mut worker = EmuThread::spawn(actual.backend, false);
        let frame = guest_call_deliver(&worker, request(30, writes));
        let error = reply(&worker, 30).result.unwrap_err();
        assert_eq!(error.kind, MdTimedPortFailureKind::Execution);
        assert_eq!(error.index, Some(index));
        assert!(frame.runtime_fault.is_none());
        compare(&frame, &snapshot(&mut expected));
        assert_eq!(
            capture(&worker),
            expected.backend.encode_state_bytes().unwrap()
        );
        machine_mut(&mut expected.backend)
            .debug_write_ports(&[control(0x8f04)])
            .unwrap();
        let frame = guest_call_deliver(&worker, request(31, vec![control(0x8f04)]));
        assert_eq!(reply(&worker, 31).result.unwrap().writes, 1);
        compare(&frame, &snapshot(&mut expected));
        assert_eq!(
            capture(&worker),
            expected.backend.encode_state_bytes().unwrap()
        );
        worker.shutdown();
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_timed_port_delivery_preserves_guest_undo_rewind_and_existing_fault_policy() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = port_owner(b'U', timing, ControllerKind::ThreeButton);
        let initial = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let token = owner.call();
        let session_before = session(&mut owner.backend);
        let mut expected_markers = owner.markers.clone();
        expected_markers.push(crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation);
        let (frame, response, dirty) = EmuThread::handle_md_timed_ports(
            &mut owner.backend,
            request(40, vec![control(0x8f02)]),
            owner.rewind.fill_ratio(),
            &mut owner.fault,
            owner.capture,
            &mut owner.markers,
        );
        let EmuResponse::MdTimedPortsFinished(reply) = response else {
            panic!("missing response")
        };
        assert!(reply.result.is_ok() && dirty);
        paused(&frame);
        assert_eq!(owner.markers, expected_markers);
        assert_eq!(owner.rewind.peek().unwrap().state_bytes, [1, 2, 3]);
        let host = session(&mut owner.backend);
        assert_eq!(host.registers, session_before.registers);
        assert_eq!(host.definitions, session_before.definitions);
        assert_eq!(host.trace, session_before.trace);
        owner.undo(token);
        assert_eq!(
            machine(&owner.backend)
                .encode_debugger_checkpoint()
                .unwrap(),
            initial
        );
        owner.fault.latch(Some("prior worker fault".into()));
        let before = machine(&owner.backend).encode_state().unwrap();
        let markers = owner.markers.clone();
        let (frame, response, dirty) = EmuThread::handle_md_timed_ports(
            &mut owner.backend,
            request(41, vec![control(0x8f02)]),
            owner.rewind.fill_ratio(),
            &mut owner.fault,
            owner.capture,
            &mut owner.markers,
        );
        let EmuResponse::MdTimedPortsFinished(reply) = response else {
            panic!("missing response")
        };
        assert_eq!(
            reply.result.unwrap_err().kind,
            MdTimedPortFailureKind::Lifecycle
        );
        assert_eq!(frame.runtime_fault.as_deref(), Some("prior worker fault"));
        assert!(!dirty && !owner.fault.can_step());
        assert_eq!(owner.markers, markers);
        assert_eq!(owner.rewind.peek().unwrap().state_bytes, [1, 2, 3]);
        assert_eq!(machine(&owner.backend).encode_state().unwrap(), before);
    }
}

mod psg;
mod sound_aperture;

mod save;
mod uart;
