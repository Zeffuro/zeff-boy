use super::*;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_psg_normal_worker_uses_typed_byte_rejects_mirror_and_matches_future_pcm() {
    for header in *b"UJE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            let actual = port_owner(header, timing, ControllerKind::ThreeButton);
            let mut expected = port_owner(header, timing, ControllerKind::ThreeButton);
            let before = expected.backend.encode_state_bytes().unwrap();
            let queued = machine(&expected.backend)
                .clone()
                .drain_audio_samples()
                .len();
            let mut worker = EmuThread::spawn(actual.backend, false);
            let invalid = vec![
                DebugPortWrite::Byte {
                    address: 0xc00011,
                    value: 0x9f,
                },
                DebugPortWrite::Byte {
                    address: 0xc00013,
                    value: 0x90,
                },
            ];
            let frame = guest_call_deliver(&worker, request(480, invalid));
            let rejected = reply(&worker, 480);
            let failure = rejected.result.unwrap_err();
            assert_eq!(failure.kind, MdTimedPortFailureKind::Port);
            assert_eq!(failure.index, Some(1));
            assert_eq!(rejected.queued_pcm_samples_before, Some(queued));
            assert_eq!(rejected.delivered_pcm_samples, frame.audio_samples.len());
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(capture(&worker), before);
            let registers = machine(&expected.backend).registers();
            let queue = machine(&expected.backend).queued_words();
            let fields = machine(&expected.backend).frame_count();
            let mut writes: Vec<_> = [0x85, 0x12, 0x90]
                .into_iter()
                .map(|value| DebugPortWrite::Byte {
                    address: 0xc00011,
                    value,
                })
                .collect();
            writes.push(control(0x8f02));
            let direct = machine_mut(&mut expected.backend)
                .debug_write_ports(&writes)
                .unwrap();
            let frame = guest_call_deliver(&worker, request(481, writes));
            let response = reply(&worker, 481);
            let result = response.result.unwrap();
            assert_eq!((result.writes, result.instructions), (4, 0));
            assert_eq!(result.cpu_clocks, direct.cpu_clocks.0);
            assert_eq!(result.master_ticks, direct.elapsed.get());
            assert_eq!(
                result.fields_crossed,
                machine(&expected.backend).frame_count() - fields
            );
            assert_eq!(response.delivered_pcm_samples, frame.audio_samples.len());
            assert_eq!(machine(&expected.backend).registers(), registers);
            assert_eq!(machine(&expected.backend).queued_words(), queue);
            assert_eq!(machine(&expected.backend).psg_snapshot().periods[0], 0x125);
            assert_eq!(machine(&expected.backend).psg_snapshot().volumes[0], 0);
            compare(&frame, &snapshot(&mut expected));
            assert_eq!(
                capture(&worker),
                expected.backend.encode_state_bytes().unwrap()
            );
            let before_failure = machine(&expected.backend)
                .encode_debugger_checkpoint()
                .unwrap();
            let mut faulting = vec![
                DebugPortWrite::Byte {
                    address: 0xc00011,
                    value: 0x9f,
                },
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
                panic!("PSG prefix did not reach the later real VDP refusal");
            };
            assert!(index >= 2);
            assert_eq!(
                machine(&expected.backend)
                    .encode_debugger_checkpoint()
                    .unwrap(),
                before_failure
            );
            let frame = guest_call_deliver(&worker, request(482, faulting));
            let failed = reply(&worker, 482).result.unwrap_err();
            assert_eq!(failed.kind, MdTimedPortFailureKind::Execution);
            assert_eq!(failed.index, Some(index));
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
                let actual = guest_call_deliver(&worker, input());
                let expected = guest_call_deliver(&control_worker, input());
                assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
                assert_eq!((actual.advanced_frames, expected.advanced_frames), (1, 1));
                assert_eq!(pcm(&actual), pcm(&expected));
                assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(history(&actual), history(&expected));
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
