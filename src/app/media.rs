use super::App;
use super::types::PendingMediaCommand;
use crate::emu_thread::{EmuCommand, EmuResponse, TasControlCommandKind};
use zeff_emu_common::media::{MediaEvent, MediaSlotId};
use zeff_emu_common::replay::ReplayEvent;

const FDS_DRIVE_SLOT_ID: &str = "fds.drive0";

fn commit_pending_media_command(
    pending: &mut std::collections::VecDeque<PendingMediaCommand>,
    command: PendingMediaCommand,
    sent: bool,
) {
    if sent {
        pending.push_back(command);
    }
}

impl App {
    pub(super) fn consume_media_response(&mut self, response: EmuResponse) -> Option<EmuResponse> {
        match response {
            EmuResponse::MediaEventApplied {
                event,
                snapshot,
                frame_count,
            } => {
                self.media_slot_snapshot = Some(snapshot);
                if self.recording.replay_media_events_pending > 0 {
                    self.recording.replay_media_events_pending -= 1;
                    log::debug!("Replay applied media event: {event:?}");
                    return None;
                }
                let pending = self.recording.pending_media_commands.pop_front();
                if let Some(pending) = pending {
                    if pending.event != event {
                        log::warn!(
                            "Media command acknowledgement mismatch: expected {:?}, got {:?}",
                            pending.event,
                            event
                        );
                    }
                    if let Some(origin_frame) = pending.replay_origin_frame
                        && let Some(recorder) = self.recording.replay_recorder_for_commits()
                    {
                        match frame_count.checked_sub(origin_frame) {
                            Some(frame) => recorder.record_media_event(frame, event.clone()),
                            None => log::warn!(
                                "Dropping media event applied at frame {frame_count} before replay origin {origin_frame}"
                            ),
                        }
                    }
                }
                self.toast_manager
                    .success(media_event_success_label(&event));
                None
            }
            EmuResponse::MediaEventFailed { event, error } => {
                if self.recording.replay_media_events_pending > 0 {
                    self.abort_replay_playback_after_command_failure(format!(
                        "media event failed ({event:?}): {error}"
                    ));
                } else {
                    let _ = self.recording.pending_media_commands.pop_front();
                    self.toast_manager
                        .error(format!("Media event failed ({event:?}): {error}"));
                }
                None
            }
            response => Some(response),
        }
    }

    pub(super) fn set_fds_disk_side(&mut self, side: u8) {
        let Some(snapshot) = self.media_slot_snapshot.as_ref() else {
            self.toast_manager.info("Load FDS content first");
            return;
        };
        if snapshot.inserted() && snapshot.state.side == Some(side) {
            return;
        }

        let slot = snapshot.state.slot.clone();
        let event = if snapshot.inserted() {
            MediaEvent::SelectSide { slot, side }
        } else {
            let Some(media_id) = snapshot.source_media_id.clone() else {
                self.toast_manager.error("FDS source media is unavailable");
                return;
            };
            MediaEvent::Insert {
                slot,
                media_id,
                side: Some(side),
                write_protected: false,
            }
        };
        self.request_media_event(event);
    }

    pub(super) fn request_media_event(&mut self, event: MediaEvent) {
        if let Err(error) =
            self.preflight_emu_command_kind(TasControlCommandKind::MediaOrPeripheral)
        {
            self.toast_manager.error(error.to_string());
            return;
        }
        if self.recording.replay_player.is_some() {
            self.toast_manager
                .info("Replay playback controls removable media");
            return;
        }
        if self.media_slot_snapshot.is_none() {
            self.toast_manager
                .info("Load removable media content first");
            return;
        }

        let replay_origin_frame = self
            .recording
            .replay_recorder
            .as_ref()
            .map(|_| self.recording.replay_recording_origin.frame);
        if let Err(error) =
            self.send_emu_command_checked(EmuCommand::ApplyMediaEvent(event.clone()))
        {
            self.toast_manager.error(error.to_string());
            return;
        }
        commit_pending_media_command(
            &mut self.recording.pending_media_commands,
            PendingMediaCommand {
                replay_origin_frame,
                event,
            },
            true,
        );
    }

    pub(super) fn apply_replay_events_at_cursor(&mut self) {
        if let Err(error) =
            self.preflight_emu_command_kind(TasControlCommandKind::MediaOrPeripheral)
        {
            self.abort_replay_playback_after_command_failure(error.to_string());
            return;
        }
        let Some(player) = self.recording.replay_player.as_mut() else {
            return;
        };
        let mut failed = None;
        for event in player.take_events_at_cursor() {
            match event {
                ReplayEvent::FdsDiskSide { side, .. } => {
                    if let Err(error) = self.send_replay_media_event(MediaEvent::SelectSide {
                        slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
                        side,
                    }) {
                        failed = Some(error.to_string());
                        break;
                    }
                }
                ReplayEvent::Media { event, .. } => {
                    if let Err(error) = self.send_replay_media_event(event) {
                        failed = Some(error.to_string());
                        break;
                    }
                }
                ReplayEvent::GameBoyLinkState { state, .. } => {
                    if let Err(error) =
                        self.send_emu_command_checked(EmuCommand::RestoreGameBoyLinkState(state))
                    {
                        failed = Some(error.to_string());
                        break;
                    }
                }
                ReplayEvent::GameBoyLinkStateAtTick { .. } => {}
                ReplayEvent::GameBoyLink { .. } | ReplayEvent::WonderSwanLink { .. } => {}
            }
        }
        if let Some(error) = failed {
            self.abort_replay_playback_after_command_failure(error);
        }
    }

    fn send_replay_media_event(
        &mut self,
        event: MediaEvent,
    ) -> Result<(), super::command_gate::EmuCommandSendError> {
        self.send_emu_command_checked(EmuCommand::ApplyMediaEvent(event))?;
        self.recording.replay_media_events_pending += 1;
        Ok(())
    }
}

fn media_event_success_label(event: &MediaEvent) -> String {
    match event {
        MediaEvent::Insert { side, .. } => side.map_or_else(
            || "Media inserted".to_string(),
            |side| format!("FDS {} inserted", fds_side_label(side)),
        ),
        MediaEvent::Eject { .. } => "FDS disk ejected".to_string(),
        MediaEvent::SelectSide { side, .. } => {
            format!("FDS {} selected", fds_side_label(*side))
        }
        MediaEvent::SetWriteProtected {
            write_protected, ..
        } => {
            if *write_protected {
                "FDS disk is write-protected".to_string()
            } else {
                "FDS disk is writable".to_string()
            }
        }
    }
}

pub(super) fn fds_side_label(side: u8) -> String {
    let disk = usize::from(side) / 2 + 1;
    let face = if side.is_multiple_of(2) { 'A' } else { 'B' };
    format!("Disk {disk}, Side {face}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_pending_queue_changes_only_after_send() {
        let event = MediaEvent::Eject {
            slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
        };
        let mut pending = std::collections::VecDeque::new();
        commit_pending_media_command(
            &mut pending,
            PendingMediaCommand {
                replay_origin_frame: Some(4),
                event: event.clone(),
            },
            false,
        );
        assert!(pending.is_empty());
        commit_pending_media_command(
            &mut pending,
            PendingMediaCommand {
                replay_origin_frame: Some(4),
                event,
            },
            true,
        );
        assert_eq!(pending.len(), 1);
    }
}
