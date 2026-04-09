use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::Arc;

use zeff_emu_common::rewind::RewindBuffer;

use super::types::{self, EmuCommand, EmuResponse, FrameInput, FrameResult, SharedFramebuffer};
use super::{DEFAULT_REWIND_SECONDS, REWIND_SNAPSHOTS_PER_SECOND};
use crate::cheats::CheatPatch;
use crate::emu_backend::EmuBackend;
use crate::ui;

struct Inner {
    backend: EmuBackend,
    pending_frames: VecDeque<FrameResult>,
    pending_responses: VecDeque<EmuResponse>,
    uncapped_mode: bool,
    rewind_buffer: RewindBuffer,
    rewind_seconds: usize,
    last_cheats: Vec<CheatPatch>,
}

pub(crate) struct EmuThread {
    inner: RefCell<Inner>,
    shared_framebuffer: SharedFramebuffer,
}

impl EmuThread {
    pub(crate) fn spawn(backend: EmuBackend) -> Self {
        Self {
            inner: RefCell::new(Inner {
                backend,
                pending_frames: VecDeque::new(),
                pending_responses: VecDeque::new(),
                uncapped_mode: false,
                rewind_buffer: RewindBuffer::new(
                    DEFAULT_REWIND_SECONDS,
                    REWIND_SNAPSHOTS_PER_SECOND,
                ),
                rewind_seconds: DEFAULT_REWIND_SECONDS,
                last_cheats: Vec::new(),
            }),
            shared_framebuffer: types::new_shared_framebuffer(),
        }
    }

    pub(crate) fn shared_framebuffer(&self) -> &SharedFramebuffer {
        &self.shared_framebuffer
    }

