use super::App;
use super::types::PendingMediaCommand;
use crate::emu_thread::{EmuCommand, EmuResponse};
use zeff_emu_common::media::{MediaEvent, MediaSlotId};
use zeff_emu_common::replay::ReplayEvent;

const FDS_DRIVE_SLOT_ID: &str = "fds.drive0";

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
                    self.recording.replay_media_events_pending -= 1;
                    self.recording.replay_player = None;
                } else {
                    let _ = self.recording.pending_media_commands.pop_front();
                }
                self.toast_manager
                    .error(format!("Media event failed ({event:?}): {error}"));
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
        let Some(thread) = &self.emu_thread else {
            self.toast_manager.error("No game running");
            return;
        };
        thread.send(EmuCommand::ApplyMediaEvent(event.clone()));
        self.recording
            .pending_media_commands
            .push_back(PendingMediaCommand {
                replay_origin_frame,
                event,
            });
    }

    pub(super) fn apply_replay_events_at_cursor(&mut self) {
        let Some(player) = self.recording.replay_player.as_mut() else {
            return;
        };
        for event in player.take_events_at_cursor() {
            match event {
                ReplayEvent::FdsDiskSide { side, .. } => {
                    self.send_replay_media_event(MediaEvent::SelectSide {
                        slot: MediaSlotId::from(FDS_DRIVE_SLOT_ID),
                        side,
                    });
                }
                ReplayEvent::Media { event, .. } => self.send_replay_media_event(event),
                ReplayEvent::GameBoyLinkState { state, .. } => {
                    if let Some(thread) = &self.emu_thread {
                        thread.send(EmuCommand::RestoreGameBoyLinkState(state));
                    }
                }
                ReplayEvent::GameBoyLinkStateAtTick { .. } => {}
                ReplayEvent::GameBoyLink { .. } | ReplayEvent::WonderSwanLink { .. } => {}
            }
        }
    }

    fn send_replay_media_event(&mut self, event: MediaEvent) {
        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::ApplyMediaEvent(event));
            self.recording.replay_media_events_pending += 1;
        }
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
