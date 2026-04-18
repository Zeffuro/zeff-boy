use std::cell::RefCell;
use std::collections::VecDeque;

use zeff_emu_common::rewind::RewindBuffer;

use super::types::{self, EmuCommand, EmuResponse, FrameInput, FrameResult, SharedFramebuffer};
use super::{DEFAULT_REWIND_SECONDS, REWIND_SNAPSHOTS_PER_SECOND};
use crate::cheats::CheatPatch;
use crate::emu_backend::EmuBackend;

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
                let result = Self::handle_step_frames(
                    backend,
                    *input,
                    last_cheats,
                    *uncapped_mode,
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
                Self::finalize_load_state(&resp, rewind_buffer, backend, last_cheats);
                pending_responses.push_back(resp);
            }
            EmuCommand::SaveStateToPath(path) => {
                let resp = Self::encode_and_write_state(backend, &path);
                pending_responses.push_back(resp);
            }
            EmuCommand::LoadStateFromPath {
                path,
                buttons_pressed,
                dpad_pressed,
            } => {
                let result = backend.load_state_from_path(&path);
                let resp = Self::respond_load_state(
                    backend,
                    result,
                    path.display().to_string(),
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                Self::finalize_load_state(&resp, rewind_buffer, backend, last_cheats);
                pending_responses.push_back(resp);
            }
            EmuCommand::AutoSaveState => {
                let resp = match backend.auto_save_path() {
                    Some(path) => Self::encode_and_write_state(backend, &path),
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
                        Self::respond_load_state(
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
                Self::finalize_load_state(&resp, rewind_buffer, backend, last_cheats);
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
                let resp = Self::respond_load_state(
                    backend,
                    result,
                    "bytes".to_string(),
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                Self::finalize_load_state(&resp, rewind_buffer, backend, last_cheats);
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
        match backend.slot_path(slot) {
            Ok(path) => Self::encode_and_write_state(backend, &path),
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
        Self::respond_load_state(
            backend,
            result,
            path.display().to_string(),
            buttons_pressed,
            dpad_pressed,
            shared_fb,
        )
    }
}
