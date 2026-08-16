use super::App;
use crate::emu_backend::ActiveSystem;
use crate::emu_thread::EmuCommand;
use zeff_emu_common::replay::ReplayEvent;

impl App {
    pub(super) fn set_fds_disk_side(&mut self, side: u8) {
        if self.recording.replay_player.is_some() {
            self.toast_manager
                .info("Replay playback controls FDS side changes");
            return;
        }

        if self.active_system != ActiveSystem::Nes {
            self.toast_manager.info("Load FDS content first");
            return;
        }

        if let Some(thread) = &self.emu_thread {
            thread.send(EmuCommand::SetFdsDiskSide(side));
            self.record_replay_event(ReplayEvent::FdsDiskSide {
                frame: self.replay_recorded_frame_count(),
                side,
            });
        } else {
            self.toast_manager.error("No game running");
        }
    }

    pub(super) fn apply_replay_events_at_cursor(&mut self) {
        let Some(player) = self.recording.replay_player.as_mut() else {
            return;
        };
        for event in player.take_events_at_cursor() {
            match event {
                ReplayEvent::FdsDiskSide { side, .. } => {
                    if let Some(thread) = &self.emu_thread {
                        thread.send(EmuCommand::SetFdsDiskSide(side));
                        self.recording.replay_media_events_pending += 1;
                    }
                }
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

    fn record_replay_event(&mut self, event: ReplayEvent) {
        if let Some(recorder) = &mut self.recording.replay_recorder {
            recorder.record_event(event);
        }
    }

    fn replay_recorded_frame_count(&self) -> u64 {
        self.recording
            .replay_recorder
            .as_ref()
            .map(|recorder| recorder.frame_count() as u64)
            .unwrap_or(0)
    }
}

pub(super) fn fds_side_label(side: u8) -> char {
    char::from(b'A'.saturating_add(side))
}
