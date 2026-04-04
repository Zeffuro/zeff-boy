use crossbeam_channel::{Receiver, Sender};

use crate::cheats::CheatPatch;
use crate::emu_backend::EmuBackend;

use super::{
    DEFAULT_REWIND_SECONDS, EmuCommand, EmuResponse, EmuThread, FrameResult,
    REWIND_SNAPSHOTS_PER_SECOND, SharedFramebuffer,
};

pub(super) struct EmuLoop {
    pub(super) backend: EmuBackend,
    pub(super) cmd_rx: Receiver<EmuCommand>,
    pub(super) frame_tx: Sender<FrameResult>,
    pub(super) drain_rx: Receiver<FrameResult>,
    pub(super) resp_tx: Sender<EmuResponse>,
    pub(super) shared_framebuffer: SharedFramebuffer,
    uncapped_mode: bool,
    last_cheats: Vec<CheatPatch>,
    rewind_buffer: zeff_emu_common::rewind::RewindBuffer,
    rewind_seconds: usize,
}

impl EmuLoop {
    pub(super) fn new(
        backend: EmuBackend,
        cmd_rx: Receiver<EmuCommand>,
        frame_tx: Sender<FrameResult>,
        drain_rx: Receiver<FrameResult>,
        resp_tx: Sender<EmuResponse>,
        shared_framebuffer: SharedFramebuffer,
    ) -> Self {
        Self {
            backend,
            cmd_rx,
            frame_tx,
            drain_rx,
            resp_tx,
            shared_framebuffer,
            uncapped_mode: false,
            last_cheats: Vec::new(),
            rewind_buffer: zeff_emu_common::rewind::RewindBuffer::new(
                DEFAULT_REWIND_SECONDS,
                REWIND_SNAPSHOTS_PER_SECOND,
            ),
            rewind_seconds: DEFAULT_REWIND_SECONDS,
        }
    }

    pub(super) fn run(&mut self) {
        loop {
            let command = if self.uncapped_mode {
                match self.cmd_rx.try_recv() {
                    Ok(cmd) => Some(cmd),
                    Err(crossbeam_channel::TryRecvError::Empty) => None,
                    Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                }
            } else {
                match self.cmd_rx.recv() {
                    Ok(cmd) => Some(cmd),
                    Err(_) => break,
                }
            };

            if let Some(command) = command {
                if !self.handle_command(command) {
                    break;
                }
            } else {
                EmuThread::run_uncapped_batch(
                    &mut self.backend,
                    &self.last_cheats,
                    &self.shared_framebuffer,
                    &self.rewind_buffer,
                    &self.frame_tx,
                    &self.drain_rx,
                );
            }
        }
    }

