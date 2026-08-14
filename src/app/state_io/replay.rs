#[cfg(not(target_arch = "wasm32"))]
use super::super::types::{PendingReplayStart, ReplayFinalizationState, ReplaySaveResult};
use super::App;
#[cfg(not(target_arch = "wasm32"))]
use crate::emu_thread::ReplayStartState;
use crate::emu_thread::{EmuCommand, EmuResponse};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

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
            if self.recording.is_replay_finalizing() {
                self.toast_manager.info("Replay save is still finishing");
                return;
            }

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

            if let Err(err) = self.start_replay_recording_to_path(path) {
                log::error!("Failed to start replay recording: {err}");
                self.toast_manager
                    .error(format!("Replay start failed: {err}"));
            }
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(in crate::app) fn start_replay_recording_to_path(
        &mut self,
        path: PathBuf,
    ) -> anyhow::Result<()> {
        if self.emu_thread.is_none() {
            anyhow::bail!("no ROM is running");
        }
        if self.recording.is_replay_finalizing() {
            anyhow::bail!("replay save is still finishing");
        }
        if self.recording.pending_replay_start.is_some() {
            anyhow::bail!("replay recording is already starting");
        }
        if self.recording.replay_recorder.is_some() {
            anyhow::bail!("replay recording is already active");
        }
        if self.recording.replay_player.is_some() {
            anyhow::bail!("stop replay playback before recording");
        }

        self.disable_uncapped_for_replay();
        self.clear_replay_progress();

        self.recording.pending_replay_start = Some(PendingReplayStart { path });
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::CaptureReplayStart);
        }
        self.toast_manager.info("Replay recording starting");
        Ok(())
    }

    pub(in crate::app) fn stop_replay_recording(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.recording.pending_replay_start.take().is_some() {
            self.clear_replay_progress();
            self.toast_manager.set_replay_recording(false);
            self.toast_manager.info("Replay start canceled");
            return;
        }

        if self.recording.replay_recorder.is_some() {
            self.toast_manager.set_replay_recording(false);
            while let Some(result) = self.emu_thread.as_ref().and_then(|t| t.try_recv_frame()) {
                self.process_frame_result(result);
            }
            let capture_requested = if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::CaptureStateBytes);
                true
            } else {
                false
            };
            let Some(recorder) = self.recording.replay_recorder.take() else {
                return;
            };
            let frame_count = recorder.frame_count();

            #[cfg(not(target_arch = "wasm32"))]
            {
                self.recording.replay_finalization =
                    Some(ReplayFinalizationState::CapturingFinalState {
                        recorder: Box::new(recorder),
                        frame_count,
                    });
                self.toast_manager.set_replay_saving(true);
                if !capture_requested {
                    self.start_replay_save_worker(
                        None,
                        Some("emulator thread is not available".to_string()),
                    );
                }
            }

            #[cfg(target_arch = "wasm32")]
            {
                drop(recorder);
                self.clear_replay_progress();
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
            if self.recording.is_replay_finalizing() {
                self.toast_manager.info("Replay save is still finishing");
                return;
            }

            self.disable_uncapped_for_replay();
            self.clear_replay_progress();

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
                    if let Err(err) = self.validate_replay_playback(&player) {
                        log::error!("Replay metadata mismatch: {err}");
                        self.toast_manager.error(format!("Replay mismatch: {err}"));
                        return;
                    }
                    let total = player.total_frames();
                    let state_bytes = player.save_state().to_vec();
                    if let Some(thread) = &self.emu_thread {
                        thread.send(EmuCommand::LoadStateBytes {
                            state_bytes,
                            buttons_pressed: 0,
                            dpad_pressed: 0,
                            replay_events: Some(player.metadata().events.clone()),
                            game_boy_link_start_state: player.metadata().game_boy_link_start_state,
                            game_boy_link_start_tick: player.metadata().game_boy_link_start_tick,
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

    pub(in crate::app) fn disable_uncapped_for_replay(&mut self) {
        if !(self.timing.uncapped_speed || self.settings.emulation.uncapped_speed) {
            return;
        }

        self.timing.uncapped_speed = false;
        self.settings.emulation.uncapped_speed = false;
        self.settings.save();
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::SetUncapped(false));
        }
        self.toast_manager
            .info("Uncapped mode disabled for deterministic replay");
    }

    fn clear_replay_progress(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.recording.pending_replay_start = None;
        }
        self.recording.pending_replay_batches.clear();
        self.recording.queued_replay_playback_frames = 0;
        self.recording.replay_recording_origin = crate::app::types::ReplayCaptureOrigin::default();
        self.recording.replay_media_events_pending = 0;
    }

    fn validate_replay_playback(
        &self,
        player: &zeff_emu_common::replay::ReplayPlayer,
    ) -> anyhow::Result<()> {
        let metadata = player.metadata();
        if metadata.is_empty() {
            return self.validate_replay_input_devices(player);
        }

        let current = self
            .rom_info
            .replay_metadata
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("load the matching ROM first"))?;

        if metadata.system != current.system {
            anyhow::bail!(
                "system differs (replay={}, current={})",
                metadata.system.as_deref().unwrap_or("unknown"),
                current.system.as_deref().unwrap_or("unknown")
            );
        }
        if metadata.rom_sha256 != current.rom_sha256 {
            anyhow::bail!("ROM hash differs");
        }
        if metadata.firmware != current.firmware {
            anyhow::bail!("firmware differs");
        }
        let current_cheat_hash = crate::cheats::enabled_patch_hash(
            &self.debug_windows.cheat.user_codes,
            &self.debug_windows.cheat.libretro_codes,
        );
        if metadata.cheat_sha256 != current_cheat_hash {
            anyhow::bail!("enabled cheats differ");
        }

        self.validate_replay_input_devices(player)?;

        Ok(())
    }

    fn validate_replay_input_devices(
        &self,
        player: &zeff_emu_common::replay::ReplayPlayer,
    ) -> anyhow::Result<()> {
        if player.uses_zapper_input() && self.active_system != crate::emu_backend::ActiveSystem::Nes
        {
            anyhow::bail!("replay contains NES Zapper input but current ROM is not a NES game");
        }
        if player.uses_game_boy_link_events()
            && self.active_system != crate::emu_backend::ActiveSystem::GameBoy
        {
            anyhow::bail!(
                "replay contains Game Boy link events but current ROM is not a GB/GBC game"
            );
        }
        if player.uses_wonder_swan_link_events()
            && self.active_system != crate::emu_backend::ActiveSystem::WonderSwan
        {
            anyhow::bail!(
                "replay contains WonderSwan link events but current ROM is not a WonderSwan game"
            );
        }
        if player.uses_host_tilt_input() && !self.rom_info.is_mbc7 {
            anyhow::bail!("replay contains MBC7 tilt input but current ROM is not an MBC7 game");
        }
        if player.uses_host_camera_input() && !self.rom_info.is_pocket_camera {
            anyhow::bail!(
                "replay contains Pocket Camera input but current ROM is not a Pocket Camera game"
            );
        }
        validate_replay_host_input_frame_shapes(player)?;
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl App {
    pub(in crate::app) fn consume_replay_start_response(
        &mut self,
        response: EmuResponse,
    ) -> Option<EmuResponse> {
        match response {
            EmuResponse::ReplayStartCaptured(start) => {
                let Some(pending) = self.recording.pending_replay_start.take() else {
                    log::warn!("Ignoring stale replay start capture response");
                    return None;
                };
                if let Err(err) = self.finish_pending_replay_start(start, pending.path) {
                    log::error!("Failed to start replay recording: {err}");
                    self.clear_replay_progress();
                    self.toast_manager
                        .error(format!("Replay start failed: {err}"));
                }
                None
            }
            EmuResponse::StateCaptureFailed(err)
                if self.recording.pending_replay_start.is_some() =>
            {
                self.recording.pending_replay_start = None;
                self.clear_replay_progress();
                let message = format!("failed to capture replay start state: {err}");
                log::error!("{message}");
                self.toast_manager
                    .error(format!("Replay start failed: {err}"));
                None
            }
            other => Some(other),
        }
    }

    fn finish_pending_replay_start(
        &mut self,
        start: Box<ReplayStartState>,
        path: PathBuf,
    ) -> anyhow::Result<()> {
        if self.recording.replay_recorder.is_some() {
            anyhow::bail!("replay recording is already active");
        }
        if self.recording.replay_player.is_some() {
            anyhow::bail!("stop replay playback before recording");
        }

        let mut metadata = start.metadata;
        metadata.cheat_sha256 = crate::cheats::enabled_patch_hash(
            &self.debug_windows.cheat.user_codes,
            &self.debug_windows.cheat.libretro_codes,
        );
        metadata.game_boy_link_start_tick = start.game_boy_cpu_cycles;
        let recorder = zeff_emu_common::replay::ReplayRecorder::new_with_metadata(
            path,
            start.state_bytes,
            metadata,
        );
        self.recording.replay_recording_origin = crate::app::types::ReplayCaptureOrigin {
            frame: start.frame_count,
            game_boy_tick: start.game_boy_cpu_cycles,
        };
        self.recording.replay_recorder = Some(recorder);
        self.toast_manager.set_replay_recording(true);
        Ok(())
    }

    pub(in crate::app) fn consume_replay_finalization_response(
        &mut self,
        response: EmuResponse,
    ) -> Option<EmuResponse> {
        if !self.recording.is_replay_final_state_capture_pending() {
            return Some(response);
        }

        match response {
            EmuResponse::StateCaptured(bytes) => {
                self.start_replay_save_worker(Some(bytes), None);
                None
            }
            EmuResponse::StateCaptureFailed(err) => {
                log::warn!("Replay final state hash capture failed: {err}");
                self.start_replay_save_worker(None, Some(err));
                None
            }
            other => Some(other),
        }
    }

    fn start_replay_save_worker(
        &mut self,
        final_state_bytes: Option<Vec<u8>>,
        capture_error: Option<String>,
    ) {
        let Some(finalization) = self.recording.replay_finalization.take() else {
            return;
        };
        let ReplayFinalizationState::CapturingFinalState {
            mut recorder,
            frame_count,
        } = finalization
        else {
            self.recording.replay_finalization = Some(finalization);
            return;
        };

        if !self.recording.pending_replay_batches.is_empty() {
            log::warn!(
                "Replay finalizing with {} uncommitted replay input batch(es); dropping them",
                self.recording.pending_replay_batches.len()
            );
        }
        self.clear_replay_progress();

        let (sender, receiver) = std::sync::mpsc::channel();
        let active_system = self.active_system;
        std::thread::spawn(move || {
            if let Some(mut bytes) = final_state_bytes {
                crate::emu_backend::canonicalize_state_bytes_for_replay_hash(
                    active_system,
                    &mut bytes,
                );
                recorder.set_final_state_sha256(zeff_firmware::sha256_bytes(&bytes));
            }
            if let Some(err) = capture_error {
                log::warn!("Saving replay without final state hash: {err}");
            }
            let result = recorder.finish().map_err(|err| err.to_string());
            let _ = sender.send(ReplaySaveResult {
                frame_count,
                result,
            });
        });

        self.recording.replay_finalization = Some(ReplayFinalizationState::Saving {
            frame_count,
            receiver,
        });
    }

    pub(in crate::app) fn poll_replay_save_worker(&mut self) {
        let result = match self.recording.replay_finalization.as_mut() {
            Some(ReplayFinalizationState::Saving {
                frame_count,
                receiver,
            }) => match receiver.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(ReplaySaveResult {
                    frame_count: *frame_count,
                    result: Err("replay save worker stopped".to_string()),
                }),
            },
            _ => None,
        };

        if let Some(result) = result {
            self.finish_replay_save_result(result);
        }
    }

    pub(in crate::app) fn wait_for_replay_finalization_on_shutdown(&mut self) {
        while self.recording.replay_finalization.is_some() {
            if let Some(ReplayFinalizationState::Saving { .. }) =
                self.recording.replay_finalization.as_ref()
            {
                let result = match self.recording.replay_finalization.take() {
                    Some(ReplayFinalizationState::Saving {
                        frame_count,
                        receiver,
                    }) => receiver.recv().unwrap_or_else(|_| ReplaySaveResult {
                        frame_count,
                        result: Err("replay save worker stopped".to_string()),
                    }),
                    other => {
                        self.recording.replay_finalization = other;
                        continue;
                    }
                };
                self.finish_replay_save_result(result);
                continue;
            }

            while let Some(result) = self.emu_thread.as_ref().and_then(|t| t.try_recv_frame()) {
                self.process_frame_result(result);
            }
            let response = match self.emu_thread.as_ref().and_then(|t| t.recv()) {
                Some(response) => response,
                None => {
                    log::warn!("Replay finalization abandoned: emulator thread stopped");
                    self.recording.replay_finalization = None;
                    self.toast_manager.set_replay_saving(false);
                    break;
                }
            };
            if self.handle_link_response(&response) {
                continue;
            }
            match self.consume_replay_finalization_response(response) {
                None => continue,
                Some(response) => {
                    log::debug!(
                        "Ignoring unexpected response while finalizing replay on shutdown: {:?}",
                        response_kind(&response)
                    );
                }
            }
        }
    }

    fn finish_replay_save_result(&mut self, result: ReplaySaveResult) {
        self.recording.replay_finalization = None;
        self.toast_manager.set_replay_saving(false);

        match result.result {
            Ok(path) => {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                log::info!(
                    "Replay saved to {} ({} frames)",
                    path.display(),
                    result.frame_count
                );
                self.toast_manager
                    .success(format!("Saved {name} ({} frames)", result.frame_count));
            }
            Err(err) => {
                log::error!("Failed to save replay: {}", err);
                self.toast_manager
                    .error(format!("Replay save failed: {err}"));
            }
        }
    }
}

