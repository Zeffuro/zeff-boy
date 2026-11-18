use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::EmuCommand;
use std::path::Path;
use zeff_md_core::Region;

fn backend(region: Region, rate: u32, fault: bool) -> EmuBackend {
    backend_with_program(
        region,
        rate,
        if fault {
            &[0x3039, 0x0040, 0x0000]
        } else {
            &[]
        },
    )
}

pub(super) fn backend_with_program(region: Region, rate: u32, program: &[u16]) -> EmuBackend {
    let mut bytes = vec![0; 32 * 1024];
    bytes[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    bytes[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    let mut words = Vec::new();
    for (address, value) in [
        (0xc00011_u32, 0x80),
        (0xc00011, 0x04),
        (0xc00011, 0x90),
        (0xa11100, 1),
        (0xa11200, 1),
    ] {
        words.extend([0x13fc, value, (address >> 16) as u16, address as u16]);
    }
    for (register, value) in [
        (0x30_u16, 1),
        (0x34, 1),
        (0x38, 1),
        (0x3c, 1),
        (0x40, 0),
        (0x44, 0),
        (0x48, 0),
        (0x4c, 0),
        (0x50, 0x1f),
        (0x54, 0x1f),
        (0x58, 0x1f),
        (0x5c, 0x1f),
        (0x80, 0x0f),
        (0x84, 0x0f),
        (0x88, 0x0f),
        (0x8c, 0x0f),
        (0xb0, 7),
        (0xb4, 0xc0),
        (0xa4, 0x22),
        (0xa0, 0x69),
        (0x28, 0xf0),
    ] {
        for (address, value) in [(0xa04000_u32, register), (0xa04001, value)] {
            words.extend([0x13fc, value, (address >> 16) as u16, address as u16]);
        }
    }
    for value in [
        0x8c81_u16,
        if region == Region::Pal {
            0x810c
        } else {
            0x8104
        },
        0xc000,
        0,
    ] {
        words.extend([0x33fc, value, 0x00c0, 0x0004]);
    }
    words.extend([0x33fc, 0x000e, 0x00c0, 0x0000]);
    words.extend_from_slice(program);
    words.extend([0x60fe, 0x4e71]);
    for (index, word) in words.into_iter().enumerate() {
        bytes[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = Path::new("rewind.md");
    load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(bytes),
        BackendLoadConfig {
            md_region: Some(region),
            sample_rate: Some(rate),
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

fn input(frames: usize) -> FrameInput {
    let mut input = super::md_audio_tests::input(frames, false);
    input.rewind_enabled = true;
    input.joypad.buttons = 8;
    input
}

fn pcm(backend: &mut EmuBackend) -> Vec<u32> {
    let mut samples = Vec::new();
    backend.drain_audio_samples_into(&mut samples);
    samples.into_iter().map(f32::to_bits).collect()
}

fn step(
    backend: &mut EmuBackend,
    rewind: &mut zeff_emu_common::rewind::RewindBuffer,
    frames: usize,
) -> FrameResult {
    EmuThread::handle_step_frames(
        backend,
        input(frames),
        &[],
        false,
        rewind,
        &mut 10,
        &mut WorkerRuntimeFault::default(),
    )
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_rewind_preserves_queued_and_drained_future_pcm_and_fields() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for rate in [44_100, 48_000] {
            for drained in [false, true] {
                let mut subject = backend(region, rate, false);
                for _ in 0..3 {
                    subject.step_frame();
                }
                assert!(!subject.is_suspended());
                if drained {
                    assert!(!pcm(&mut subject).is_empty());
                }
                let saved = subject.encode_rewind_checkpoint().unwrap();
                assert!(saved.starts_with(b"ZMDDBG01"));
                assert!(
                    subject
                        .encode_state_bytes()
                        .unwrap()
                        .starts_with(b"ZMDRAW01")
                );
                let mut control = backend(region, rate, false);
                control.restore_rewind_checkpoint(saved.clone()).unwrap();
                let mut history = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
                EmuThread::capture_rewind_snapshot(&subject, &mut history, true, 1);
                subject.set_input(0x40, 3);
                for _ in 0..2 {
                    subject.step_frame();
                }
                let fb = Default::default();
                assert!(matches!(
                    EmuThread::handle_rewind(&mut subject, &mut history, &fb, 1),
                    EmuResponse::RewindOk { .. }
                ));
                assert_eq!(subject.encode_rewind_checkpoint().unwrap(), saved);
                assert_eq!(fb.load_full().unwrap().as_slice(), subject.framebuffer());
                assert_eq!(
                    fb.load_full().unwrap().dimensions(),
                    subject.display_framebuffer().1
                );
                for buttons in [8, 0, 0x40] {
                    for emu in [&mut subject, &mut control] {
                        emu.set_input(buttons, 1);
                        emu.step_frame();
                    }
                    assert_eq!(subject.frame_count(), control.frame_count());
                    assert_eq!(
                        subject.encode_state_bytes().unwrap(),
                        control.encode_state_bytes().unwrap()
                    );
                    assert_eq!(subject.framebuffer(), control.framebuffer());
                    let expected = pcm(&mut control);
                    assert!(expected.iter().any(|bits| *bits & 0x7fff_ffff != 0));
                    assert_eq!(pcm(&mut subject), expected);
                }
            }
        }
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_rewind_worker_captures_after_delivery_for_single_and_batched_fields() {
    for batch in [1, 4] {
        let mut subject = backend(Region::NtscUs, 48_000, false);
        let mut control = backend(Region::NtscUs, 48_000, false);
        let mut history = zeff_emu_common::rewind::RewindBuffer::new(1, 4);
        let mut ignored = zeff_emu_common::rewind::RewindBuffer::new(0, 4);
        for _ in 0..4 / batch {
            assert_eq!(
                step(&mut subject, &mut history, batch).audio_samples,
                step(&mut control, &mut ignored, batch).audio_samples
            );
        }
        let cut = subject.encode_rewind_checkpoint().unwrap();
        assert_eq!(history.peek().unwrap().state_bytes, cut);
        assert!(pcm(&mut subject).is_empty());
        for _ in 0..4 / batch {
            step(&mut subject, &mut history, batch);
        }
        assert_eq!(history.len(), 2);
        assert!(matches!(
            EmuThread::handle_rewind(&mut subject, &mut history, &Default::default(), 1),
            EmuResponse::RewindOk { .. }
        ));
        assert_eq!(subject.frame_count(), 4);
        assert_eq!(subject.encode_rewind_checkpoint().unwrap(), cut);
        let expected = step(&mut control, &mut ignored, 3);
        let actual = step(&mut subject, &mut history, 3);
        assert_eq!(
            actual
                .audio_samples
                .iter()
                .map(|s| s.to_bits())
                .collect::<Vec<_>>(),
            expected
                .audio_samples
                .iter()
                .map(|s| s.to_bits())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            subject.encode_rewind_checkpoint().unwrap(),
            control.encode_rewind_checkpoint().unwrap()
        );
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_rewind_failure_keeps_primary_history_and_published_frame() {
    let mut subject = backend(Region::NtscUs, 48_000, false);
    subject.step_frame();
    let good = subject.encode_rewind_checkpoint().unwrap();
    let mut other = backend(Region::Pal, 48_000, false);
    other.step_frame();
    let mut bad_audio = good.clone();
    bad_audio.push(0);
    let wrong_rom = backend(Region::NtscUs, 48_000, true)
        .encode_rewind_checkpoint()
        .unwrap();
    let wrong_rate = backend(Region::NtscUs, 44_100, false)
        .encode_rewind_checkpoint()
        .unwrap();
    let mut muted = backend(Region::NtscUs, 48_000, false);
    muted.set_apu_channel_mutes(&[true; 10]);
    let wrong_mutes = muted.encode_rewind_checkpoint().unwrap();
    muted.set_apu_channel_mutes(&[false; 10]);
    muted.set_apu_sample_generation_enabled(false);
    let wrong_generation = muted.encode_rewind_checkpoint().unwrap();
    for bad in [
        vec![0; 12],
        good[..good.len() - 1].to_vec(),
        bad_audio,
        other.encode_rewind_checkpoint().unwrap(),
        wrong_rom,
        wrong_rate,
        wrong_mutes,
        wrong_generation,
        subject.encode_state_bytes().unwrap(),
    ] {
        let mut history = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
        history.push(&bad, &[]);
        let before = subject.encode_rewind_checkpoint().unwrap();
        let fb = Default::default();
        publish_backend_framebuffer(&fb, &subject);
        let published = fb.load_full().unwrap();
        assert!(matches!(
            EmuThread::handle_rewind(&mut subject, &mut history, &fb, 1),
            EmuResponse::RewindFailed(_)
        ));
        assert_eq!(history.peek().unwrap().state_bytes, bad);
        assert_eq!(subject.encode_rewind_checkpoint().unwrap(), before);
        assert!(std::sync::Arc::ptr_eq(&published, &fb.load_full().unwrap()));
    }
    let before = subject.encode_rewind_checkpoint().unwrap();
    assert!(matches!(
        EmuThread::handle_rewind(
            &mut subject,
            &mut zeff_emu_common::rewind::RewindBuffer::new(1, 1),
            &Default::default(),
            1
        ),
        EmuResponse::RewindFailed(_)
    ));
    assert_eq!(subject.encode_rewind_checkpoint().unwrap(), before);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_rewind_invalidates_host_audio_changes_but_keeps_identical_mutes() {
    for change in 0..4 {
        let mut subject = backend(Region::NtscUs, 48_000, false);
        let mut history = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
        step(&mut subject, &mut history, 1);
        assert_eq!(history.len(), 1);
        match change {
            0 => subject.set_sample_rate(44_100),
            1 => subject.set_apu_channel_mutes(&[true; 10]),
            2 => subject.set_apu_sample_generation_enabled(false),
            _ => subject.set_sample_rate(48_000),
        }
        let before = subject.encode_rewind_checkpoint().unwrap();
        assert!(matches!(
            EmuThread::handle_rewind(&mut subject, &mut history, &Default::default(), 1),
            EmuResponse::RewindFailed(_)
        ));
        assert!(history.is_empty());
        assert_eq!(subject.encode_rewind_checkpoint().unwrap(), before);
        step(&mut subject, &mut history, 1);
        assert_eq!(history.len(), 1);
        subject.set_apu_channel_mutes(&[change == 1; 10]);
        EmuThread::capture_rewind_snapshot(&subject, &mut history, false, 0);
        assert_eq!(history.len(), 1);
    }
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_rewind_common_command_recovers_latched_fault_and_suspend() {
    let mut subject = backend(Region::Pal, 48_000, true);
    let mut history = zeff_emu_common::rewind::RewindBuffer::new(1, 1);
    EmuThread::capture_rewind_snapshot(&subject, &mut history, true, 1);
    subject.step_frame();
    assert!(subject.is_suspended());
    let mut fault = WorkerRuntimeFault::default();
    fault.latch(subject.take_runtime_fault());
    assert!(!fault.can_step());
    let before = history.peek().unwrap().state_bytes;
    let mut corrupt = history.clone();
    corrupt.push(&[0], &[]);
    assert!(matches!(
        rewind_command(&mut subject, &mut corrupt, &mut fault),
        EmuResponse::RewindFailed(_)
    ));
    assert!(subject.is_suspended() && !fault.can_step());
    assert_eq!(corrupt.len(), 2);
    assert_eq!(history.peek().unwrap().state_bytes, before);
    assert!(matches!(
        rewind_command(&mut subject, &mut history, &mut fault),
        EmuResponse::RewindOk { .. }
    ));
    assert!(fault.can_step() && !subject.is_suspended());
    assert!(subject.take_runtime_fault().is_none());
    let duration = EmuThread::reset_rewind_for_loaded_state(&mut history, 10, &subject);
    assert_eq!(duration, subject.nominal_frame_duration_ns());
    assert_eq!(history.capacity(), 125);
    assert!(history.is_empty());
}

fn rewind_command(
    backend: &mut EmuBackend,
    history: &mut zeff_emu_common::rewind::RewindBuffer,
    fault: &mut WorkerRuntimeFault,
) -> EmuResponse {
    let result = crate::emu_thread::commands::CommonCommandContext {
        backend,
        shared_framebuffer: &Default::default(),
        uncapped_mode: &mut false,
        rewind_buffer: history,
        last_cheats: &mut Vec::new(),
        audio_recording_capture: &mut Default::default(),
        pending_audio_discontinuities: &mut Vec::new(),
        runtime_fault: fault,
    }
    .dispatch(EmuCommand::Rewind(1));
    let crate::emu_thread::commands::CommonCommandDispatch::Handled(effects) = result else {
        panic!("command not handled");
    };
    effects.response.expect("missing rewind response")
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), test)]
fn md_rewind_command_transport_resumes_identical_pcm() {
    let subject = EmuThread::spawn(backend(Region::NtscJapan, 44_100, false), false);
    let control = EmuThread::spawn(backend(Region::NtscJapan, 44_100, false), false);
    for thread in [&subject, &control] {
        thread.send(EmuCommand::StepFrames(Box::new(input(4))));
        assert_eq!(receive_frame(thread).advanced_frames, 4);
    }
    subject.send(EmuCommand::StepFrames(Box::new(input(4))));
    receive_frame(&subject);
    subject.send(EmuCommand::Rewind(1));
    assert!(matches!(
        receive_response(&subject),
        EmuResponse::RewindOk { .. }
    ));
    for thread in [&subject, &control] {
        thread.send(EmuCommand::StepFrames(Box::new(input(3))));
    }
    let actual = receive_frame(&subject);
    let expected = receive_frame(&control);
    assert_eq!(
        actual
            .audio_samples
            .iter()
            .map(|s| s.to_bits())
            .collect::<Vec<_>>(),
        expected
            .audio_samples
            .iter()
            .map(|s| s.to_bits())
            .collect::<Vec<_>>()
    );
    for thread in [&subject, &control] {
        thread.send(EmuCommand::CaptureStateBytes);
    }
    let EmuResponse::StateCaptured(actual) = receive_response(&subject) else {
        panic!("missing subject state");
    };
    let EmuResponse::StateCaptured(expected) = receive_response(&control) else {
        panic!("missing control state");
    };
    assert_eq!(actual, expected);
    assert_eq!(
        subject.shared_framebuffer().load_full().unwrap().as_slice(),
        control.shared_framebuffer().load_full().unwrap().as_slice()
    );
}

fn receive_frame(thread: &EmuThread) -> FrameResult {
    #[cfg(target_arch = "wasm32")]
    return thread.try_recv_frame().expect("missing frame");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            if let Some(frame) = thread.try_recv_frame() {
                return frame;
            }
            assert!(std::time::Instant::now() < until, "frame timed out");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

fn receive_response(thread: &EmuThread) -> EmuResponse {
    #[cfg(target_arch = "wasm32")]
    return thread.try_recv_response().expect("missing response");
    #[cfg(not(target_arch = "wasm32"))]
    {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
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
fn md_rewind_same_frame_input_change_and_cadence_rollback() {
    let mut subject = backend(Region::NtscUs, 48_000, false);
    let mut history = zeff_emu_common::rewind::RewindBuffer::new(1, 4);
    step(&mut subject, &mut history, 4);
    step(&mut subject, &mut history, 4);
    let latest = history.peek().unwrap().state_bytes;
    subject.set_input(0x40, 3);
    assert_ne!(subject.encode_rewind_checkpoint().unwrap(), latest);
    assert!(matches!(
        EmuThread::handle_rewind(&mut subject, &mut history, &Default::default(), 1),
        EmuResponse::RewindOk { .. }
    ));
    assert_eq!(subject.frame_count(), 8);
    assert_eq!(subject.encode_rewind_checkpoint().unwrap(), latest);
    assert_eq!(history.len(), 1);
    history.push(&[0], &[]);
    step(&mut subject, &mut history, 2);
    assert!(matches!(
        EmuThread::handle_rewind(&mut subject, &mut history, &Default::default(), 1),
        EmuResponse::RewindFailed(_)
    ));
    assert_eq!(history.len(), 2);
    step(&mut subject, &mut history, 1);
    assert_eq!(history.len(), 2);
    step(&mut subject, &mut history, 1);
    assert_eq!(history.len(), 3);
}
