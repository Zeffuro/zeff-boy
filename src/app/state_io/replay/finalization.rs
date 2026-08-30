use super::App;
use crate::app::types::{ReplayFinalizationState, ReplaySaveResult};
use crate::emu_thread::EmuResponse;

impl App {
    pub(super) fn start_replay_save_worker(
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
                match crate::emu_backend::canonicalize_state_bytes_for_replay_hash(
                    active_system,
                    &mut bytes,
                ) {
                    Ok(()) => {
                        recorder.set_final_state_sha256(zeff_firmware::sha256_bytes(&bytes));
                    }
                    Err(error) => {
                        log::warn!("Saving replay without final state hash: {error}");
                    }
                }
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

fn response_kind(response: &EmuResponse) -> &'static str {
    match response {
        EmuResponse::SaveStateOk { .. } => "SaveStateOk",
        EmuResponse::SaveStateFailed(_) => "SaveStateFailed",
        EmuResponse::LoadStateOk { .. } => "LoadStateOk",
        EmuResponse::LoadStateFailed(_) => "LoadStateFailed",
        EmuResponse::RewindOk { .. } => "RewindOk",
        EmuResponse::RewindFailed(_) => "RewindFailed",
        EmuResponse::StateCaptured(_) => "StateCaptured",
        EmuResponse::ReplayStartCaptured { .. } => "ReplayStartCaptured",
        EmuResponse::ReplayStartCaptureFailed { .. } => "ReplayStartCaptureFailed",
        EmuResponse::ReplayCheckpointCaptured { .. } => "ReplayCheckpointCaptured",
        EmuResponse::ReplayCheckpointCaptureFailed { .. } => "ReplayCheckpointCaptureFailed",
        EmuResponse::StateCaptureFailed(_) => "StateCaptureFailed",
        EmuResponse::GuestCallCompleted { .. } => "GuestCallCompleted",
        EmuResponse::GuestCallFailed { .. } => "GuestCallFailed",
        EmuResponse::GuestCallUndone => "GuestCallUndone",
        EmuResponse::GuestCallUndoFailed(_) => "GuestCallUndoFailed",
        EmuResponse::MediaEventApplied { .. } => "MediaEventApplied",
        EmuResponse::MediaEventFailed { .. } => "MediaEventFailed",
        EmuResponse::BardigunBarcodeScanStarted(_) => "BardigunBarcodeScanStarted",
        EmuResponse::BardigunBarcodeScanFailed(_) => "BardigunBarcodeScanFailed",
        EmuResponse::BarcodeBoyScanStarted => "BarcodeBoyScanStarted",
        EmuResponse::BarcodeBoyScanFailed(_) => "BarcodeBoyScanFailed",
        EmuResponse::LinkPending(_) => "LinkPending",
        EmuResponse::LinkConnected { .. } => "LinkConnected",
        EmuResponse::LinkFailed(_) => "LinkFailed",
        EmuResponse::LinkDisconnected { .. } => "LinkDisconnected",
        EmuResponse::TasControlAcquired { .. } => "TasControlAcquired",
        EmuResponse::TasControlAcquireRejected { .. } => "TasControlAcquireRejected",
        EmuResponse::TasExecutionCompleted { .. } => "TasExecutionCompleted",
        EmuResponse::TasExecutionRejected { .. } => "TasExecutionRejected",
        EmuResponse::TasFrameAdvanced { .. } => "TasFrameAdvanced",
        EmuResponse::TasFrameAdvanceRejected { .. } => "TasFrameAdvanceRejected",
        EmuResponse::TasControlCommandRejected { .. } => "TasControlCommandRejected",
        EmuResponse::TasControlRolledBack { .. } => "TasControlRolledBack",
        EmuResponse::TasControlRollbackRejected { .. } => "TasControlRollbackRejected",
        EmuResponse::TasControlCommitted { .. } => "TasControlCommitted",
        EmuResponse::TasControlCommitRejected { .. } => "TasControlCommitRejected",
        EmuResponse::SramFlushed(_) => "SramFlushed",
        EmuResponse::SramFlushFailed(_) => "SramFlushFailed",
        EmuResponse::RecoveryMissing => "RecoveryMissing",
        EmuResponse::RecoveryAvailable(_) => "RecoveryAvailable",
        EmuResponse::RecoveryRejected(_) => "RecoveryRejected",
        EmuResponse::RecoverySaved(_) => "RecoverySaved",
        EmuResponse::RecoverySaveFailed(_) => "RecoverySaveFailed",
        EmuResponse::ShutdownComplete => "ShutdownComplete",
    }
}
