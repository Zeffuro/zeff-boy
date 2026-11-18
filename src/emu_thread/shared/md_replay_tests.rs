use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use std::path::Path;
use zeff_emu_common::replay::{ReplayLoadLimits, ReplayPlayer, ReplayRecorder};
use zeff_md_core::{ControllerKind, TimingModel};

pub(super) fn backend(header: u8, controller: ControllerKind, timing: TimingModel) -> EmuBackend {
    let mut rom = vec![0; 0x400];
    rom[..8].copy_from_slice(&[0, 0xff, 0xff, 0xfc, 0, 0, 2, 0]);
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    rom[0x1f0] = header;
    let words = [
        0x13fc_u16, 0x85, 0xc0, 0x11, 0x13fc, 6, 0xc0, 0x11, 0x13fc, 0x90, 0xc0, 0x11, 0x5239,
        0xff, 0x40, 0x60f8,
    ];
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = Path::new("replay.md");
    load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            sample_rate: Some(48_000),
            md_controller: controller,
            md_timing: timing,
            md_load_save_ram: false,
            md_host_persistence: false,
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_replay_codec_and_multifield_worker_match_twelve_configurations() {
    let frames = [
        ReplayJoypadFrame {
            buttons: 0x41,
            dpad: 4,
            buttons_p2: 0x82,
            dpad_p2: 1,
            ..Default::default()
        },
        ReplayJoypadFrame {
            buttons: 0x34,
            dpad: 2,
            buttons_p2: 0x88,
            dpad_p2: 8,
            ..Default::default()
        },
        ReplayJoypadFrame {
            buttons: 0xff,
            buttons_p2: 1,
            ..Default::default()
        },
        ReplayJoypadFrame::default(),
    ];
    let mut count = 0;
    for header in *b"UJE" {
        for controller in [ControllerKind::ThreeButton, ControllerKind::SixButton] {
            for timing in [TimingModel::Standard, TimingModel::Precise] {
                let mut control = backend(header, controller, timing);
                assert!(control.supports_replay());
                let cold_origin = control.frame_count();
                control.step_frame();
                control.step_frame();
                let origin = control.frame_count();
                assert_eq!(origin - cold_origin, 2);
                assert!(origin > 0);
                let start = control.encode_replay_start_state_bytes().unwrap();
                let mut recorder = ReplayRecorder::new_with_metadata(
                    "unused.zrpl".into(),
                    start.clone(),
                    control.replay_metadata(),
                );
                for frame in &frames {
                    recorder.record_joypad_frame(frame.clone());
                    control.apply_replay_input(frame);
                    control.step_frame();
                }
                let expected_state = control.encode_replay_hash_state_bytes().unwrap();
                recorder.set_final_state_sha256(zeff_firmware::sha256_bytes(&expected_state));
                let bytes = recorder.into_bytes().unwrap();
                let player = ReplayPlayer::decode_bounded(
                    &bytes,
                    ReplayLoadLimits {
                        max_file_bytes: 64 * 1024 * 1024,
                        max_metadata_bytes: 4096,
                        max_state_bytes: 16 * 1024 * 1024,
                        max_frames: 16,
                        max_decoded_camera_bytes: 0,
                    },
                )
                .unwrap();
                assert_eq!(player.save_state(), start);
                assert_eq!(player.peek_joypad_frames(0, 4), frames);
                let mut first_pcm = None;
                for _ in 0..2 {
                    let mut target = backend(header, controller, timing);
                    for _ in 0..5 {
                        target.step_frame();
                    }
                    crate::replay_execution::validate_replay_playback(&player, &target).unwrap();
                    target
                        .load_state_from_bytes(player.save_state().to_vec())
                        .unwrap();
                    let mut input = md_audio_tests::input(4, false);
                    input.replay_joypad_frames = Some(player.peek_joypad_frames(0, 4));
                    let result = EmuThread::handle_step_frames(
                        &mut target,
                        input,
                        &[],
                        false,
                        &mut zeff_emu_common::rewind::RewindBuffer::new(0, 1),
                        &mut 0,
                        &mut WorkerRuntimeFault::default(),
                    );
                    assert!(result.runtime_fault.is_none() && result.replay_error.is_none());
                    assert_eq!((result.advanced_frames, result.staged_input_frames), (4, 4));
                    assert_eq!(target.frame_count() - origin, frames.len() as u64);
                    assert_eq!(
                        target.encode_replay_hash_state_bytes().unwrap(),
                        expected_state
                    );
                    assert_eq!(target.framebuffer(), control.framebuffer());
                    assert!(result.audio_samples.iter().any(|sample| *sample != 0.0));
                    if let Some(pcm) = &first_pcm {
                        assert_eq!(&result.audio_samples, pcm);
                    } else {
                        first_pcm = Some(result.audio_samples);
                    }
                }
                count += 1;
            }
        }
    }
    assert_eq!(count, 12);
}
