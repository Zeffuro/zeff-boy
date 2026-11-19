use super::*;

fn byte(address: u32, value: u8) -> DebugPortWrite {
    DebugPortWrite::Byte { address, value }
}

fn deliver(worker: &EmuThread, expected: &mut Owner, id: u64, writes: Vec<DebugPortWrite>) {
    let registers = machine(&expected.backend).registers();
    let queue = machine(&expected.backend).queued_words();
    let fields = machine(&expected.backend).frame_count();
    let direct = machine_mut(&mut expected.backend)
        .debug_write_ports(&writes)
        .unwrap();
    let frame = guest_call_deliver(worker, request(id, writes));
    let response = reply(worker, id);
    let result = response.result.unwrap();
    assert_eq!(result.instructions, 0);
    assert_eq!(result.writes, direct.writes);
    assert_eq!(result.cpu_clocks, direct.cpu_clocks.0);
    assert_eq!(result.master_ticks, direct.elapsed.get());
    assert_eq!(
        result.fields_crossed,
        machine(&expected.backend).frame_count() - fields
    );
    assert_eq!(response.delivered_pcm_samples, frame.audio_samples.len());
    assert_eq!(machine(&expected.backend).registers(), registers);
    assert_eq!(machine(&expected.backend).queued_words(), queue);
    let expected_frame = snapshot(expected);
    compare(&frame, &expected_frame);
    let sound_lines = |frame: &FrameResult| {
        frame
            .ui_data
            .cpu_debug
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .find(|section| section.heading == "SOUND Z80 (read-only published state)")
            .unwrap()
            .lines
            .clone()
    };
    assert_eq!(sound_lines(&frame), sound_lines(&expected_frame));
    assert_eq!(
        capture(worker),
        expected.backend.encode_state_bytes().unwrap()
    );
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sound_aperture_normal_worker_observes_grant_and_keeps_atomic_fault_and_future_pcm() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let actual = port_owner(header, timing, ControllerKind::ThreeButton);
            let mut expected = port_owner(header, timing, ControllerKind::ThreeButton);
            let registers = machine(&expected.backend).registers();
            let mut worker = EmuThread::spawn(actual.backend, false);
            assert!(machine(&expected.backend).sound_reset_asserted());
            deliver(
                &worker,
                &mut expected,
                600,
                vec![
                    byte(0xa00010, 0xa5),
                    byte(0xa04000, 0x2a),
                    byte(0xa04001, 0xff),
                    byte(0xa06000, 1),
                    control(0x8f02),
                ],
            );
            assert_eq!(machine(&expected.backend).sound_bank(), 0);
            assert_eq!(machine(&expected.backend).opn2_snapshot().dac, 0x80);
            deliver(
                &worker,
                &mut expected,
                601,
                vec![byte(0xa11100, 1), byte(0xa11200, 1), control(0x8f02)],
            );
            for id in 602..858 {
                if machine(&expected.backend).sound_bus_granted() {
                    break;
                }
                deliver(&worker, &mut expected, id, vec![control(0x8f02)]);
            }
            assert!(machine(&expected.backend).sound_bus_granted());
            assert!(!machine(&expected.backend).sound_reset_asserted());
            assert_eq!(machine(&expected.backend).peek(0xa00010), Some(0));
            deliver(
                &worker,
                &mut expected,
                858,
                vec![
                    byte(0xa00010, 0xa5),
                    byte(0xa00011, 0x5a),
                    byte(0xa06000, 1),
                    byte(0xa04002, 0xb6),
                    byte(0xa04003, 0xc0),
                    byte(0xa04000, 0x2a),
                    byte(0xa04001, 0xff),
                    byte(0xa04000, 0x2b),
                    byte(0xa04001, 0x80),
                    control(0x8f02),
                ],
            );
            assert_eq!(machine(&expected.backend).peek(0xa00010), Some(0xa5));
            assert_eq!(machine(&expected.backend).peek(0xa00011), Some(0x5a));
            assert_eq!(machine(&expected.backend).sound_bank(), 0x100);
            assert_eq!(machine(&expected.backend).opn2_snapshot().dac, 0xff);
            assert!(machine(&expected.backend).opn2_snapshot().dac_enabled);
            let before = machine(&expected.backend)
                .encode_debugger_checkpoint()
                .unwrap();
            let rejected = vec![byte(0xa00010, 0), byte(0xa02000, 1)];
            let frame = guest_call_deliver(&worker, request(859, rejected));
            let failure = reply(&worker, 859).result.unwrap_err();
            assert_eq!(
                (failure.kind, failure.index),
                (MdTimedPortFailureKind::Port, Some(1))
            );
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
            let mut faulting = vec![
                byte(0xa00010, 0),
                byte(0xa06000, 0),
                byte(0xa04000, 0x2a),
                byte(0xa04001, 0),
                control(0x8f02),
                control(if timing == TimingModel::Standard {
                    0x8c80
                } else {
                    0x8c02
                }),
            ];
            faulting.extend([control(0x8f02); 64]);
            let DebugPortError::Execution { index, .. } = machine_mut(&mut expected.backend)
                .debug_write_ports(&faulting)
                .unwrap_err()
            else {
                panic!("real aperture prefix did not reach later VDP refusal");
            };
            assert!(index >= 5);
            assert_eq!(
                machine(&expected.backend)
                    .encode_debugger_checkpoint()
                    .unwrap(),
                before
            );
            let frame = guest_call_deliver(&worker, request(860, faulting));
            let failure = reply(&worker, 860).result.unwrap_err();
            assert_eq!(
                (failure.kind, failure.index),
                (MdTimedPortFailureKind::Execution, Some(index))
            );
            assert!(frame.runtime_fault.is_none());
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
            let mut control_worker = EmuThread::spawn(expected.backend, false);
            for _ in 0..2 {
                let input = || {
                    let mut input = guest_call_input(1);
                    input.debug_continue = true;
                    input.debug_actions.remove_breakpoints.push(registers.pc);
                    input
                };
                let a = guest_call_deliver(&worker, input());
                let b = guest_call_deliver(&control_worker, input());
                assert!(a.runtime_fault.is_none() && b.runtime_fault.is_none());
                assert_eq!((a.advanced_frames, b.advanced_frames), (1, 1));
                assert_eq!(pcm(&a), pcm(&b));
                assert!(a.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(history(&a), history(&b));
                assert_eq!(capture(&worker), capture(&control_worker));
                assert_eq!(
                    *worker.shared_framebuffer().load_full().unwrap(),
                    *control_worker.shared_framebuffer().load_full().unwrap()
                );
            }
            worker.shutdown();
            control_worker.shutdown();
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_sound_aperture_delivery_preserves_undo_rewind_and_prior_fault() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = port_owner(b'U', timing, ControllerKind::ThreeButton);
        machine_mut(&mut owner.backend)
            .debug_write_ports(&[byte(0xa11100, 1), byte(0xa11200, 1), control(0x8f02)])
            .unwrap();
        for _ in 0..256 {
            if machine(&owner.backend).sound_bus_granted() {
                break;
            }
            machine_mut(&mut owner.backend)
                .debug_write_ports(&[control(0x8f02)])
                .unwrap();
        }
        assert!(machine(&owner.backend).sound_bus_granted());
        let initial = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let token = owner.call();
        let before = session(&mut owner.backend);
        let mut expected_markers = owner.markers.clone();
        expected_markers.push(crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation);
        let writes = vec![byte(0xa00010, 0xa5), byte(0xa06000, 1), control(0x8f02)];
        let (frame, response, dirty) = EmuThread::handle_md_timed_ports(
            &mut owner.backend,
            request(870, writes),
            owner.rewind.fill_ratio(),
            &mut owner.fault,
            owner.capture,
            &mut owner.markers,
        );
        let EmuResponse::MdTimedPortsFinished(response) = response else {
            panic!("missing reply");
        };
        assert!(response.result.is_ok() && dirty);
        paused(&frame);
        assert!(machine(&owner.backend).sound_bus_granted());
        assert_eq!(machine(&owner.backend).peek(0xa00010), Some(0xa5));
        assert_eq!(machine(&owner.backend).sound_bank(), 0x100);
        assert_eq!(owner.markers, expected_markers);
        assert_eq!(owner.rewind.peek().unwrap().state_bytes, [1, 2, 3]);
        let after = session(&mut owner.backend);
        assert_eq!(after.registers, before.registers);
        assert_eq!(after.definitions, before.definitions);
        assert_eq!(after.trace, before.trace);
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
            request(871, vec![byte(0xa00010, 0x5a)]),
            owner.rewind.fill_ratio(),
            &mut owner.fault,
            owner.capture,
            &mut owner.markers,
        );
        let EmuResponse::MdTimedPortsFinished(response) = response else {
            panic!("missing reply");
        };
        assert_eq!(
            response.result.unwrap_err().kind,
            MdTimedPortFailureKind::Lifecycle
        );
        assert_eq!(frame.runtime_fault.as_deref(), Some("prior worker fault"));
        assert!(!dirty && !owner.fault.can_step());
        assert_eq!(owner.markers, markers);
        assert_eq!(owner.rewind.peek().unwrap().state_bytes, [1, 2, 3]);
        assert_eq!(machine(&owner.backend).encode_state().unwrap(), before);
    }
}
