use super::*;
use crate::debug::DebugUiActions;
use crate::emu_thread::shared::md_event_tests::dma::{before_arm, dma_actions};
use zeff_emu_common::debug::{DebugEvent, TraceBoundaryKind};
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::TimingModel;

fn deliver(
    thread: &EmuThread,
    frames: usize,
    actions: DebugUiActions,
    resume: bool,
) -> FrameResult {
    let mut request = input(frames, 0, 0);
    request.rewind_enabled = false;
    request.debug_actions = actions;
    request.debug_continue = resume;
    request.snapshot.want_debug_info = true;
    request.snapshot.show_instruction_trace = true;
    thread.send(EmuCommand::StepFrames(Box::new(request)));
    receive_frame(thread)
}

fn first_external_boundary(backend: &mut EmuBackend) -> (u32, [u8; 2]) {
    let EmuBackend::Md(md) = backend else {
        panic!("expected MD reference");
    };
    assert_eq!(md.core().vdp_registers()[19], Some(8));
    assert_eq!(md.core().vdp_registers()[20], Some(0));
    for _ in 0..512 {
        let pc = md.core().registers().pc;
        let opcode = md.core().queued_words().unwrap()[0].to_be_bytes();
        let result = md
            .test_normal_core_mut()
            .unwrap()
            .step_instruction()
            .unwrap();
        assert!(matches!(
            result.cpu,
            zeff_m68k::timing::TimedOutcome::Instruction(_)
        ));
        let registers = md.core().vdp_registers();
        if registers[19] != Some(8) || registers[20] != Some(0) {
            assert_eq!(registers[20], Some(0));
            assert!(registers[19].unwrap() < 8);
            return (pc, opcode);
        }
    }
    panic!("no ordinary boundary with actual external source-counter progress");
}

