use std::sync::Arc;

use crate::emu_backend::EmuBackend;
use crate::ui;

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

        if let Some(mutes) = &input.debug_actions.apu_channel_mutes {
            backend.set_apu_channel_mutes(mutes);
        }

        if input.frames > 0 && backend.is_running() {
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

        Self::build_frame_result(
            backend,
            shared_fb,
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
            Ok(bytes) => match crate::save_paths::write_state_bytes_to_file(path, &bytes) {
                Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
            },
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
            Self::install_rom_patches(backend, cheats);
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
                let fb = backend.framebuffer().to_vec();
                shared_fb.store(Some(Arc::new(fb)));
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
                    shared_fb.store(Some(Arc::new(fb)));
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
        if let Some(gb) = backend.gb_mut() {
            Self::apply_debug_actions(&mut gb.emu, &input.debug_actions);
            gb.emu
                .set_mbc7_host_tilt(input.host_tilt.0, input.host_tilt.1);
            gb.emu
                .set_dmg_palette_preset(input.snapshot.render.dmg_palette_preset);
            gb.emu
                .set_sgb_border_enabled(input.snapshot.render.sgb_border_enabled);
            if let Some(ref frame) = input.host_camera_frame {
                gb.emu.set_camera_host_frame(frame);
            }
            gb.emu
                .set_apu_debug_capture_enabled(input.audio.apu_capture_enabled);
            if !uncapped_mode {
                gb.emu
                    .set_apu_sample_generation_enabled(!input.audio.skip_audio);
            }
            gb.emu
                .set_opcode_log_enabled(input.snapshot.want_debug_info);

            if gb.emu.is_cpu_suspended() {
                if input.debug_continue {
                    gb.emu.debug_continue();
                } else if input.debug_step {
                    gb.emu.debug_step();
                }
            }
        }

        if let Some(nes) = backend.nes_mut() {
            Self::apply_nes_debug_actions(&mut nes.emu, &input.debug_actions);
            nes.emu
                .set_palette_mode(input.snapshot.render.nes_palette_mode);
            nes.emu
                .set_apu_debug_collection_enabled(input.audio.apu_capture_enabled);
            if nes.emu.is_cpu_suspended() {
                if input.debug_continue {
                    nes.emu.debug_continue();
                } else if input.debug_step {
                    nes.emu.debug_step();
                }
            }
        }
    }

    pub(crate) fn step_n_frames(
        backend: &mut EmuBackend,
        n: usize,
        cheats: &[crate::cheats::CheatPatch],
    ) {
        for _ in 0..n {
            backend.step_frame();
            if let Some(gb) = backend.gb_mut() {
                Self::apply_ram_cheats(&mut gb.emu, cheats);
            }
            if let Some(nes) = backend.nes_mut() {
                Self::apply_nes_ram_cheats(&mut nes.emu, cheats);
            }
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
        match backend {
            EmuBackend::Gb(gb) => ui::collect_emu_snapshot(
                &gb.emu,
                snapshot,
                buffers.vram,
                buffers.oam,
                buffers.memory_page,
            ),
            EmuBackend::Nes(nes) => ui::collect_nes_snapshot(
                &mut nes.emu,
                snapshot,
                buffers.nes_chr,
                buffers.nes_nametable,
                buffers.memory_page,
            ),
        }
    }

    pub(crate) fn build_frame_result(
        backend: &mut EmuBackend,
        shared_fb: &SharedFramebuffer,
        reusable_audio: Option<Vec<f32>>,
        ui_data: ui::UiFrameData,
        midi_capture_active: bool,
        rewind_fill: f32,
    ) -> FrameResult {
        let src = backend.framebuffer();
        shared_fb.store(Some(Arc::new(src.to_vec())));

        let rumble = backend.rumble_active();
        let mut audio_samples = reusable_audio.unwrap_or_default();
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
