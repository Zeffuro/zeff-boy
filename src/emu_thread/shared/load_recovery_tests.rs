use super::*;
use crate::emu_thread::{EmuCommand, EmuResponse, EmuThread};
use zeff_md_core::Region;

mod guest_calls;
mod md_dma_transport;
mod memory_editing;

fn backend() -> EmuBackend {
    super::rewind_tests::backend_with_program(
        Region::NtscUs,
        48_000,
        &[
            0x1039, 0x00a1, 0x0003, 0x0800, 0x0004, 0x66f4, 0x3039, 0x0040, 0x0000,
        ],
    )
}

fn input(frames: usize, buttons: u8, dpad: u8) -> FrameInput {
    let mut input = super::md_audio_tests::input(frames, false);
    input.joypad.buttons = buttons;
    input.joypad.dpad = dpad;
    input.rewind_enabled = true;
    input
}

fn machine(backend: &EmuBackend) -> zeff_md_core::Emulator {
    let EmuBackend::Md(md) = backend else {
        panic!("expected MD");
    };
    md.core().clone()
}

fn finalize(
    backend: &mut EmuBackend,
    history: &mut zeff_emu_common::rewind::RewindBuffer,
    fault: &mut WorkerRuntimeFault,
    response: &EmuResponse,
    published_duration: &mut Option<u64>,
) -> bool {
    (crate::emu_thread::commands::LoadFinalizationContext {
        rewind_buffer: history,
        rewind_seconds: 10,
        backend,
        cheats: &[],
        audio_recording_capture: Default::default(),
        pending_audio_discontinuities: &mut Vec::new(),
        runtime_fault: fault,
    })
    .finalize(response, |duration| *published_duration = Some(duration))
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_load_recovery_common_finalizer_preserves_failure_and_clears_success() {
    let mut subject = backend();
    let mut history = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
    let mut fault = WorkerRuntimeFault::default();
    let healthy = EmuThread::handle_step_frames(
        &mut subject,
        input(4, 0, 0),
        &[],
        false,
        &mut history,
        &mut 10,
        &mut fault,
    );
    assert_eq!(healthy.advanced_frames, 4);
    assert!(healthy.runtime_fault.is_none());
    let saved = subject.encode_state_bytes().unwrap();
    let failed = EmuThread::handle_step_frames(
        &mut subject,
        input(1, 1, 0),
        &[],
        false,
        &mut history,
        &mut 10,
        &mut fault,
    );
    assert!(failed.runtime_fault.is_some());
    assert!(!fault.can_step() && subject.is_suspended());
    assert!(fault.take_pending_delivery().is_none());
    let before = machine(&subject);
    let previous = history.peek().unwrap().state_bytes;
    let history_len = history.len();
    let fb: SharedFramebuffer = Default::default();
    publish_backend_framebuffer(&fb, &subject);
    let published = fb.load_full().unwrap();
    let result = subject.load_state_from_bytes(vec![0]);
    let response =
        EmuThread::respond_load_state(&mut subject, result, "bad".into(), Some((0, 0)), &fb);
    assert!(matches!(response, EmuResponse::LoadStateFailed(_)));
    let mut duration = None;
    assert!(!finalize(
        &mut subject,
        &mut history,
        &mut fault,
        &response,
        &mut duration
    ));
    assert!(duration.is_none());
    assert_eq!(machine(&subject), before);
    assert_eq!(history.len(), history_len);
    assert_eq!(history.peek().unwrap().state_bytes, previous);
    assert!(std::sync::Arc::ptr_eq(&published, &fb.load_full().unwrap()));
    assert!(!fault.can_step() && subject.is_suspended());
    let result = subject.load_state_from_bytes(saved);
    let response =
        EmuThread::respond_load_state(&mut subject, result, "good".into(), Some((0, 0)), &fb);
    assert!(matches!(response, EmuResponse::LoadStateOk { .. }));
    assert!(finalize(
        &mut subject,
        &mut history,
        &mut fault,
        &response,
        &mut duration
    ));
    assert!(
        fault.can_step(),
        "successful load must clear the worker fault latch"
    );
    assert!(fault.take_pending_delivery().is_none());
    assert!(!subject.is_suspended());
    assert!(subject.take_runtime_fault().is_none());
    assert!(history.is_empty());
    assert_eq!(duration, Some(subject.nominal_frame_duration_ns()));
}

fn load(thread: &EmuThread, state_bytes: Vec<u8>) -> EmuResponse {
    load_with_input(thread, state_bytes, (0, 0), false)
}

fn load_with_input(
    thread: &EmuThread,
    state_bytes: Vec<u8>,
    input: (u8, u8),
    replay: bool,
) -> EmuResponse {
    thread.send(EmuCommand::LoadStateBytes {
        state_bytes,
        buttons_pressed: input.0,
        dpad_pressed: input.1,
        replay_events: replay.then(Vec::new),
        game_boy_link_start_state: None,
        game_boy_link_coordinator_start_state: None,
        game_boy_link_start_tick: None,
        wonder_swan_link_start_tick: None,
    });
    receive_response(thread)
}

fn step(thread: &EmuThread, frames: usize, buttons: u8, dpad: u8) -> FrameResult {
    thread.send(EmuCommand::StepFrames(Box::new(input(
        frames, buttons, dpad,
    ))));
    receive_frame(thread)
}

pub(super) fn capture(thread: &EmuThread) -> Vec<u8> {
    thread.send(EmuCommand::CaptureStateBytes);
    let EmuResponse::StateCaptured(bytes) = receive_response(thread) else {
        panic!("missing state capture");
    };
    assert_eq!(&bytes[..8], b"ZMDRAW01");
    assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 1);
    bytes
}

