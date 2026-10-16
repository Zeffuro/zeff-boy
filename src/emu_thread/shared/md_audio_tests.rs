use super::*;
use crate::emu_backend::{ActiveSystem, BackendLoadConfig, load_backend_from_rom_source};
use crate::emu_thread::{AudioConfig, RenderSettings, ReusableBuffers, SnapshotRequest};
use std::path::Path;
use zeff_md_core::Region;

fn backend(region: Region, rate: u32, fault: bool) -> EmuBackend {
    let mut rom = vec![0; 32 * 1024];
    rom[..4].copy_from_slice(&0x00ff_fffc_u32.to_be_bytes());
    rom[4..8].copy_from_slice(&0x200_u32.to_be_bytes());
    rom[0x100..0x110].copy_from_slice(b"SEGA MEGA DRIVE ");
    let mut words = Vec::<u16>::new();
    for (address, value) in [(0xc00011_u32, 0x80), (0xc00011, 0x04), (0xc00011, 0x90)] {
        words.extend([0x13fc, value, (address >> 16) as u16, address as u16]);
    }
    if fault {
        words.extend([0x3039, 0x0040, 0x0000]);
    }
    words.extend([0x60fe, 0x4e71]);
    for (index, word) in words.into_iter().enumerate() {
        rom[0x200 + index * 2..0x202 + index * 2].copy_from_slice(&word.to_be_bytes());
    }
    let path = Path::new("catchup.md");
    load_backend_from_rom_source(
        ActiveSystem::Md,
        path,
        path,
        Some(rom),
        BackendLoadConfig {
            md_region: Some(region),
            sample_rate: Some(rate),
            ..Default::default()
        },
    )
    .unwrap()
    .backend
}

fn input(frames: usize, skip_audio: bool) -> FrameInput {
    FrameInput {
        frames,
        speculation_blockers: super::super::SpeculationBlockers::feature_disabled(),
        replay_joypad_frames: None,
        host_tilt: (0.0, 0.0),
        host_camera_frame: None,
        joypad: super::super::JoypadInput {
            buttons: 0,
            dpad: 0,
            buttons_p2: 0,
            dpad_p2: 0,
            buttons_p3: 0,
            dpad_p3: 0,
            buttons_p4: 0,
            dpad_p4: 0,
            buttons_p5: 0,
            dpad_p5: 0,
        },
        pce_mouse: Default::default(),
        zapper: Default::default(),
        debug_step: false,
        debug_continue: false,
        debug_suspend_after_frame: false,
        audio: AudioConfig {
            apu_capture_enabled: false,
            skip_audio,
            playback_speed: 1,
            recording_capture: Default::default(),
        },
        debug_actions: crate::debug::DebugUiActions::none(),
        snapshot: SnapshotRequest {
            want_debug_info: false,
            want_perf_info: false,
            any_viewer_open: false,
            any_vram_viewer_open: false,
            show_oam_viewer: false,
            show_apu_viewer: false,
            show_disassembler: false,
            show_rom_info: false,
            show_memory_viewer: false,
            memory_view_start: 0,
            show_rom_viewer: false,
            show_instruction_trace: false,
            trace_after_sequence: None,
            rom_view_start: 0,
            last_disasm_pc: None,
            last_disasm_mapping: None,
            disasm_target: None,
            memory_search: None,
            rom_search: None,
            render: RenderSettings {
                color_correction: crate::settings::ColorCorrection::None,
                color_correction_matrix: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
                dmg_palette_preset: Default::default(),
                nes_palette_mode: Default::default(),
                nes_custom_palette: None,
                pce_overscan_mode: Default::default(),
                pce_palette_mode: Default::default(),
                sgb_border_enabled: false,
            },
        },
        buffers: ReusableBuffers {
            audio: Some(vec![99.0; 100]),
            vram: None,
            oam: None,
            memory_page: None,
            nes_chr: None,
            nes_nametable: None,
        },
        rewind_enabled: false,
        rewind_seconds: 10,
    }
}

