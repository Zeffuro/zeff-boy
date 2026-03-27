mod cheats;
mod debug_actions;
mod runner;
mod state;
mod types;

use std::thread::{self, JoinHandle};

use crossbeam_channel::{self as chan, Receiver, Sender};

use crate::emu_backend::EmuBackend;

pub(crate) use types::*;

pub(crate) struct EmuThread {
    cmd_tx: Sender<EmuCommand>,
    frame_rx: Receiver<FrameResult>,
    resp_rx: Receiver<EmuResponse>,
    join: Option<JoinHandle<()>>,
}

impl EmuThread {
    pub(crate) fn spawn(mut backend: EmuBackend) -> Self {
        let (cmd_tx, cmd_rx) = chan::unbounded();
        let (frame_tx, frame_rx) = chan::bounded::<FrameResult>(1);
        let (resp_tx, resp_rx) = chan::unbounded();

        let drain_rx = frame_rx.clone();

        let join = thread::spawn(move || {
            let mut uncapped_mode = false;
            let mut uncapped_fb: Option<Vec<u8>> = None;
            let mut last_cheats: Vec<crate::cheats::CheatPatch> = Vec::new();

            let mut rewind_buffer = zeff_gb_core::rewind::RewindBuffer::new(10, 4);
            let mut rewind_seconds = 10usize;

            let send_resp = |resp: EmuResponse| -> bool { resp_tx.send(resp).is_ok() };

            'main: loop {
                let command = if uncapped_mode {
                    match cmd_rx.try_recv() {
                        Ok(cmd) => Some(cmd),
                        Err(crossbeam_channel::TryRecvError::Empty) => None,
                        Err(crossbeam_channel::TryRecvError::Disconnected) => break,
                    }
                } else {
                    match cmd_rx.recv() {
                        Ok(cmd) => Some(cmd),
                        Err(_) => break,
                    }
                };

                if let Some(command) = command {
                    match command {
                        EmuCommand::SetUncapped(on) => {
                            uncapped_mode = on;
                            backend.set_apu_sample_generation_enabled(!on);
                        }

                        EmuCommand::UpdateCheats(cheats) => {
                            last_cheats = cheats;
                            Self::install_rom_patches(&mut backend, &last_cheats);
                        }

                        EmuCommand::StepFrames(input) => {
                            let result = Self::handle_step_frames(
                                &mut backend,
                                input,
                                &last_cheats,
                                uncapped_mode,
                                &mut rewind_buffer,
                                &mut rewind_seconds,
                            );

                            if !Self::send_frame(&frame_tx, &drain_rx, result) {
                                break 'main;
                            }
                        }

                        EmuCommand::SaveStateSlot(slot) => {
                            match backend.slot_path(slot) {
                                Ok(path) => {
                                    if !Self::save_state_async(&backend, path, &resp_tx, &send_resp) {
                                        break 'main;
                                    }
                                }
                                Err(e) => {
                                    if !send_resp(EmuResponse::SaveStateFailed(e.to_string())) {
                                        break 'main;
                                    }
                                }
                            }
                        }

                        EmuCommand::LoadStateSlot {
                            slot,
                            buttons_pressed,
                            dpad_pressed,
                        } => {
                            let result = backend.load_state(slot);
                            let path_label = result.as_ref().ok().cloned().unwrap_or_default();
                            let resp = Self::respond_load_state(
                                &mut backend,
                                result.map(|_| ()),
                                path_label,
                                buttons_pressed,
                                dpad_pressed,
                            );
                            if !send_resp(resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::SaveStateToPath(path) => {
                            if !Self::save_state_async(&backend, path, &resp_tx, &send_resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::LoadStateFromPath {
                            path,
                            buttons_pressed,
                            dpad_pressed,
                        } => {
                            let label = path.display().to_string();
                            let result = backend.load_state_from_path(&path);
                            let resp = Self::respond_load_state(
                                &mut backend,
                                result,
                                label,
                                buttons_pressed,
                                dpad_pressed,
                            );
                            if !send_resp(resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::SetSampleRate(rate) => {
                            backend.set_sample_rate(rate);
                        }

                        EmuCommand::CaptureStateBytes => {
                            let resp = match Self::encode_current_state(&backend) {
                                Ok(bytes) => EmuResponse::StateCaptured(bytes),
                                Err(err) => EmuResponse::StateCaptureFailed(err.to_string()),
                            };
                            if !send_resp(resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::LoadStateBytes {
                            state_bytes,
                            buttons_pressed,
                            dpad_pressed,
                        } => {
                            let result = backend.load_state_from_bytes(state_bytes);
                            let resp = Self::respond_load_state(
                                &mut backend,
                                result,
                                "(replay)".to_string(),
                                buttons_pressed,
                                dpad_pressed,
                            );
                            if !send_resp(resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::AutoSaveState => {
                            if let Some(path) = backend.auto_save_path() {
                                if !Self::save_state_async(&backend, path, &resp_tx, &send_resp) {
                                    break 'main;
                                }
                            } else if !send_resp(EmuResponse::SaveStateFailed("Auto-save not supported for this system".to_string())) {
                                break 'main;
                            }
                        }

                        EmuCommand::AutoLoadState {
                            buttons_pressed,
                            dpad_pressed,
                        } => {
                            if let Some(path) = backend.auto_save_path() {
                                if path.exists() {
                                    let label = path.display().to_string();
                                    let result = backend.load_state_from_path(&path);
                                    let resp = Self::respond_load_state(
                                        &mut backend,
                                        result,
                                        label,
                                        buttons_pressed,
                                        dpad_pressed,
                                    );
                                    if !send_resp(resp) {
                                        break 'main;
                                    }
                                } else if !send_resp(EmuResponse::LoadStateFailed(
                                    "no auto-save".to_string(),
                                )) {
                                    break 'main;
                                }
                            } else if !send_resp(EmuResponse::LoadStateFailed(
                                "no auto-save".to_string(),
                            )) {
                                break 'main;
                            }
                        }

                        EmuCommand::Rewind => {
                            let resp = Self::handle_rewind(&mut backend, &mut rewind_buffer);
                            if !send_resp(resp) {
                                break 'main;
                            }
                        }

                        EmuCommand::Shutdown => {
                            let sram_path = backend.flush_battery_sram().unwrap_or_else(|err| {
                                log::error!("Failed to flush SRAM on shutdown: {}", err);
                                None
                            });
                            let _ = resp_tx.send(EmuResponse::SramFlushed(sram_path));
                            let _ = resp_tx.send(EmuResponse::ShutdownComplete);
                            break 'main;
                        }
                    }
                } else {
                    Self::run_uncapped_batch(
                        &mut backend,
                        &last_cheats,
                        &mut uncapped_fb,
                        &rewind_buffer,
                        &frame_tx,
                        &drain_rx,
                    );
                }
            }
        });

        Self {
            cmd_tx,
            frame_rx,
            resp_rx,
            join: Some(join),
        }
    }

    pub(crate) fn send(&self, cmd: EmuCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    pub(crate) fn try_recv_frame(&self) -> Option<FrameResult> {
        self.frame_rx.try_recv().ok()
    }

    pub(crate) fn recv(&self) -> Option<EmuResponse> {
        self.resp_rx.recv().ok()
    }

    pub(crate) fn try_recv_response(&self) -> Option<EmuResponse> {
        self.resp_rx.try_recv().ok()
    }

    pub(crate) fn shutdown(&mut self) {
        let _ = self.cmd_tx.send(EmuCommand::Shutdown);
        while self.frame_rx.try_recv().is_ok() {}

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let timeout = deadline.saturating_duration_since(std::time::Instant::now());
            if timeout.is_zero() {
                log::warn!("Emu thread shutdown timed out after 5s");
                break;
            }
            match self.resp_rx.recv_timeout(timeout) {
                Ok(EmuResponse::ShutdownComplete) => break,
                Ok(EmuResponse::SramFlushed(Some(path))) => {
                    log::info!("Saved battery RAM to {}", path);
                }
                Ok(_) => continue,
                Err(_) => break,
            }
        }
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for EmuThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