fn pcm(frame: &FrameResult) -> Vec<u32> {
    frame
        .audio_samples
        .iter()
        .map(|sample| sample.to_bits())
        .collect()
}

fn field_count(bytes: &[u8]) -> u64 {
    let mut restored = backend();
    restored.load_state_from_bytes(bytes.to_vec()).unwrap();
    restored.frame_count()
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_load_recovery_bytes_transport_advances_exact_future_pcm_and_fields() {
    let subject = EmuThread::spawn(backend(), false);
    let control = EmuThread::spawn(backend(), false);
    let healthy = step(&subject, 4, 0, 0);
    assert_eq!(healthy.advanced_frames, 4);
    assert!(healthy.runtime_fault.is_none());
    assert!(!pcm(&healthy).is_empty());
    let saved = capture(&subject);
    assert_eq!(field_count(&saved), 4);
    assert!(matches!(
        load(&control, saved.clone()),
        EmuResponse::LoadStateOk { .. }
    ));
    let fault = step(&subject, 1, 1, 0);
    assert!(
        fault.runtime_fault.is_some(),
        "the guest unsupported bus read must fault"
    );
    let inert = step(&subject, 1, 0, 0);
    assert_eq!(inert.advanced_frames, 0);
    assert!(inert.runtime_fault.is_none());
    assert!(inert.audio_samples.is_empty());
    for _ in 0..2 {
        subject.send(EmuCommand::CaptureStateBytes);
        assert!(matches!(
            receive_response(&subject),
            EmuResponse::StateCaptureFailed(_)
        ));
    }
    let published = subject.shared_framebuffer().load_full().unwrap();
    assert!(matches!(
        load(&subject, vec![0]),
        EmuResponse::LoadStateFailed(_)
    ));
    assert!(std::sync::Arc::ptr_eq(
        &published,
        &subject.shared_framebuffer().load_full().unwrap()
    ));
    let inert = step(&subject, 2, 0, 0);
    assert_eq!(inert.advanced_frames, 0);
    assert!(inert.runtime_fault.is_none());
    assert!(inert.audio_samples.is_empty());
    assert!(matches!(
        load(&subject, saved),
        EmuResponse::LoadStateOk { .. }
    ));
    assert_eq!(capture(&subject), capture(&control));
    for (index, (buttons, dpad)) in [(0, 1), (0x40, 2)].into_iter().enumerate() {
        let actual = step(&subject, 1, buttons, dpad);
        let expected = step(&control, 1, buttons, dpad);
        assert_eq!(
            actual.advanced_frames, 1,
            "successful load must resume stepping"
        );
        assert_eq!(expected.advanced_frames, 1);
        assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
        assert!(!actual.audio_samples.is_empty());
        assert_eq!(pcm(&actual), pcm(&expected));
        let actual_state = capture(&subject);
        assert_eq!(actual_state, capture(&control));
        assert_eq!(field_count(&actual_state), 5 + index as u64);
        let actual = subject.shared_framebuffer().load_full().unwrap();
        let expected = control.shared_framebuffer().load_full().unwrap();
        assert_eq!(actual.dimensions(), expected.dimensions());
        assert_eq!(actual.as_slice(), expected.as_slice());
    }
    subject.send(EmuCommand::Rewind(1));
    assert!(matches!(
        receive_response(&subject),
        EmuResponse::RewindFailed(_)
    ));
}

pub(super) fn receive_frame(thread: &EmuThread) -> FrameResult {
    #[cfg(target_arch = "wasm32")]
    return thread.try_recv_frame().expect("missing frame");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if let Some(frame) = thread.try_recv_frame() {
                return frame;
            }
            assert!(std::time::Instant::now() < until, "frame timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

pub(super) fn receive_response(thread: &EmuThread) -> EmuResponse {
    #[cfg(target_arch = "wasm32")]
    return thread.try_recv_response().expect("missing response");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(60);
        loop {
            if let crate::emu_thread::EmuResponsePoll::Response(response) = thread.poll_response() {
                return *response;
            }
            assert!(std::time::Instant::now() < until, "response timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_replay_command_restore_preserves_origin_while_ordinary_load_projects_live_input() {
    let mut source = backend();
    source.set_input(0x74, 6);
    source.set_input_p2(0x8a, 9);
    source.step_frame();
    assert!(!source.is_suspended());
    let origin = source.encode_state_bytes().unwrap();
    let md = machine(&source);
    assert_ne!(
        md.input().controller_raw(zeff_md_core::ControllerPort::One),
        0xff
    );
    assert_ne!(
        md.input().controller_raw(zeff_md_core::ControllerPort::Two),
        0xff
    );
    assert_eq!(
        md.input()
            .controller_extra_raw(zeff_md_core::ControllerPort::One),
        2
    );
    assert_eq!(
        md.input()
            .controller_extra_raw(zeff_md_core::ControllerPort::Two),
        13
    );
    source.set_input(1, 1);
    let ordinary = source.encode_state_bytes().unwrap();
    assert_ne!(origin, ordinary);
    let subject = EmuThread::spawn(backend(), false);
    assert!(matches!(
        load_with_input(&subject, origin.clone(), (1, 1), true),
        EmuResponse::LoadStateOk { .. }
    ));
    assert_eq!(capture(&subject), origin);
    let frame = subject.shared_framebuffer().load_full().unwrap();
    for replay in [false, true] {
        assert!(matches!(
            load_with_input(&subject, vec![0], (0, 0), replay),
            EmuResponse::LoadStateFailed(_)
        ));
        assert_eq!(capture(&subject), origin);
        assert!(std::sync::Arc::ptr_eq(
            &frame,
            &subject.shared_framebuffer().load_full().unwrap()
        ));
    }
    assert!(matches!(
        load_with_input(&subject, origin.clone(), (1, 1), false),
        EmuResponse::LoadStateOk { .. }
    ));
    assert_eq!(capture(&subject), ordinary);
    assert!(matches!(
        load_with_input(&subject, origin.clone(), (0, 0), true),
        EmuResponse::LoadStateOk { .. }
    ));
    assert_eq!(capture(&subject), origin);

    let sega8 = || {
        let emulator = zeff_sega8_core::emulator::Emulator::new_with_hint(
            &[0],
            44_100,
            zeff_sega8_core::hardware::cartridge::SystemHint::MasterSystem,
        )
        .unwrap();
        EmuBackend::from_sega8(emulator, std::path::PathBuf::from("restore-policy.sms"))
    };
    let mut source = sega8();
    source.set_input(0x40, 4);
    let origin = source.encode_state_bytes().unwrap();
    source.set_input(1, 1);
    let expected = source.encode_state_bytes().unwrap();
    assert_ne!(origin, expected);
    let subject = EmuThread::spawn(sega8(), false);
    assert!(matches!(
        load_with_input(&subject, origin, (1, 1), true),
        EmuResponse::LoadStateOk { .. }
    ));
    subject.send(EmuCommand::CaptureStateBytes);
    let EmuResponse::StateCaptured(actual) = receive_response(&subject) else {
        panic!("state capture failed");
    };
    assert_eq!(
        actual, expected,
        "other-core replay must retain its existing live-input projection"
    );
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_replay_command_rejects_timing_switch_while_ordinary_state_migrates() {
    use zeff_md_core::{ControllerKind, TimingModel};
    let build = |timing| super::md_replay_tests::backend(b'U', ControllerKind::SixButton, timing);
    for (source_model, target_model) in [
        (TimingModel::Standard, TimingModel::Precise),
        (TimingModel::Precise, TimingModel::Standard),
    ] {
        let mut source = build(source_model);
        source.set_input(0x74, 6);
        source.set_input_p2(0x8a, 9);
        let origin = source.encode_state_bytes().unwrap();
        let held_target = || {
            let mut target = build(target_model);
            target.set_input(0x41, 4);
            target.set_input_p2(0x82, 1);
            target
        };
        let target = held_target();
        let before = target.encode_state_bytes().unwrap();
        let subject = EmuThread::spawn(target, false);
        let frame = subject.shared_framebuffer().load_full();
        let response = load_with_input(&subject, origin.clone(), (1, 1), true);
        let EmuResponse::LoadStateFailed(error) = response else {
            panic!("mismatched replay timing was accepted");
        };
        assert!(error.contains("replay timing mismatch"), "{error}");
        assert_eq!(capture(&subject), before);
        assert_eq!(subject.shared_framebuffer().load_full(), frame);
        let control = EmuThread::spawn(held_target(), false);
        let actual = step(&subject, 1, 0, 0);
        let expected = step(&control, 1, 0, 0);
        assert_eq!((actual.advanced_frames, expected.advanced_frames), (1, 1));
        assert!(actual.runtime_fault.is_none() && expected.runtime_fault.is_none());
        assert_eq!(pcm(&actual), pcm(&expected));
        assert_eq!(capture(&subject), capture(&control));

        let mut migrated = build(target_model);
        migrated.load_state_from_bytes(origin.clone()).unwrap();
        assert_eq!(machine(&migrated).timing_model(), source_model);
        migrated.set_input(1, 1);
        let expected = migrated.encode_state_bytes().unwrap();
        assert!(matches!(
            load_with_input(&subject, origin.clone(), (1, 1), false),
            EmuResponse::LoadStateOk { .. }
        ));
        assert_eq!(capture(&subject), expected);
        assert!(matches!(
            load_with_input(&subject, origin.clone(), (1, 1), true),
            EmuResponse::LoadStateOk { .. }
        ));
        assert_eq!(capture(&subject), origin);
    }
}
