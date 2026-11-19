use super::*;
use zeff_emu_common::cheats::{CheatPatch, CheatValue};
use zeff_md_core::{ControllerKind, TimingModel};

const INNER: u32 = 0x408;

pub(super) fn backend(timing: TimingModel) -> EmuBackend {
    guest_call_backend_with(
        b'U',
        timing,
        ControllerKind::ThreeButton,
        &[
            (0, &[0x00ff, 0xfffc, 0x8000, 0x0200]),
            (
                0x200,
                &[
                    0x13fc, 0x80, 0xc0, 0x11, 0x13fc, 0x10, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11,
                    0x4eb9, 0, 0x400, 0x60fe, 0x4e71,
                ],
            ),
            (0x400, &[0x6106, 0x4e75, 0x4e71, 0x4e71]),
            (INNER, &[0x4a39, 0x00ff, 0x0040, 0x67f8, 0x4e75, 0x4e71]),
        ],
        false,
    )
}

fn input(frames: usize, inspect: bool) -> FrameInput {
    let mut request = guest_call_input(frames);
    request.snapshot.want_debug_info = inspect;
    request.snapshot.memory_view_start = 0x1800;
    request
}

fn stack(frame: &FrameResult) -> Vec<(u32, u32, &'static str)> {
    let cpu = frame.ui_data.cpu_debug.as_ref().unwrap();
    assert!(cpu.call_stack_available);
    assert!(
        cpu.call_stack
            .iter()
            .all(|row| row.target_rom_offset.is_none() && row.return_rom_offset.is_none())
    );
    cpu.call_stack
        .iter()
        .map(|row| (row.target, row.return_address, row.kind))
        .collect()
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_call_stack_actual_workers_capture_pause_and_return_without_changing_future_output() {
    for timing in [TimingModel::Standard, TimingModel::Precise] {
        for trace in [false, true] {
            let mut subject = EmuThread::spawn(backend(timing), false);
            let mut control = EmuThread::spawn(backend(timing), false);
            for thread in [&subject, &control] {
                thread.send(EmuCommand::UpdateCheats(vec![CheatPatch::WideRomWrite {
                    address: 0x1800,
                    value: CheatValue::Constant(0xa5),
                }]));
            }
            let mut actual = input(2, true);
            actual.debug_actions.trace_enabled = Some(trace);
            let mut expected = input(2, false);
            expected.debug_actions.trace_enabled = Some(trace);
            let actual = guest_call_deliver(&subject, actual);
            let expected = guest_call_deliver(&control, expected);
            assert_eq!((actual.advanced_frames, expected.advanced_frames), (2, 2));
            assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
            assert_eq!(
                stack(&actual),
                [(0x400, 0x8000021e, "JSR"), (INNER, 0x402, "BSR")]
            );
            assert!(
                actual
                    .ui_data
                    .core_features
                    .as_ref()
                    .unwrap()
                    .supports_debugger
            );
            assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
            assert_eq!(pcm(&actual), pcm(&expected));
            assert_eq!(capture(&subject), capture(&control));
            for (thread, inspect) in [(&subject, true), (&control, false)] {
                let mut stop = input(1, inspect);
                stop.debug_actions.add_breakpoint = Some(INNER);
                paused(&guest_call_deliver(thread, stop));
            }
            let before = capture(&subject);
            assert_eq!(before, capture(&control));
            for _ in 0..8 {
                let frame = guest_call_deliver(&subject, input(0, true));
                paused(&frame);
                assert!(frame.audio_samples.is_empty());
                assert_eq!(
                    stack(&frame),
                    [(0x400, 0x8000021e, "JSR"), (INNER, 0x402, "BSR")]
                );
                assert_eq!(frame.ui_data.cpu_debug.as_ref().unwrap().pc, INNER);
                assert!(frame.ui_data.memory_page.unwrap().contains(&(0x1800, 0xa5)));
                assert_eq!(capture(&subject), before);
            }
            for round in 0..2 {
                let run = |inspect| {
                    let mut request = input(1, inspect);
                    if round == 0 {
                        request.debug_actions.memory_writes.push((0xff0040, 1));
                        request.debug_actions.remove_breakpoints.push(INNER);
                        request.debug_continue = true;
                    }
                    request
                };
                let actual = guest_call_deliver(&subject, run(true));
                let expected = guest_call_deliver(&control, run(false));
                assert_eq!((actual.advanced_frames, expected.advanced_frames), (1, 1));
                assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
                assert!(stack(&actual).is_empty());
                assert!(actual.audio_samples.iter().any(|sample| *sample != 0.0));
                assert_eq!(pcm(&actual), pcm(&expected));
                assert_eq!(capture(&subject), capture(&control));
                let actual = subject.shared_framebuffer().load_full().unwrap();
                let expected = control.shared_framebuffer().load_full().unwrap();
                assert!(!actual.is_empty());
                assert_eq!(actual.dimensions(), expected.dimensions());
                assert_eq!(actual.as_slice(), expected.as_slice());
            }
            subject.shutdown();
            control.shutdown();
        }
    }
}