fn step(
    backend: &mut EmuBackend,
    frames: usize,
    skip_audio: bool,
    native_path: bool,
) -> FrameResult {
    let mut rewind = zeff_emu_common::rewind::RewindBuffer::new(0, 1);
    let mut seconds = 10;
    let mut fault = WorkerRuntimeFault::default();
    #[cfg(not(target_arch = "wasm32"))]
    if native_path {
        return EmuThread::handle_step_frames_with_tcp_link(
            backend,
            input(frames, skip_audio),
            &[],
            None,
            false,
            &mut rewind,
            &mut seconds,
            &mut fault,
        );
    }
    let _ = native_path;
    EmuThread::handle_step_frames(
        backend,
        input(frames, skip_audio),
        &[],
        false,
        &mut rewind,
        &mut seconds,
        &mut fault,
    )
}

fn compare_batches(region: Region, rate: u32, smallest_overflow: usize) {
    for native_path in [false, true] {
        for frames in [1, smallest_overflow - 1, smallest_overflow, 10] {
            let mut batched = backend(region, rate, false);
            let mut single = backend(region, rate, false);
            let result = step(&mut batched, frames, false, native_path);
            let mut expected = Vec::new();
            for _ in 0..frames {
                expected.extend(step(&mut single, 1, false, native_path).audio_samples);
            }
            assert_eq!(result.advanced_frames, frames);
            assert!(result.runtime_fault.is_none());
            assert!(expected.iter().any(|sample| *sample != 0.0));
            if frames == smallest_overflow {
                assert!(expected.len() > 8192);
            }
            assert_eq!(
                result.audio_samples.len(),
                expected.len(),
                "{region:?} {rate} n={frames} native={native_path}"
            );
            assert!(
                result
                    .audio_samples
                    .iter()
                    .zip(&expected)
                    .all(|(actual, expected)| actual.to_bits() == expected.to_bits())
            );
            assert!(batched.encode_state_bytes().unwrap() == single.encode_state_bytes().unwrap());
        }
    }
}

#[test]
fn md_catchup_audio_ntsc_48k_matches_single_fields() {
    compare_batches(Region::NtscUs, 48_000, 6);
}
#[test]
fn md_catchup_audio_pal_48k_matches_single_fields() {
    compare_batches(Region::Pal, 48_000, 5);
}
#[test]
fn md_catchup_audio_ntsc_96k_matches_single_fields() {
    compare_batches(Region::NtscUs, 96_000, 3);
}
#[test]
fn md_catchup_audio_pal_96k_matches_single_fields() {
    compare_batches(Region::Pal, 96_000, 3);
}

#[test]
fn md_catchup_audio_remaining_output_rates_match_single_fields() {
    for (region, rate, first_overflow) in [
        (Region::NtscUs, 32_000, 8),
        (Region::Pal, 32_000, 7),
        (Region::NtscUs, 44_100, 6),
        (Region::Pal, 44_100, 5),
        (Region::NtscUs, 192_000, 2),
        (Region::Pal, 192_000, 2),
    ] {
        compare_batches(region, rate, first_overflow);
    }
}