    fn handle_command(&mut self, command: EmuCommand) -> bool {
        match command {
            EmuCommand::SetUncapped(on) => {
                self.uncapped_mode = on;
                self.backend.set_apu_sample_generation_enabled(!on);
            }

            EmuCommand::UpdateCheats(cheats) => {
                self.last_cheats = cheats;
                EmuThread::install_rom_patches(&mut self.backend, &self.last_cheats);
            }

            EmuCommand::StepFrames(input) => {
                let input = *input;
                let result = EmuThread::handle_step_frames(
                    &mut self.backend,
                    input,
                    &self.last_cheats,
                    self.uncapped_mode,
                    &mut self.rewind_buffer,
                    &mut self.rewind_seconds,
                    &self.shared_framebuffer,
                );
                if !EmuThread::send_frame(&self.frame_tx, &self.drain_rx, result) {
                    return false;
                }
            }

            EmuCommand::SaveStateSlot(slot) => match self.backend.slot_path(slot) {
                Ok(path) => {
                    if !EmuThread::save_state_async(
                        &self.backend,
                        path,
                        &self.resp_tx,
                        &self.send_resp_fn(),
                    ) {
                        return false;
                    }
                }
                Err(e) => {
                    if !self.send_resp(EmuResponse::SaveStateFailed(e.to_string())) {
                        return false;
                    }
                }
            },

            EmuCommand::LoadStateSlot {
                slot,
                buttons_pressed,
                dpad_pressed,
            } => {
                let result = self.backend.load_state(slot);
                let loaded = result.is_ok();
                let path_label = result.as_ref().ok().cloned().unwrap_or_default();
                let resp = EmuThread::respond_load_state(
                    &mut self.backend,
                    result.map(|_| ()),
                    path_label,
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                if loaded {
                    self.rewind_buffer.clear();
                    EmuThread::install_rom_patches(&mut self.backend, &self.last_cheats);
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::SaveStateToPath(path) => {
                if !EmuThread::save_state_async(
                    &self.backend,
                    path,
                    &self.resp_tx,
                    &self.send_resp_fn(),
                ) {
                    return false;
                }
            }

            EmuCommand::LoadStateFromPath {
                path,
                buttons_pressed,
                dpad_pressed,
            } => {
                let label = path.display().to_string();
                let result = self.backend.load_state_from_path(&path);
                let loaded = result.is_ok();
                let resp = EmuThread::respond_load_state(
                    &mut self.backend,
                    result,
                    label,
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                if loaded {
                    self.rewind_buffer.clear();
                    EmuThread::install_rom_patches(&mut self.backend, &self.last_cheats);
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::SetSampleRate(rate) => {
                self.backend.set_sample_rate(rate);
            }

            EmuCommand::CaptureStateBytes => {
                let resp = match EmuThread::encode_current_state(&self.backend) {
                    Ok(bytes) => EmuResponse::StateCaptured(bytes),
                    Err(err) => EmuResponse::StateCaptureFailed(err.to_string()),
                };
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::LoadStateBytes {
                state_bytes,
                buttons_pressed,
                dpad_pressed,
            } => {
                let result = self.backend.load_state_from_bytes(state_bytes);
                let loaded = result.is_ok();
                let resp = EmuThread::respond_load_state(
                    &mut self.backend,
                    result,
                    "(replay)".to_string(),
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                if loaded {
                    self.rewind_buffer.clear();
                    EmuThread::install_rom_patches(&mut self.backend, &self.last_cheats);
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::AutoSaveState => {
                if let Some(path) = self.backend.auto_save_path() {
                    if !EmuThread::save_state_async(
                        &self.backend,
                        path,
                        &self.resp_tx,
                        &self.send_resp_fn(),
                    ) {
                        return false;
                    }
                } else if !self.send_resp(EmuResponse::SaveStateFailed(
                    "Auto-save not supported for this system".to_string(),
                )) {
                    return false;
                }
            }

            EmuCommand::AutoLoadState {
                buttons_pressed,
                dpad_pressed,
            } => {
                return self.handle_auto_load(buttons_pressed, dpad_pressed);
            }

            EmuCommand::Rewind => {
                let resp = EmuThread::handle_rewind(
                    &mut self.backend,
                    &mut self.rewind_buffer,
                    &self.shared_framebuffer,
                );
                if matches!(&resp, EmuResponse::RewindOk) {
                    EmuThread::install_rom_patches(&mut self.backend, &self.last_cheats);
                }
                if !self.send_resp(resp) {
                    return false;
                }
            }

            EmuCommand::Shutdown => {
                self.handle_shutdown();
                return false;
            }
        }
        true
    }

    fn send_resp(&self, resp: EmuResponse) -> bool {
        self.resp_tx.send(resp).is_ok()
    }

    fn send_resp_fn(&self) -> impl Fn(EmuResponse) -> bool + '_ {
        |resp| self.resp_tx.send(resp).is_ok()
    }

    fn handle_auto_load(&mut self, buttons_pressed: u8, dpad_pressed: u8) -> bool {
        if let Some(path) = self.backend.auto_save_path() {
            if path.exists() {
                let label = path.display().to_string();
                let result = self.backend.load_state_from_path(&path);
                let loaded = result.is_ok();
                let resp = EmuThread::respond_load_state(
                    &mut self.backend,
                    result,
                    label,
                    buttons_pressed,
                    dpad_pressed,
                    &self.shared_framebuffer,
                );
                if loaded {
                    self.rewind_buffer.clear();
                    EmuThread::install_rom_patches(&mut self.backend, &self.last_cheats);
                }
                self.send_resp(resp)
            } else {
                self.send_resp(EmuResponse::LoadStateFailed("no auto-save".to_string()))
            }
        } else {
            self.send_resp(EmuResponse::LoadStateFailed("no auto-save".to_string()))
        }
    }

    fn handle_shutdown(&mut self) {
        let sram_path = self.backend.flush_battery_sram().unwrap_or_else(|err| {
            log::error!("Failed to flush SRAM on shutdown: {}", err);
            None
        });
        if self
            .resp_tx
            .send(EmuResponse::SramFlushed(sram_path))
            .is_err()
        {
            log::debug!("shutdown: SRAM flush response dropped (receiver closed)");
        }
        if self.resp_tx.send(EmuResponse::ShutdownComplete).is_err() {
            log::debug!("shutdown: completion response dropped (receiver closed)");
        }
    }
}