fn validate_replay_host_input_frame_shapes(
    player: &zeff_emu_common::replay::ReplayPlayer,
) -> anyhow::Result<()> {
    for (frame_index, len) in player.host_camera_frame_lengths() {
        if len != zeff_emu_common::replay::POCKET_CAMERA_FRAME_BYTES {
            anyhow::bail!(
                "replay Pocket Camera frame {frame_index} has {len} bytes, expected {}",
                zeff_emu_common::replay::POCKET_CAMERA_FRAME_BYTES
            );
        }
    }
    Ok(())
}

fn response_kind(response: &EmuResponse) -> &'static str {
    match response {
        EmuResponse::SaveStateOk(_) => "SaveStateOk",
        EmuResponse::SaveStateFailed(_) => "SaveStateFailed",
        EmuResponse::LoadStateOk { .. } => "LoadStateOk",
        EmuResponse::LoadStateFailed(_) => "LoadStateFailed",
        EmuResponse::RewindOk => "RewindOk",
        EmuResponse::RewindFailed(_) => "RewindFailed",
        EmuResponse::StateCaptured(_) => "StateCaptured",
        EmuResponse::ReplayStartCaptured(_) => "ReplayStartCaptured",
        EmuResponse::StateCaptureFailed(_) => "StateCaptureFailed",
        EmuResponse::FdsDiskSideChanged(_) => "FdsDiskSideChanged",
        EmuResponse::FdsDiskSideChangeFailed(_) => "FdsDiskSideChangeFailed",
        #[cfg(not(target_arch = "wasm32"))]
        EmuResponse::LinkPending(_) => "LinkPending",
        #[cfg(not(target_arch = "wasm32"))]
        EmuResponse::LinkConnected { .. } => "LinkConnected",
        #[cfg(not(target_arch = "wasm32"))]
        EmuResponse::LinkFailed(_) => "LinkFailed",
        #[cfg(not(target_arch = "wasm32"))]
        EmuResponse::LinkDisconnected { .. } => "LinkDisconnected",
        EmuResponse::SramFlushed(_) => "SramFlushed",
        EmuResponse::ShutdownComplete => "ShutdownComplete",
    }
}
