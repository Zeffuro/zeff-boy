use super::App;
use crate::emu_thread::{EmuCommand, EmuResponse};

impl App {
    pub(in crate::app) fn start_replay_recording(&mut self) {
        if self.emu_thread.is_none() {
            return;
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.toast_manager
                .error("Replay recording is not yet available on web");
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let default_name = self
                .rom_info
                .rom_path
                .as_ref()
                .and_then(|p| p.file_stem())
                .and_then(|s| s.to_str())
                .map(|stem| format!("{stem}.zrpl"))
                .unwrap_or_else(|| "replay.zrpl".to_string());

            let was_paused = self.pause_for_dialog();
            let file = crate::platform::FileDialog::new()
                .set_title("Save Replay")
                .set_directory(self.state_dialog_dir())
                .add_filter("Zeff Boy Replay", &["zrpl"])
                .set_file_name(&default_name)
                .save_file();

            self.resume_after_dialog(was_paused);
            let Some(path) = file else {
                return;
            };

            if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::CaptureStateBytes);
            }
            match self.recv_cold_response() {
                Some(EmuResponse::StateCaptured(state_bytes)) => {
                    let recorder = zeff_emu_common::replay::ReplayRecorder::new(path, state_bytes);
                    self.recording.replay_recorder = Some(recorder);
                    self.toast_manager.set_replay_recording(true);
                }
                Some(EmuResponse::StateCaptureFailed(err)) => {
                    log::error!("Failed to capture state for replay: {}", err);
                    self.toast_manager
                        .error(format!("Replay start failed: {err}"));
                }
                _ => {}
            }
        }
    }

    pub(in crate::app) fn stop_replay_recording(&mut self) {
        if let Some(recorder) = self.recording.replay_recorder.take() {
            self.toast_manager.set_replay_recording(false);
            let frame_count = recorder.frame_count();
            match recorder.finish() {
                Ok(path) => {
                    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                    log::info!(
                        "Replay saved to {} ({} frames)",
                        path.display(),
                        frame_count
                    );
                    self.toast_manager
                        .success(format!("Saved {name} ({frame_count} frames)"));
                }
                Err(err) => {
                    log::error!("Failed to save replay: {}", err);
                    self.toast_manager
                        .error(format!("Replay save failed: {err}"));
                }
            }
        }
    }

    pub(in crate::app) fn load_and_play_replay(&mut self) {
        if self.emu_thread.is_none() {
            return;
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.toast_manager
                .error("Replay playback is not yet available on web");
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let file = crate::platform::FileDialog::new()
                .set_title("Load Replay")
                .set_directory(self.state_dialog_dir())
                .add_filter("Zeff Boy Replay", &["zrpl"])
                .pick_file();

            let Some(path) = file else {
                return;
            };

            match zeff_emu_common::replay::ReplayPlayer::load(&path) {
                Ok(player) => {
                    let total = player.total_frames();
                    let state_bytes = player.save_state().to_vec();
                    if let Some(thread) = &self.emu_thread {
                        thread.send(EmuCommand::LoadStateBytes {
                            state_bytes,
                            buttons_pressed: 0,
                            dpad_pressed: 0,
                        });
                    }
                    match self.recv_cold_response() {
                        Some(EmuResponse::LoadStateOk { .. }) => {
                            if let Some(thread) = &self.emu_thread {
                                self.latest_frame = thread.shared_framebuffer().load_full();
                            }
                            self.recording.replay_player = Some(player);
                            self.toast_manager
                                .info(format!("Playing replay ({total} frames)"));
                        }
                        Some(EmuResponse::LoadStateFailed(err)) => {
                            log::error!("Failed to load replay state: {}", err);
                            self.toast_manager
                                .error(format!("Replay load failed: {err}"));
                        }
                        _ => {}
                    }
                }
                Err(err) => {
                    log::error!("Failed to load replay: {}", err);
                    self.toast_manager
                        .error(format!("Replay load failed: {err}"));
                }
            }
        }
    }
}
