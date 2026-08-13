use super::App;
use crate::emu_thread::{EmuCommand, EmuResponse};
use zeff_firmware::sha256_bytes;

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
            self.disable_uncapped_for_replay();
            self.clear_replay_progress();

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
                thread.send(EmuCommand::CaptureReplayStart);
            }
            match self.recv_cold_response() {
                Some(EmuResponse::ReplayStartCaptured(start)) => {
                    let mut metadata = start.metadata;
                    metadata.cheat_sha256 = crate::cheats::enabled_patch_hash(
                        &self.debug_windows.cheat.user_codes,
                        &self.debug_windows.cheat.libretro_codes,
                    );
                    let recorder = zeff_emu_common::replay::ReplayRecorder::new_with_metadata(
                        path,
                        start.state_bytes,
                        metadata,
                    );
                    self.recording.replay_recording_base_frame = start.frame_count;
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
        if self.recording.replay_recorder.is_some() {
            self.toast_manager.set_replay_recording(false);
            while let Some(result) = self.emu_thread.as_ref().and_then(|t| t.try_recv_frame()) {
                self.process_frame_result(result);
            }
            if let Some(thread) = &self.emu_thread {
                thread.send(EmuCommand::CaptureStateBytes);
            }
            let mut final_state_hash = None;
            match self.recv_cold_response() {
                Some(EmuResponse::StateCaptured(bytes)) => {
                    final_state_hash = Some(sha256_bytes(&bytes));
                }
                Some(EmuResponse::StateCaptureFailed(err)) => {
                    log::warn!("Replay final state hash capture failed: {err}");
                }
                Some(other) => {
                    log::debug!(
                        "Unexpected response while capturing replay final state: {:?}",
                        response_kind(&other)
                    );
                }
                None => {}
            }
            let Some(mut recorder) = self.recording.replay_recorder.take() else {
                return;
            };
            if let Some(hash) = final_state_hash {
                recorder.set_final_state_sha256(hash);
            }
            self.clear_replay_progress();
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
        self.recording.pending_replay_batches.clear();
        self.recording.queued_replay_playback_frames = 0;
        self.recording.replay_recording_base_frame = 0;
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
        EmuResponse::LinkConnected(_) => "LinkConnected",
        #[cfg(not(target_arch = "wasm32"))]
        EmuResponse::LinkFailed(_) => "LinkFailed",
        #[cfg(not(target_arch = "wasm32"))]
        EmuResponse::LinkDisconnected => "LinkDisconnected",
        EmuResponse::SramFlushed(_) => "SramFlushed",
        EmuResponse::ShutdownComplete => "ShutdownComplete",
    }
}
