use super::*;

fn byte(address: u32, value: u8) -> DebugPortWrite {
    DebugPortWrite::Byte { address, value }
}

fn writes() -> Vec<DebugPortWrite> {
    vec![
        byte(0xa10013, 0x38),
        byte(0xa1000f, 0x53),
        byte(0xa10019, 0x38),
        byte(0xa10015, 0x4d),
    ]
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_uart_normal_worker_uses_actual_cycles_and_keeps_rejections_recoverable() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let actual = Owner::new(fixture(header, timing, ControllerKind::ThreeButton, &[]));
            let mut expected =
                Owner::new(fixture(header, timing, ControllerKind::ThreeButton, &[]));
            let registers = machine(&expected.backend).registers();
            let queue = machine(&expected.backend).queued_words();
            let fields = machine(&expected.backend).frame_count();
            let queued = machine(&expected.backend)
                .clone()
                .drain_audio_samples()
                .len();
            let direct = machine_mut(&mut expected.backend)
                .debug_write_ports(&writes())
                .unwrap();
            let mut worker = EmuThread::spawn(actual.backend, false);
            let frame = guest_call_deliver(&worker, request(920, writes()));
            let response = reply(&worker, 920);
            let result = response.result.unwrap();
            assert_eq!((result.writes, result.instructions), (4, 0));
            assert_eq!(result.cpu_clocks, direct.cpu_clocks.0);
            assert_eq!(result.master_ticks, direct.elapsed.get());
            assert_eq!(
                result.fields_crossed,
                machine(&expected.backend).frame_count() - fields
            );
            assert_eq!(response.queued_pcm_samples_before, Some(queued));
            assert_eq!(response.delivered_pcm_samples, frame.audio_samples.len());
            assert_eq!(machine(&expected.backend).registers(), registers);
            assert_eq!(machine(&expected.backend).queued_words(), queue);
            for status in [0xa10013, 0xa10019] {
                assert_eq!(machine(&expected.backend).peek(status), None);
            }
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
            let before = machine(&expected.backend)
                .encode_debugger_checkpoint()
                .unwrap();
            let rejected = vec![byte(0xc00011, 0x9f), byte(0xa10011, 1)];
            let frame = guest_call_deliver(&worker, request(921, rejected));
            let failure = reply(&worker, 921).result.unwrap_err();
            assert_eq!(
                (failure.kind, failure.index),
                (MdTimedPortFailureKind::Port, Some(1))
            );
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
            let faulting = vec![byte(0xc00011, 0x9f), byte(0xa10013, 0x20)];
            let DebugPortError::Execution { index, .. } = machine_mut(&mut expected.backend)
                .debug_write_ports(&faulting)
                .unwrap_err()
            else {
                panic!("used UART mode change did not reach the real owner refusal");
            };
            assert_eq!(index, 1);
            assert_eq!(
                machine(&expected.backend)
                    .encode_debugger_checkpoint()
                    .unwrap(),
                before
            );
            let frame = guest_call_deliver(&worker, request(922, faulting));
            let failure = reply(&worker, 922).result.unwrap_err();
            assert_eq!(
                (failure.kind, failure.index),
                (MdTimedPortFailureKind::Execution, Some(1))
            );
            assert!(frame.runtime_fault.is_none());
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
            machine_mut(&mut expected.backend)
                .debug_write_ports(&[byte(0xa10013, 0x38)])
                .unwrap();
            let frame = guest_call_deliver(&worker, request(923, vec![byte(0xa10013, 0x38)]));
            assert_eq!(reply(&worker, 923).result.unwrap().writes, 1);
            compare(&frame, &snapshot(&mut expected));
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
fn md_uart_delivery_keeps_guest_undo_rewind_and_prior_fault_ownership() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        let mut owner = Owner::new(fixture(b'U', timing, ControllerKind::ThreeButton, &[]));
        let initial = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let token = owner.call();
        let before = session(&mut owner.backend);
        let markers = owner.markers.clone();
        let (frame, response, dirty) = EmuThread::handle_md_timed_ports(
            &mut owner.backend,
            request(924, writes()),
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
        let mut marked = markers;
        marked.push(crate::audio_recorder::AudioTimelineDiscontinuity::DebuggerMutation);
        assert_eq!(owner.markers, marked);
        let changed = machine(&owner.backend)
            .encode_debugger_checkpoint()
            .unwrap();
        let (frame, response, dirty) = EmuThread::handle_md_timed_ports(
            &mut owner.backend,
            request(925, vec![byte(0xc00011, 0x9f), byte(0xa10013, 0x20)]),
            owner.rewind.fill_ratio(),
            &mut owner.fault,
            owner.capture,
            &mut owner.markers,
        );
        let EmuResponse::MdTimedPortsFinished(response) = response else {
            panic!("missing reply");
        };
        let failure = response.result.unwrap_err();
        assert_eq!(
            (failure.kind, failure.index),
            (MdTimedPortFailureKind::Execution, Some(1))
        );
        assert!(!dirty && owner.fault.can_step());
        paused(&frame);
        assert_eq!(owner.markers, marked);
        assert_eq!(
            machine(&owner.backend)
                .encode_debugger_checkpoint()
                .unwrap(),
            changed
        );
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
            request(926, writes()),
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
        assert_eq!(machine(&owner.backend).encode_state().unwrap(), before);
    }
}