#[test]
fn md_catchup_audio_mute_and_fault_preserve_worker_behavior() {
    for native_path in [false, true] {
        let mut muted = backend(Region::Pal, 96_000, false);
        let result = step(&mut muted, 10, true, native_path);
        assert_eq!(result.advanced_frames, 10);
        assert!(result.audio_samples.is_empty());
        let mut batched = backend(Region::NtscUs, 48_000, true);
        let mut single = backend(Region::NtscUs, 48_000, true);
        let result = step(&mut batched, 10, false, native_path);
        let expected = step(&mut single, 1, false, native_path);
        assert!(result.runtime_fault.is_some());
        assert_eq!(result.runtime_fault, expected.runtime_fault);
        assert_eq!(result.advanced_frames, expected.advanced_frames);
        assert_eq!(result.audio_samples, expected.audio_samples);
        assert_eq!(batched.frame_count(), single.frame_count());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn md_catchup_audio_recovery_reaches_native_callback_preroll() {
    use crate::audio::{AudioOutput, AudioQueueConfig};
    use crate::settings::AudioBufferPolicy;
    let config = AudioQueueConfig {
        master_volume: 1.0,
        playback_speed: 1,
        mute_during_fast_forward: false,
        low_pass_enabled: false,
        low_pass_cutoff_hz: 20_000,
    };
    for (region, frames) in [(Region::NtscUs, 6), (Region::Pal, 5)] {
        let mut backend = backend(region, 48_000, false);
        let pcm = step(&mut backend, frames, false, true).audio_samples;
        for complete in [false, true] {
            let (mut output, mut driver) = AudioOutput::test_output(AudioBufferPolicy::Auto);
            assert!(pcm.len() >= driver.preroll());
            output.queue_samples(&pcm, &config);
            assert!(driver.render(pcm.len()).iter().any(|sample| *sample != 0.0));
            assert!(driver.render(1024).iter().all(|sample| *sample == 0.0));
            assert_eq!(driver.underruns(), 1);
            let recovery = if complete {
                &pcm[..]
            } else {
                &pcm[pcm.len() - 8192..]
            };
            output.queue_samples(recovery, &config);
            assert_eq!(
                driver.render(1024).iter().any(|sample| *sample != 0.0),
                complete
            );
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn md_worker_read_only_inspection_preserves_state_and_future_audio() {
    for region in [Region::NtscUs, Region::NtscJapan, Region::Pal] {
        for native_path in [false, true] {
            let mut inspected = backend(region, 48_000, false);
            let mut control = backend(region, 48_000, false);
            let mut request = input(1, false);
            request.snapshot.show_disassembler = true;
            request.snapshot.show_apu_viewer = true;
            request.snapshot.any_viewer_open = true;
            request.snapshot.any_vram_viewer_open = true;
            let mut rewind = zeff_emu_common::rewind::RewindBuffer::new(0, 1);
            let mut seconds = 10;
            let mut fault = WorkerRuntimeFault::default();
            #[cfg(not(target_arch = "wasm32"))]
            let result = if native_path {
                EmuThread::handle_step_frames_with_tcp_link(
                    &mut inspected,
                    request,
                    &[],
                    None,
                    false,
                    &mut rewind,
                    &mut seconds,
                    &mut fault,
                )
            } else {
                EmuThread::handle_step_frames(
                    &mut inspected,
                    request,
                    &[],
                    false,
                    &mut rewind,
                    &mut seconds,
                    &mut fault,
                )
            };
            #[cfg(target_arch = "wasm32")]
            let result = EmuThread::handle_step_frames(
                &mut inspected,
                request,
                &[],
                false,
                &mut rewind,
                &mut seconds,
                &mut fault,
            );
            let expected = step(&mut control, 1, false, native_path);
            let view = result.ui_data.disassembly_view.unwrap();
            assert_eq!(view.lines.len(), 26);
            assert_eq!(view.lines[0].address, view.pc);
            assert!(expected.ui_data.disassembly_view.is_none());
            let apu = result.ui_data.apu_debug.unwrap();
            assert!(apu.channels.is_empty() && apu.master_waveform.is_empty());
            assert_eq!(apu.extra_sections[0].heading, "PSG");
            assert_eq!(apu.extra_sections[1].heading, "OPN2");
            assert!(expected.ui_data.apu_debug.is_none());
            assert!(result.ui_data.palette_debug.is_some());
            let Some(crate::debug::ConsoleGraphicsData::Md(graphics)) =
                result.ui_data.graphics_data
            else {
                panic!("expected Mega Drive graphics");
            };
            assert_eq!(graphics.vram.len(), 65536);
            assert_eq!(graphics.cram, vec![0; 128]);
            assert_eq!(graphics.vsram, vec![0; 80]);
            assert!(expected.ui_data.palette_debug.is_none());
            assert!(expected.ui_data.graphics_data.is_none());
            assert_eq!(result.audio_samples, expected.audio_samples);
            assert_eq!(
                inspected.encode_state_bytes().unwrap(),
                control.encode_state_bytes().unwrap()
            );
            let future = step(&mut inspected, 2, false, native_path);
            let expected = step(&mut control, 2, false, native_path);
            assert!(future.runtime_fault.is_none());
            assert!(future.audio_samples.iter().any(|sample| *sample != 0.0));
            assert_eq!(future.audio_samples, expected.audio_samples);
            assert_eq!(inspected.framebuffer(), control.framebuffer());
            assert_eq!(
                inspected.encode_state_bytes().unwrap(),
                control.encode_state_bytes().unwrap()
            );
        }
    }
}