fn stopped(frame: &FrameResult) {
    assert_eq!(frame.advanced_frames, 0);
    assert_eq!(frame.completed_step_requests, 1);
    assert!(frame.runtime_fault.is_none());
    let cpu = frame.ui_data.cpu_debug.as_ref().unwrap();
    assert_eq!(cpu.hit_event, Some(DebugEvent::Dma));
    assert!(cpu.supported_events.contains(&DebugEvent::Dma));
    let features = frame.ui_data.core_features.as_ref().unwrap();
    assert!(features.supports_debugger);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_dma_actual_platform_workers_stop_inspect_disable_and_continue_exactly() {
    for header in *b"UE" {
        for timing in [TimingModel::Standard, TimingModel::Precise] {
            for trace in [false, true] {
                let tail = [0x4e71, 0x60fe, 0x4e71];
                let subject_backend = before_arm(header, timing, &tail);
                let mut reference_backend = before_arm(header, timing, &tail);
                assert_eq!(
                    subject_backend.encode_state_bytes().unwrap(),
                    reference_backend.encode_state_bytes().unwrap()
                );
                assert_eq!(
                    machine(&subject_backend).encode_frontend_state().unwrap(),
                    machine(&reference_backend).encode_frontend_state().unwrap()
                );
                let (executed_pc, opcode) = first_external_boundary(&mut reference_backend);
                let reference_machine = machine(&reference_backend);
                let expected_clock = reference_machine.timing_snapshot().now();
                let expected_cpu = reference_machine.registers();
                let mut prefix_pcm = Vec::new();
                reference_backend.drain_audio_samples_into(&mut prefix_pcm);
                let prefix_bits = prefix_pcm
                    .iter()
                    .map(|sample| sample.to_bits())
                    .collect::<Vec<_>>();
                let stopped_raw = reference_backend.encode_state_bytes().unwrap();
                reference_backend.debug_suspend();
                let mut subject = EmuThread::spawn(subject_backend, false);
                let result = deliver(&subject, 2, dma_actions(true, trace), false);
                stopped(&result);
                assert_eq!(pcm(&result), prefix_bits);
                assert_eq!(capture(&subject), stopped_raw);
                let cpu = result.ui_data.cpu_debug.as_ref().unwrap();
                assert_eq!(cpu.pc, expected_cpu.pc);
                assert_eq!(cpu.cycles, reference_machine.cpu_clock().0);
                assert!(result.audio_timeline_discontinuities.is_empty());
                if trace {
                    let rows = &result.ui_data.instruction_trace.as_ref().unwrap().entries;
                    let row = rows.last().unwrap();
                    assert_eq!(row.pc, executed_pc);
                    assert!(row.instruction_bytes().starts_with(&opcode));
                    assert_eq!(row.event, Some(DebugEvent::Dma));
                    let boundary = row.boundary.unwrap();
                    assert_eq!(boundary.kind, TraceBoundaryKind::Instruction);
                    assert!(!boundary.machine_fault);
                } else {
                    let trace = result.ui_data.instruction_trace.as_ref().unwrap();
                    assert!(!trace.enabled && trace.entries.is_empty());
                }

                let mut control = EmuThread::spawn(reference_backend, false);
                let control_paused = deliver(&control, 0, DebugUiActions::none(), false);
                assert_eq!(control_paused.advanced_frames, 0);
                assert!(control_paused.runtime_fault.is_none());
                assert!(control_paused.audio_samples.is_empty());
                assert_eq!(
                    control_paused.ui_data.cpu_debug.as_ref().unwrap().hit_event,
                    None
                );
                assert_eq!(capture(&subject), capture(&control));
                for _ in 0..2 {
                    let paused = deliver(&subject, 1, DebugUiActions::none(), false);
                    stopped(&paused);
                    assert!(paused.audio_samples.is_empty());
                    assert!(paused.audio_timeline_discontinuities.is_empty());
                    assert_eq!(capture(&subject), stopped_raw);
                }

                let resumed = deliver(&subject, 1, dma_actions(false, false), true);
                let expected = deliver(&control, 1, dma_actions(false, false), true);
                assert_eq!((resumed.advanced_frames, expected.advanced_frames), (1, 1));
                assert!(resumed.runtime_fault.is_none() && expected.runtime_fault.is_none());
                assert_eq!(resumed.ui_data.cpu_debug.as_ref().unwrap().hit_event, None);
                assert!(
                    !resumed
                        .ui_data
                        .cpu_debug
                        .as_ref()
                        .unwrap()
                        .event_breakpoints
                        .contains(&DebugEvent::Dma)
                );
                assert!(resumed.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(pcm(&resumed), pcm(&expected));
                assert_eq!(capture(&subject), capture(&control));
                assert_eq!(
                    subject.shared_framebuffer().load_full(),
                    control.shared_framebuffer().load_full()
                );
                let mut post = reference_machine.clone();
                post.decode_state(&capture(&subject)).unwrap();
                assert_eq!(
                    &post.vdp().memory().vram()[0x400..0x408],
                    &[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]
                );
                assert!(post.timing_snapshot().now().get() > expected_clock.get());

                let actual = deliver(&subject, 1, dma_actions(false, false), false);
                let expected = deliver(&control, 1, dma_actions(false, false), false);
                assert_eq!((actual.advanced_frames, expected.advanced_frames), (1, 1));
                assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
                assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(pcm(&actual), pcm(&expected));
                assert_eq!(capture(&subject), capture(&control));
                let actual_video = subject.shared_framebuffer().load_full().unwrap();
                let expected_video = control.shared_framebuffer().load_full().unwrap();
                assert_eq!(actual_video, expected_video);
                subject.shutdown();
                control.shutdown();
                #[cfg(not(target_arch = "wasm32"))]
                println!(
                    "MD_DMA_TRANSPORT_PASS header={} timing={timing:?} trace={trace}",
                    char::from(header)
                );
                #[cfg(target_arch = "wasm32")]
                wasm_bindgen_test::console_log!(
                    "MD_DMA_TRANSPORT_PASS header={} timing={timing:?} trace={trace}",
                    char::from(header)
                );
            }
        }
    }
}

mod rom_breakpoints;