    pub(crate) fn send(&self, cmd: EmuCommand) {
        let inner = &mut *self.inner.borrow_mut();
        let Inner {
            backend,
            pending_frames,
            pending_responses,
            uncapped_mode,
            rewind_buffer,
            rewind_seconds,
            last_cheats,
        } = inner;
        match cmd {
            EmuCommand::StepFrames(input) => {
                let result = Self::step_frames_inline(
                    backend,
                    *input,
                    *uncapped_mode,
                    last_cheats,
                    rewind_buffer,
                    rewind_seconds,
                    &self.shared_framebuffer,
                );
                pending_frames.push_back(result);
            }
            EmuCommand::SetSampleRate(rate) => {
                backend.set_sample_rate(rate);
            }
            EmuCommand::SetUncapped(on) => {
                *uncapped_mode = on;
                backend.set_apu_sample_generation_enabled(!on);
            }
            EmuCommand::SaveStateSlot(slot) => {
                let resp = Self::save_state_sync(backend, slot);
                pending_responses.push_back(resp);
            }
            EmuCommand::LoadStateSlot {
                slot,
                buttons_pressed,
                dpad_pressed,
            } => {
                let resp = Self::load_state_sync(
                    backend,
                    slot,
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                if matches!(&resp, EmuResponse::LoadStateOk { .. }) {
                    rewind_buffer.clear();
                    Self::install_rom_patches(backend, last_cheats);
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::SaveStateToPath(path) => {
                let resp = match backend.encode_state_bytes() {
                    Ok(bytes) => {
                        match crate::save_paths::write_state_bytes_to_file(&path, &bytes) {
                            Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                            Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
                        }
                    }
                    Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
                };
                pending_responses.push_back(resp);
            }
            EmuCommand::LoadStateFromPath {
                path,
                buttons_pressed,
                dpad_pressed,
            } => {
                let result = backend.load_state_from_path(&path);
                let resp = Self::respond_load(
                    backend,
                    result,
                    path.display().to_string(),
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                if matches!(&resp, EmuResponse::LoadStateOk { .. }) {
                    rewind_buffer.clear();
                    Self::install_rom_patches(backend, last_cheats);
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::AutoSaveState => {
                let resp = match backend.auto_save_path() {
                    Some(path) => match backend.encode_state_bytes() {
                        Ok(bytes) => {
                            match crate::save_paths::write_state_bytes_to_file(&path, &bytes) {
                                Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                                Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
                            }
                        }
                        Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
                    },
                    None => EmuResponse::SaveStateFailed("no auto-save path".to_string()),
                };
                pending_responses.push_back(resp);
            }
            EmuCommand::AutoLoadState {
                buttons_pressed,
                dpad_pressed,
            } => {
                let resp = match backend.auto_save_path() {
                    Some(path) => {
                        let result = backend.load_state_from_path(&path);
                        Self::respond_load(
                            backend,
                            result,
                            path.display().to_string(),
                            buttons_pressed,
                            dpad_pressed,
                            &self.shared_framebuffer,
                        )
                    }
                    None => EmuResponse::LoadStateFailed("no auto-save path".to_string()),
                };
                if matches!(&resp, EmuResponse::LoadStateOk { .. }) {
                    rewind_buffer.clear();
                    Self::install_rom_patches(backend, last_cheats);
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::CaptureStateBytes => {
                let resp = match backend.encode_state_bytes() {
                    Ok(bytes) => EmuResponse::StateCaptured(bytes),
                    Err(e) => EmuResponse::StateCaptureFailed(e.to_string()),
                };
                pending_responses.push_back(resp);
            }
            EmuCommand::LoadStateBytes {
                state_bytes,
                buttons_pressed,
                dpad_pressed,
            } => {
                let result = backend.load_state_from_bytes(state_bytes);
                let resp = Self::respond_load(
                    backend,
                    result,
                    "bytes".to_string(),
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                if matches!(&resp, EmuResponse::LoadStateOk { .. }) {
                    rewind_buffer.clear();
                    Self::install_rom_patches(backend, last_cheats);
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::UpdateCheats(patches) => {
                *last_cheats = patches;
                Self::install_rom_patches(backend, last_cheats);
            }
            EmuCommand::Rewind => {
                let resp = Self::handle_rewind(backend, rewind_buffer, &self.shared_framebuffer);
                if matches!(&resp, EmuResponse::RewindOk) {
                    Self::install_rom_patches(backend, last_cheats);
                }
                pending_responses.push_back(resp);
            }
            EmuCommand::Shutdown => {
                pending_responses.push_back(EmuResponse::ShutdownComplete);
            }
        }
    }

    pub(crate) fn try_recv_frame(&self) -> Option<FrameResult> {
        self.inner.borrow_mut().pending_frames.pop_front()
    }

    pub(crate) fn recv(&self) -> Option<EmuResponse> {
        self.inner.borrow_mut().pending_responses.pop_front()
    }

    pub(crate) fn try_recv_response(&self) -> Option<EmuResponse> {
        self.inner.borrow_mut().pending_responses.pop_front()
    }

    pub(crate) fn shutdown(&mut self) {}

    fn save_state_sync(backend: &EmuBackend, slot: u8) -> EmuResponse {
        match backend.encode_state_bytes() {
            Ok(bytes) => {
                let path = match backend.slot_path(slot) {
                    Ok(p) => p,
                    Err(e) => return EmuResponse::SaveStateFailed(e.to_string()),
                };
                match crate::save_paths::write_state_bytes_to_file(&path, &bytes) {
                    Ok(()) => EmuResponse::SaveStateOk(path.display().to_string()),
                    Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
                }
            }
            Err(e) => EmuResponse::SaveStateFailed(e.to_string()),
        }
    }

    fn load_state_sync(
        backend: &mut EmuBackend,
        slot: u8,
        buttons_pressed: u8,
        dpad_pressed: u8,
        shared_fb: &SharedFramebuffer,
    ) -> EmuResponse {
        let path = match backend.slot_path(slot) {
            Ok(p) => p,
            Err(e) => return EmuResponse::LoadStateFailed(e.to_string()),
        };
        let result = backend.load_state_from_path(&path);
        Self::respond_load(
            backend,
            result,
            path.display().to_string(),
            buttons_pressed,
            dpad_pressed,
            shared_fb,
        )
    }

    fn respond_load(
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

    #[allow(clippy::too_many_arguments)]
    fn step_frames_inline(
        backend: &mut EmuBackend,
        input: FrameInput,
        uncapped_mode: bool,
        cheats: &[CheatPatch],
        rewind_buffer: &mut RewindBuffer,
        rewind_seconds: &mut usize,
        shared_fb: &SharedFramebuffer,
    ) -> FrameResult {
        if let Some(gb) = backend.gb_mut() {
            Self::apply_debug_actions(&mut gb.emu, &input.debug_actions);
        }

        if let Some(nes) = backend.nes_mut() {
            Self::apply_nes_debug_actions(&mut nes.emu, &input.debug_actions);
        }

        backend.set_input(input.joypad.buttons, input.joypad.dpad);
        backend.set_input_p2(input.joypad.buttons_p2, input.joypad.dpad_p2);

        if let Some(mutes) = &input.debug_actions.apu_channel_mutes {
            backend.set_apu_channel_mutes(mutes);
        }

        if let Some(gb) = backend.gb_mut() {
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

        if input.frames > 0 && backend.is_running() {
            for _ in 0..input.frames {
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

        // Resize rewind buffer if settings changed
        if input.rewind_seconds != *rewind_seconds {
            *rewind_seconds = input.rewind_seconds;
            *rewind_buffer = RewindBuffer::new(*rewind_seconds, REWIND_SNAPSHOTS_PER_SECOND);
        }

        // Capture rewind snapshot
        if input.rewind_enabled
            && rewind_buffer.tick()
            && let Ok(bytes) = backend.encode_state_bytes()
        {
            rewind_buffer.push(&bytes, backend.framebuffer());
        }

        let ui_data = match backend {
            EmuBackend::Gb(gb) => ui::collect_emu_snapshot(
                &gb.emu,
                &input.snapshot,
                input.buffers.vram,
                input.buffers.oam,
                input.buffers.memory_page,
            ),
            EmuBackend::Nes(nes) => ui::collect_nes_snapshot(&mut nes.emu, &input.snapshot),
        };

        let src = backend.framebuffer();
        shared_fb.store(Some(Arc::new(src.to_vec())));

        let mut audio_samples = input.buffers.audio.unwrap_or_default();
        backend.drain_audio_samples_into(&mut audio_samples);

        FrameResult {
            rumble: backend.rumble_active(),
            audio_samples,
            ui_data,
            is_mbc7: backend.is_mbc7(),
            is_pocket_camera: backend.is_pocket_camera(),
            rewind_fill: rewind_buffer.fill_ratio(),
            apu_snapshot: None,
        }
    }

    fn handle_rewind(
        backend: &mut EmuBackend,
        rewind_buffer: &mut RewindBuffer,
        shared_fb: &SharedFramebuffer,
    ) -> EmuResponse {
        let current_state = backend.encode_state_bytes().ok();
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
}
