#[cfg(not(target_arch = "wasm32"))]
use super::super::types::{PendingReplayStart, ReplayFinalizationState};
use super::App;
use crate::app::command_gate::EmuCommandSendError;
#[cfg(not(target_arch = "wasm32"))]
use crate::emu_thread::ReplayStartState;
use crate::emu_thread::{EmuCommand, EmuResponse, TasControlCommandKind};
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;

const REPLAY_CHECKPOINT_INTERVAL_FRAMES: usize = 300;

#[cfg(not(target_arch = "wasm32"))]
mod finalization;
#[cfg(test)]
mod tests;

#[cfg(not(target_arch = "wasm32"))]
fn replay_capture_id_reservation(current: u64) -> Option<(u64, u64)> {
    Some((current, current.checked_add(1)?))
}

#[cfg(not(target_arch = "wasm32"))]
fn commit_pending_replay_start(
    slot: &mut Option<PendingReplayStart>,
    next_capture_id: &mut u64,
    pending: PendingReplayStart,
    reserved_next: u64,
    sent: bool,
) {
    if sent {
        *next_capture_id = reserved_next;
        *slot = Some(pending);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn finish_pending_replay_cancellation(
    pending: &mut Option<PendingReplayStart>,
    resume_result: Result<(), EmuCommandSendError>,
) -> Result<(), EmuCommandSendError> {
    *pending = None;
    resume_result
}

fn commit_checkpoint_marker(marker: &mut usize, frame: u64, sent: bool) {
    if sent {
        *marker = frame as usize;
    }
}

fn commit_pending_checkpoint(
    pending: &mut std::collections::BTreeMap<u64, [u8; 32]>,
    frame: u64,
    hash: [u8; 32],
    sent: bool,
) {
    if sent {
        pending.insert(frame, hash);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReplayStopPostCapture {
    AwaitFinalState,
    SaveWithoutFinalState(EmuCommandSendError),
}

fn replay_stop_post_capture(
    resume_result: Result<(), EmuCommandSendError>,
) -> ReplayStopPostCapture {
    match resume_result {
        Ok(()) => ReplayStopPostCapture::AwaitFinalState,
        Err(error) => ReplayStopPostCapture::SaveWithoutFinalState(error),
    }
}

impl App {
    pub(in crate::app) fn start_replay_recording(&mut self) {
        if !self.core_supports_replay() {
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
            if let Err(error) = self.preflight_emu_command_kind(TasControlCommandKind::Replay) {
                self.toast_manager.error(error.to_string());
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

            self.pause_for_dialog();
            let file = crate::platform::FileDialog::new()
                .set_title("Save Replay")
                .set_directory(self.state_dialog_dir())
                .add_filter("Zeff Boy Replay", &["zrpl"])
                .set_file_name(&default_name)
                .save_file();

            self.resume_after_dialog();
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
        if !self.core_supports_replay() {
            anyhow::bail!("the active core does not support replay capture");
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

        self.preflight_emu_command_kind(TasControlCommandKind::Replay)?;
        let (capture_id, next_capture_id) =
            replay_capture_id_reservation(self.recording.next_replay_capture_id)
                .ok_or_else(|| anyhow::anyhow!("replay capture ID exhausted"))?;

        if self.timing.uncapped_speed {
            self.send_emu_command_checked(EmuCommand::SetUncapped(false))?;
        }
        if let Err(error) =
            self.send_emu_command_checked(EmuCommand::CaptureReplayStart { capture_id })
        {
            self.resume_uncapped_worker_after_replay();
            return Err(error.into());
        }

        self.clear_replay_progress();
        commit_pending_replay_start(
            &mut self.recording.pending_replay_start,
            &mut self.recording.next_replay_capture_id,
            PendingReplayStart { path, capture_id },
            next_capture_id,
            true,
        );
        self.resume_if_paused_by_unfocus_for_link();
        self.timing.last_frame_time = crate::platform::Instant::now();
        self.toast_manager.info("Replay recording starting");
        Ok(())
    }

    pub(in crate::app) fn stop_replay_recording(&mut self) -> Result<(), EmuCommandSendError> {
        self.preflight_emu_command_kind(TasControlCommandKind::Replay)?;
        #[cfg(not(target_arch = "wasm32"))]
        if self.recording.pending_replay_start.is_some() {
            let resume_result = self.try_resume_uncapped_worker_after_replay();
            let resume_result = finish_pending_replay_cancellation(
                &mut self.recording.pending_replay_start,
                resume_result,
            );
            self.clear_replay_progress();
            self.toast_manager.set_replay_recording(false);
            self.timing.last_frame_time = crate::platform::Instant::now();
            resume_result?;
            self.toast_manager.info("Replay start canceled");
            return Ok(());
        }

        if self.recording.replay_recorder.is_some() {
            while let Some(result) = self.emu_thread.as_ref().and_then(|t| t.try_recv_frame()) {
                self.process_frame_result(result);
            }
            self.send_emu_command_checked(EmuCommand::CaptureStateBytes)?;
            self.toast_manager.set_replay_recording(false);
            self.timing.last_frame_time = crate::platform::Instant::now();
            let Some(recorder) = self.recording.replay_recorder.take() else {
                return Ok(());
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
                if let ReplayStopPostCapture::SaveWithoutFinalState(error) =
                    replay_stop_post_capture(self.try_resume_uncapped_worker_after_replay())
                {
                    self.start_replay_save_worker(
                        None,
                        Some(format!(
                            "emulator command failed after state capture: {error}"
                        )),
                    );
                    return Err(error);
                }
            }

            #[cfg(target_arch = "wasm32")]
            {
                drop(recorder);
                self.clear_replay_progress();
                self.try_resume_uncapped_worker_after_replay()?;
            }
        }
        Ok(())
    }

    pub(in crate::app) fn stop_replay_recording_for_teardown(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.recording.pending_replay_start = None;
        }
        self.recording.replay_player = None;
        let Some(recorder) = self.recording.replay_recorder.take() else {
            self.clear_replay_progress();
            return;
        };
        let frame_count = recorder.frame_count();
        self.toast_manager.set_replay_recording(false);

        #[cfg(not(target_arch = "wasm32"))]
        {
            self.recording.replay_finalization =
                Some(ReplayFinalizationState::CapturingFinalState {
                    recorder: Box::new(recorder),
                    frame_count,
                });
            self.toast_manager.set_replay_saving(true);
            self.start_replay_save_worker(
                None,
                Some("emulator worker teardown prevented final state capture".to_string()),
            );
        }
        #[cfg(target_arch = "wasm32")]
        {
            drop(recorder);
            self.clear_replay_progress();
        }
    }

    pub(in crate::app) fn load_and_play_replay(&mut self) {
        if !self.core_supports_replay() {
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
            if let Err(error) = self.preflight_emu_command_kind(TasControlCommandKind::Replay) {
                self.toast_manager.error(error.to_string());
                return;
            }

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
                    if self.timing.uncapped_speed
                        && let Err(error) =
                            self.send_emu_command_checked(EmuCommand::SetUncapped(false))
                    {
                        self.toast_manager.error(error.to_string());
                        return;
                    }
                    let command = EmuCommand::LoadStateBytes {
                        state_bytes,
                        buttons_pressed: 0,
                        dpad_pressed: 0,
                        replay_events: Some(player.metadata().events.clone()),
                        game_boy_link_start_state: player.metadata().game_boy_link_start_state,
                        game_boy_link_coordinator_start_state: player
                            .metadata()
                            .game_boy_link_coordinator_start_state,
                        game_boy_link_start_tick: player.metadata().game_boy_link_start_tick,
                        wonder_swan_link_start_tick: player.metadata().wonder_swan_link_start_tick,
                    };
                    if let Err(error) = self.send_emu_command_checked(command) {
                        self.resume_uncapped_worker_after_replay();
                        self.toast_manager.error(error.to_string());
                        return;
                    }
                    match self.recv_cold_response() {
                        Some(EmuResponse::LoadStateOk {
                            media_slot_snapshot,
                            game_boy_serial_device,
                            ..
                        }) => {
                            self.media_slot_snapshot = media_slot_snapshot;
                            if let Some(device) = game_boy_serial_device {
                                self.game_boy_serial_device = device;
                            }
                            if let Some(thread) = &self.emu_thread {
                                self.latest_frame = thread.shared_framebuffer().load_full();
                            }
                            self.clear_replay_progress();
                            self.recording.replay_player = Some(player);
                            self.toast_manager
                                .info(format!("Playing replay ({total} frames)"));
                        }
                        Some(EmuResponse::LoadStateFailed(err)) => {
                            self.resume_uncapped_worker_after_replay();
                            log::error!("Failed to load replay state: {}", err);
                            self.toast_manager
                                .error(format!("Replay load failed: {err}"));
                        }
                        _ => self.resume_uncapped_worker_after_replay(),
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

    fn try_resume_uncapped_worker_after_replay(&mut self) -> Result<(), EmuCommandSendError> {
        if self.timing.uncapped_speed {
            self.send_emu_command_checked(EmuCommand::SetUncapped(true))?;
        }
        Ok(())
    }

    pub(in crate::app) fn resume_uncapped_worker_after_replay(&mut self) {
        let _ = self.try_resume_uncapped_worker_after_replay();
    }

    pub(in crate::app) fn clear_replay_progress(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.recording.pending_replay_start = None;
        }
        self.recording.pending_replay_batches.clear();
        self.recording.queued_replay_playback_frames = 0;
        self.recording.replay_recording_origin = crate::app::types::ReplayCaptureOrigin::default();
        self.recording.replay_media_events_pending = 0;
        self.recording.last_replay_checkpoint_frame = 0;
        self.recording.pending_replay_checkpoint_hashes.clear();
    }

    pub(in crate::app) fn abort_replay_playback_after_command_failure(&mut self, error: String) {
        self.recording.replay_player = None;
        self.clear_replay_progress();
        self.resume_uncapped_worker_after_replay();
        self.toast_manager
            .error(format!("Replay playback stopped: {error}"));
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
        if !zeff_emu_common::replay::firmware_manifests_match(&metadata.firmware, &current.firmware)
        {
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
        if player.uses_game_boy_link()
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
            EmuResponse::ReplayStartCaptured { capture_id, start } => {
                if !self.recording.replay_start_matches(capture_id) {
                    log::warn!("Ignoring stale replay start capture response {capture_id}");
                    return None;
                }
                let pending = self
                    .recording
                    .pending_replay_start
                    .take()
                    .expect("matching replay start should be pending");
                if let Err(err) = self.finish_pending_replay_start(start, pending.path) {
                    log::error!("Failed to start replay recording: {err}");
                    self.clear_replay_progress();
                    self.resume_uncapped_worker_after_replay();
                    self.toast_manager
                        .error(format!("Replay start failed: {err}"));
                }
                None
            }
            EmuResponse::ReplayStartCaptureFailed { capture_id, error } => {
                if !self.recording.replay_start_matches(capture_id) {
                    log::warn!("Ignoring stale replay start capture failure {capture_id}");
                    return None;
                }
                self.recording.pending_replay_start = None;
                self.clear_replay_progress();
                self.resume_uncapped_worker_after_replay();
                self.timing.last_frame_time = crate::platform::Instant::now();
                let message = format!("failed to capture replay start state: {error}");
                log::error!("{message}");
                self.toast_manager
                    .error(format!("Replay start failed: {error}"));
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
        metadata.wonder_swan_link_start_tick = start.wonder_swan_cpu_cycles;
        let recorder = zeff_emu_common::replay::ReplayRecorder::new_with_metadata(
            path,
            start.state_bytes,
            metadata,
        );
        self.recording.replay_recording_origin = crate::app::types::ReplayCaptureOrigin {
            frame: start.frame_count,
            game_boy_tick: start.game_boy_cpu_cycles,
            wonder_swan_tick: start.wonder_swan_cpu_cycles,
        };
        self.recording.replay_recorder = Some(recorder);
        self.recording.last_replay_checkpoint_frame = 0;
        self.timing.last_frame_time = crate::platform::Instant::now();
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

    pub(in crate::app) fn schedule_replay_checkpoint(&mut self) {
        if let Some(recorder) = self.recording.replay_recorder.as_ref() {
            let frame = recorder.frame_count();
            if frame > 0
                && frame.is_multiple_of(REPLAY_CHECKPOINT_INTERVAL_FRAMES)
                && frame > self.recording.last_replay_checkpoint_frame
                && let Ok(frame) = u64::try_from(frame)
            {
                let result =
                    self.send_emu_command_checked(EmuCommand::CaptureReplayCheckpoint { frame });
                let sent = result.is_ok();
                commit_checkpoint_marker(
                    &mut self.recording.last_replay_checkpoint_frame,
                    frame,
                    sent,
                );
                if let Err(error) = result {
                    self.stop_replay_recording_for_teardown();
                    self.toast_manager
                        .error(format!("Replay recording stopped: {error}"));
                    return;
                }
            }
        }
        let Some(player) = self.recording.replay_player.as_ref() else {
            return;
        };
        let frame = u64::try_from(player.cursor()).unwrap_or(u64::MAX);
        let Some(expected_hash) = player
            .metadata()
            .checkpoints
            .iter()
            .find(|checkpoint| checkpoint.frame == frame)
            .map(|checkpoint| checkpoint.state_sha256)
        else {
            return;
        };
        if self
            .recording
            .pending_replay_checkpoint_hashes
            .contains_key(&frame)
        {
            return;
        }
        let result = self.send_emu_command_checked(EmuCommand::CaptureReplayCheckpoint { frame });
        let sent = result.is_ok();
        commit_pending_checkpoint(
            &mut self.recording.pending_replay_checkpoint_hashes,
            frame,
            expected_hash,
            sent,
        );
        if let Err(error) = result {
            self.abort_replay_playback_after_command_failure(error.to_string());
        }
    }

    pub(in crate::app) fn consume_replay_checkpoint_response(
        &mut self,
        response: EmuResponse,
    ) -> Option<EmuResponse> {
        let game_boy_link_replay = self
            .recording
            .replay_player
            .as_ref()
            .is_some_and(zeff_emu_common::replay::ReplayPlayer::uses_game_boy_link);
        match response {
            EmuResponse::ReplayCheckpointCaptured {
                frame,
                mut state_bytes,
            } => {
                if let Err(error) = crate::emu_backend::canonicalize_state_bytes_for_replay_hash(
                    self.active_system,
                    &mut state_bytes,
                ) {
                    return self.consume_replay_checkpoint_response(
                        EmuResponse::ReplayCheckpointCaptureFailed {
                            frame,
                            error: error.to_string(),
                        },
                    );
                }
                let hash = zeff_firmware::sha256_bytes(&state_bytes);
                if let Some(expected) = self
                    .recording
                    .pending_replay_checkpoint_hashes
                    .remove(&frame)
                {
                    if hash != expected {
                        if game_boy_link_replay {
                            log::warn!("Game Boy link replay diverged at frame {frame}");
                        } else {
                            self.recording.replay_player = None;
                            self.clear_replay_progress();
                            self.resume_uncapped_worker_after_replay();
                            self.toast_manager
                                .error(format!("Replay diverged at frame {frame}"));
                            return None;
                        }
                    }
                    if self
                        .recording
                        .replay_player
                        .as_ref()
                        .is_some_and(zeff_emu_common::replay::ReplayPlayer::is_finished)
                    {
                        self.recording.replay_player = None;
                        self.clear_replay_progress();
                        self.resume_uncapped_worker_after_replay();
                        self.toast_manager.info("Replay finished");
                    }
                } else if let Some(recorder) = self.recording.replay_recorder_for_commits() {
                    recorder.record_checkpoint(frame, hash);
                }
                None
            }
            EmuResponse::ReplayCheckpointCaptureFailed { frame, error } => {
                if self
                    .recording
                    .pending_replay_checkpoint_hashes
                    .remove(&frame)
                    .is_some()
                {
                    if game_boy_link_replay {
                        log::warn!(
                            "Game Boy link replay checkpoint failed at frame {frame}: {error}"
                        );
                        if self
                            .recording
                            .replay_player
                            .as_ref()
                            .is_some_and(zeff_emu_common::replay::ReplayPlayer::is_finished)
                        {
                            self.recording.replay_player = None;
                            self.clear_replay_progress();
                            self.resume_uncapped_worker_after_replay();
                            self.toast_manager.info("Replay finished");
                        }
                    } else {
                        self.recording.replay_player = None;
                        self.clear_replay_progress();
                        self.resume_uncapped_worker_after_replay();
                        self.toast_manager
                            .error(format!("Replay checkpoint failed at frame {frame}"));
                    }
                } else {
                    log::warn!("Replay checkpoint capture failed at frame {frame}: {error}");
                }
                None
            }
            response => Some(response),
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
