use crate::emu_backend::{BackendRuntimeConfig, EmuBackend};
use crate::ui;

use super::types::{publish_framebuffer, publish_owned_framebuffer};
use super::{EmuResponse, EmuThread, FrameInput, FrameResult, SharedFramebuffer};

impl EmuThread {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn handle_step_frames(
        backend: &mut EmuBackend,
        mut input: FrameInput,
        cheats: &[crate::cheats::CheatPatch],
        uncapped_mode: bool,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        rewind_seconds: &mut usize,
        shared_fb: &SharedFramebuffer,
    ) -> FrameResult {
        Self::configure_system(backend, &input, uncapped_mode);

        backend.set_input(input.joypad.buttons, input.joypad.dpad);
        backend.set_input_p2(input.joypad.buttons_p2, input.joypad.dpad_p2);
        backend.set_zapper_state(
            input.zapper.enabled,
            input.zapper.trigger,
            input.zapper.hit,
            input.zapper.screen_pos,
        );

        if let Some(mutes) = &input.debug_actions.apu_channel_mutes {
            backend.set_apu_channel_mutes(mutes);
        }

        let stepped_frames = input.frames > 0 && backend.is_running();
        if stepped_frames {
            Self::step_n_frames(backend, input.frames, cheats);
        }
        if input.rewind_seconds != *rewind_seconds {
            *rewind_seconds = input.rewind_seconds;
            *rewind_buffer = zeff_emu_common::rewind::RewindBuffer::new(
                *rewind_seconds,
                super::REWIND_SNAPSHOTS_PER_SECOND,
            );
        }

        Self::capture_rewind_snapshot(backend, rewind_buffer, input.rewind_enabled);

        let midi_capture_active = input.audio.midi_capture_active;
        let reusable_audio = input.buffers.audio.take();
        let ui_data = Self::collect_ui_snapshot(backend, &input.snapshot, input.buffers);

        publish_framebuffer(shared_fb, backend.framebuffer());

        Self::build_frame_result(
            backend,
            reusable_audio,
            ui_data,
            midi_capture_active,
            rewind_buffer.fill_ratio(),
        )
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn encode_and_write_state(
        backend: &EmuBackend,
        path: &std::path::Path,
    ) -> EmuResponse {
        match backend.encode_state_bytes() {
            Ok(bytes) => {
                match crate::save_paths::write_state_bytes_to_file_with_backup(path, &bytes) {
                    Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                    Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
                }
            }
            Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn finalize_load_state(
        resp: &EmuResponse,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        backend: &mut EmuBackend,
        cheats: &[crate::cheats::CheatPatch],
    ) {
        if matches!(resp, EmuResponse::LoadStateOk { .. }) {
            rewind_buffer.clear();
            backend.install_rom_patches(cheats);
        }
    }

    pub(crate) fn respond_load_state(
        backend: &mut EmuBackend,
        result: anyhow::Result<()>,
        path_label: String,
        buttons_pressed: u8,
        dpad_pressed: u8,
        shared_fb: &SharedFramebuffer,
    ) -> EmuResponse {
        match result {
            Ok(()) => {
                backend.set_input(buttons_pressed, dpad_pressed);
                publish_framebuffer(shared_fb, backend.framebuffer());
                EmuResponse::LoadStateOk { path: path_label }
            }
            Err(err) => EmuResponse::LoadStateFailed(err.to_string()),
        }
    }

    pub(crate) fn handle_rewind(
        backend: &mut EmuBackend,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        shared_fb: &SharedFramebuffer,
    ) -> EmuResponse {
        let current_state = Self::encode_current_state(backend).ok();
        while let Some(rewind_frame) = rewind_buffer.pop() {
            if let Some(current) = current_state.as_ref()
                && rewind_frame.state_bytes == *current
                && !rewind_buffer.is_empty()
            {
                continue;
            }
            match backend.load_state_from_bytes(rewind_frame.state_bytes) {
                Ok(()) => {
                    let fb = if rewind_frame.framebuffer.is_empty() {
                        backend.framebuffer().to_vec()
                    } else {
                        rewind_frame.framebuffer
                    };
                    publish_owned_framebuffer(shared_fb, fb);
                    return EmuResponse::RewindOk;
                }
                Err(err) => {
                    log::warn!("Rewind restore failed: {}", err);
                    return EmuResponse::RewindFailed("rewind restore failed".to_string());
                }
            }
        }

        EmuResponse::RewindFailed("no rewind data".to_string())
    }

    pub(crate) fn encode_current_state(backend: &EmuBackend) -> anyhow::Result<Vec<u8>> {
        backend.encode_state_bytes()
    }

    pub(crate) fn capture_rewind_snapshot(
        backend: &EmuBackend,
        rewind_buffer: &mut zeff_emu_common::rewind::RewindBuffer,
        enabled: bool,
    ) {
        if enabled
            && rewind_buffer.tick()
            && let Ok(bytes) = Self::encode_current_state(backend)
        {
            rewind_buffer.push(&bytes, backend.framebuffer());
        }
    }

    pub(crate) fn configure_system(
        backend: &mut EmuBackend,
        input: &FrameInput,
        uncapped_mode: bool,
    ) {
        let runtime_config = BackendRuntimeConfig {
            opcode_log_enabled: input.snapshot.want_debug_info,
            debug_continue: input.debug_continue,
            debug_step: input.debug_step,
            uncapped_mode,
            apu_capture_enabled: input.audio.apu_capture_enabled,
            skip_audio: input.audio.skip_audio,
            host_tilt: input.host_tilt,
            host_camera_frame: input.host_camera_frame.as_deref(),
            dmg_palette_preset: input.snapshot.render.dmg_palette_preset,
            sgb_border_enabled: input.snapshot.render.sgb_border_enabled,
            nes_palette_mode: input.snapshot.render.nes_palette_mode,
            nes_custom_palette: input.snapshot.render.nes_custom_palette.as_ref(),
            ..BackendRuntimeConfig::new(&input.debug_actions)
        };
        backend.apply_runtime_config(runtime_config);
    }

    pub(crate) fn step_n_frames(
        backend: &mut EmuBackend,
        n: usize,
        cheats: &[crate::cheats::CheatPatch],
    ) {
        for _ in 0..n {
            backend.step_frame();
            backend.apply_ram_cheats(cheats);
            if backend.is_suspended() {
                break;
            }
        }
    }

    pub(crate) fn collect_ui_snapshot(
        backend: &mut EmuBackend,
        snapshot: &super::SnapshotRequest,
        buffers: super::ReusableBuffers,
    ) -> ui::UiFrameData {
        ui::collect_backend_snapshot(backend, snapshot, buffers)
    }

    pub(crate) fn build_frame_result(
        backend: &mut EmuBackend,
        reusable_audio: Option<Vec<f32>>,
        ui_data: ui::UiFrameData,
        midi_capture_active: bool,
        rewind_fill: f32,
    ) -> FrameResult {
        let rumble = backend.rumble_active();
        let mut audio_samples = reusable_audio.unwrap_or_default();
        audio_samples.clear();
        backend.drain_audio_samples_into(&mut audio_samples);
        let is_mbc7 = backend.is_mbc7();
        let is_pocket_camera = backend.is_pocket_camera();

        let apu_snapshot = if midi_capture_active {
            backend.apu_channel_snapshot()
        } else {
            None
        };

        FrameResult {
            rumble,
            audio_samples,
            ui_data,
            is_mbc7,
            is_pocket_camera,
            rewind_fill,
            apu_snapshot,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use zeff_ws_core::hardware::cartridge::compute_footer_checksum;

    fn minimal_ws_rom() -> Vec<u8> {
        let mut rom = vec![0xFF; 0x10000];
        let reset_vector = rom.len() - 16;
        rom[reset_vector..reset_vector + 5].copy_from_slice(&[0xEA, 0x00, 0x00, 0x00, 0xF0]);
        rom[0] = 0xF4;
        let footer = rom.len() - 10;
        rom[footer] = 0x01;
        rom[footer + 1] = 0x00;
        rom[footer + 2] = 0x23;
        rom[footer + 4] = 0x01;
        let checksum = compute_footer_checksum(&rom);
        rom[footer + 8..footer + 10].copy_from_slice(&checksum.to_le_bytes());
        rom
    }

    #[test]
    fn build_frame_result_does_not_republish_stale_recycled_audio() {
        let rom = minimal_ws_rom();
        let emu = zeff_ws_core::emulator::Emulator::from_rom_data(&rom).unwrap();
        let mut backend = EmuBackend::from_ws(emu, PathBuf::from("test.wsc"));

        let result = EmuThread::build_frame_result(
            &mut backend,
            Some(vec![0.25, -0.25, 0.5, -0.5]),
            ui::UiFrameData::default(),
            false,
            0.0,
        );

        assert!(
            result.audio_samples.is_empty(),
            "stale recycled audio samples must be cleared before draining new core audio"
        );
    }
}
