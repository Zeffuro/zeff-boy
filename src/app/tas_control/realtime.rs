use std::time::Duration;

use crate::app::App;
use crate::platform::Instant;

const RESPONSE_POLL_INTERVAL: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RealtimePhase {
    #[default]
    Off,
    Suspended {
        frame_pending: bool,
    },
    Running {
        next_frame_at: Instant,
        frame_pending: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RealtimePoll {
    Idle,
    Record,
}

#[derive(Debug, Default)]
pub(in crate::app) struct TasRealtimeRecorder {
    phase: RealtimePhase,
}

#[derive(Debug, Default)]
pub(in crate::app) struct TasPlaybackScheduler {
    phase: RealtimePhase,
}

impl TasRealtimeRecorder {
    pub(super) fn reset(&mut self) {
        self.phase = RealtimePhase::Off;
    }

    pub(super) fn next_wake(&self, now: Instant) -> Option<Instant> {
        match self.phase {
            RealtimePhase::Running {
                next_frame_at,
                frame_pending: false,
            } => Some(next_frame_at),
            RealtimePhase::Running {
                frame_pending: true,
                ..
            }
            | RealtimePhase::Suspended {
                frame_pending: true,
            } => Some(now + RESPONSE_POLL_INTERVAL),
            _ => None,
        }
    }

    fn poll(
        &mut self,
        active: bool,
        input_owned: bool,
        can_record: bool,
        now: Instant,
        frame_duration: Duration,
    ) -> RealtimePoll {
        if !active {
            self.reset();
            return RealtimePoll::Idle;
        }

        if self.phase == RealtimePhase::Off {
            self.phase = if input_owned {
                RealtimePhase::Running {
                    next_frame_at: now + frame_duration,
                    frame_pending: false,
                }
            } else {
                RealtimePhase::Suspended {
                    frame_pending: false,
                }
            };
            return RealtimePoll::Idle;
        }

        match self.phase {
            RealtimePhase::Off => unreachable!(),
            RealtimePhase::Suspended { frame_pending } if input_owned => {
                self.phase = RealtimePhase::Running {
                    next_frame_at: now + frame_duration,
                    frame_pending: frame_pending && !can_record,
                };
            }
            RealtimePhase::Suspended { .. } => return RealtimePoll::Idle,
            RealtimePhase::Running { frame_pending, .. } if !input_owned => {
                self.phase = RealtimePhase::Suspended { frame_pending };
                return RealtimePoll::Idle;
            }
            RealtimePhase::Running {
                ref mut next_frame_at,
                ref mut frame_pending,
            } => {
                if *frame_pending {
                    if !can_record {
                        return RealtimePoll::Idle;
                    }
                    *frame_pending = false;
                    *next_frame_at = (*next_frame_at + frame_duration).max(now);
                }
                if can_record && now >= *next_frame_at {
                    return RealtimePoll::Record;
                }
            }
        }

        RealtimePoll::Idle
    }

    fn mark_sent(&mut self) {
        if let RealtimePhase::Running { frame_pending, .. } = &mut self.phase {
            *frame_pending = true;
        }
    }
}

impl TasPlaybackScheduler {
    pub(super) fn reset(&mut self) {
        self.phase = RealtimePhase::Off;
    }

    pub(super) fn next_wake(&self, now: Instant) -> Option<Instant> {
        match self.phase {
            RealtimePhase::Running {
                next_frame_at,
                frame_pending: false,
            } => Some(next_frame_at),
            RealtimePhase::Running {
                frame_pending: true,
                ..
            } => Some(now + RESPONSE_POLL_INTERVAL),
            _ => None,
        }
    }

    fn poll(
        &mut self,
        active: bool,
        can_advance: bool,
        now: Instant,
        frame_duration: Duration,
    ) -> RealtimePoll {
        if !active {
            self.reset();
            return RealtimePoll::Idle;
        }
        if self.phase == RealtimePhase::Off {
            self.phase = RealtimePhase::Running {
                next_frame_at: now + frame_duration,
                frame_pending: false,
            };
            return RealtimePoll::Idle;
        }
        let RealtimePhase::Running {
            ref mut next_frame_at,
            ref mut frame_pending,
        } = self.phase
        else {
            unreachable!("playback scheduling never suspends for input focus")
        };
        if *frame_pending {
            if !can_advance {
                return RealtimePoll::Idle;
            }
            *frame_pending = false;
            *next_frame_at = (*next_frame_at + frame_duration).max(now);
        }
        if can_advance && now >= *next_frame_at {
            RealtimePoll::Record
        } else {
            RealtimePoll::Idle
        }
    }

    fn mark_sent(&mut self) {
        if let RealtimePhase::Running { frame_pending, .. } = &mut self.phase {
            *frame_pending = true;
        }
    }
}

impl App {
    pub(in crate::app) fn start_linked_tas_playback(&mut self) -> anyhow::Result<()> {
        let session = self
            .debug_windows
            .tas_editor
            .active_session()
            .ok_or_else(|| anyhow::anyhow!("no TAS editor project is open"))?;
        self.tas_control.start_playback(session)?;
        self.tas_playback_scheduler.reset();
        Ok(())
    }

    pub(in crate::app) fn pause_linked_tas_playback(&mut self) {
        self.tas_control.pause_playback();
        self.tas_playback_scheduler.reset();
    }

    pub(in crate::app) fn linked_tas_playback_next_wake(&self, now: Instant) -> Option<Instant> {
        self.tas_playback_scheduler.next_wake(now)
    }

    pub(in crate::app) fn pump_linked_tas_playback(&mut self) {
        let active = self.tas_control.playback_active();
        let can_advance = self.tas_control.can_advance_playback();
        let now = Instant::now();
        let frame_duration = Duration::from_nanos(self.nominal_frame_duration_ns().max(1));
        match self
            .tas_playback_scheduler
            .poll(active, can_advance, now, frame_duration)
        {
            RealtimePoll::Idle => {}
            RealtimePoll::Record => {
                let command = self
                    .debug_windows
                    .tas_editor
                    .active_session()
                    .ok_or_else(|| anyhow::anyhow!("no TAS editor project is open"))
                    .and_then(|session| self.tas_control.begin_playback_frame(session));
                match command {
                    Ok(command) => {
                        self.send_tas_control_command(command);
                        self.tas_playback_scheduler.mark_sent();
                    }
                    Err(error) => {
                        self.pause_linked_tas_playback();
                        self.toast_manager
                            .error(format!("Could not play TAS movie input: {error:#}"));
                    }
                }
            }
        }
    }

    pub(in crate::app) fn start_realtime_tas_recording(&mut self) -> anyhow::Result<()> {
        self.tas_control.start_realtime_recording()?;
        if let Err(error) = self
            .debug_windows
            .tas_editor
            .begin_live_recording_history_group()
        {
            self.tas_control.stop_realtime_recording();
            return Err(error);
        }
        self.tas_realtime_recorder.reset();
        self.recompute_pause();
        Ok(())
    }

    pub(in crate::app) fn stop_realtime_tas_recording(&mut self) {
        self.tas_control.stop_realtime_recording();
        self.tas_realtime_recorder.reset();
        self.finish_realtime_tas_history_group_if_idle();
        self.recompute_pause();
    }

    pub(in crate::app) fn realtime_tas_recording_active(&self) -> bool {
        self.tas_control.realtime_recording_active()
    }

    pub(in crate::app) fn realtime_tas_recording_waiting_for_game_input(&self) -> bool {
        self.realtime_tas_recording_active() && !self.realtime_tas_game_input_owned()
    }

    pub(in crate::app) fn realtime_tas_recording_next_wake(&self, now: Instant) -> Option<Instant> {
        self.tas_realtime_recorder.next_wake(now)
    }

    pub(in crate::app) fn finish_realtime_tas_history_group_if_idle(&mut self) {
        if self.tas_control.realtime_recording_active()
            || self.tas_control.live_frame_in_flight()
            || !self
                .debug_windows
                .tas_editor
                .active_session()
                .is_some_and(|session| session.live_recording_history_group_active())
        {
            return;
        }
        if let Err(error) = self
            .debug_windows
            .tas_editor
            .end_live_recording_history_group()
        {
            self.toast_manager
                .error(format!("Could not finish TAS recording history: {error:#}"));
        }
    }

    pub(in crate::app) fn pump_realtime_tas_recording(&mut self) {
        let active = self.tas_control.realtime_recording_active();
        let input_owned = self.realtime_tas_game_input_owned();
        let can_record = self.tas_control.can_record_live_input();
        let now = Instant::now();
        let frame_duration = Duration::from_nanos(self.nominal_frame_duration_ns().max(1));
        match self
            .tas_realtime_recorder
            .poll(active, input_owned, can_record, now, frame_duration)
        {
            RealtimePoll::Idle => {}
            RealtimePoll::Record => match self.record_current_tas_input_and_advance() {
                Ok(()) => self.tas_realtime_recorder.mark_sent(),
                Err(error) => {
                    self.stop_realtime_tas_recording();
                    self.toast_manager
                        .error(format!("Could not record live TAS input: {error:#}"));
                }
            },
        }
    }

    fn realtime_tas_game_input_owned(&self) -> bool {
        self.game_window_focused && self.game_view_focused && !self.egui_wants_keyboard
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: Duration = Duration::from_millis(16);

    #[test]
    fn first_frame_waits_for_input_ownership_and_one_frame_period() {
        let start = Instant::now();
        let mut recorder = TasRealtimeRecorder::default();

        assert_eq!(
            recorder.poll(true, false, true, start, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(
            recorder.poll(true, true, true, start, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(recorder.next_wake(start), Some(start + FRAME));
        assert_eq!(
            recorder.poll(
                true,
                true,
                true,
                start + FRAME - Duration::from_nanos(1),
                FRAME
            ),
            RealtimePoll::Idle
        );
        assert_eq!(
            recorder.poll(true, true, true, start + FRAME, FRAME),
            RealtimePoll::Record
        );
    }

    #[test]
    fn pending_frame_blocks_more_samples_and_late_completion_drops_debt() {
        let start = Instant::now();
        let mut recorder = TasRealtimeRecorder::default();
        recorder.poll(true, true, true, start, FRAME);
        assert_eq!(
            recorder.poll(true, true, true, start + FRAME, FRAME),
            RealtimePoll::Record
        );
        recorder.mark_sent();

        assert_eq!(
            recorder.poll(true, true, false, start + FRAME * 4, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(
            recorder.poll(true, true, true, start + FRAME * 5, FRAME),
            RealtimePoll::Record
        );
        assert_eq!(
            recorder.next_wake(start + FRAME * 5),
            Some(start + FRAME * 5)
        );
    }

    #[test]
    fn running_recording_suspends_on_input_ownership_loss_and_resumes_fresh() {
        let start = Instant::now();
        let mut recorder = TasRealtimeRecorder::default();
        recorder.poll(true, true, true, start, FRAME);

        assert_eq!(
            recorder.poll(true, false, true, start + FRAME, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(recorder.next_wake(start + FRAME), None);
        assert_eq!(
            recorder.poll(true, true, true, start + FRAME * 10, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(
            recorder.next_wake(start + FRAME * 10),
            Some(start + FRAME * 11)
        );
        assert_eq!(
            recorder.poll(true, true, true, start + FRAME * 11, FRAME),
            RealtimePoll::Record
        );
    }

    #[test]
    fn suspended_pending_frame_never_samples_and_drops_timing_debt() {
        let start = Instant::now();
        let mut recorder = TasRealtimeRecorder::default();
        recorder.poll(true, true, true, start, FRAME);
        assert_eq!(
            recorder.poll(true, true, true, start + FRAME, FRAME),
            RealtimePoll::Record
        );
        recorder.mark_sent();

        assert_eq!(
            recorder.poll(true, false, false, start + FRAME * 2, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(
            recorder.poll(true, false, true, start + FRAME * 20, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(
            recorder.next_wake(start + FRAME * 20),
            Some(start + FRAME * 20 + RESPONSE_POLL_INTERVAL)
        );
        assert_eq!(
            recorder.poll(true, true, true, start + FRAME * 20, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(
            recorder.next_wake(start + FRAME * 20),
            Some(start + FRAME * 21)
        );
        assert_eq!(
            recorder.poll(true, true, true, start + FRAME * 21, FRAME),
            RealtimePoll::Record
        );
    }

    #[test]
    fn cleared_intent_resets_suspended_running_and_pending_states() {
        let start = Instant::now();
        let mut recorder = TasRealtimeRecorder::default();
        recorder.poll(true, true, true, start, FRAME);
        recorder.poll(true, true, true, start + FRAME, FRAME);
        recorder.mark_sent();

        assert_eq!(
            recorder.poll(false, true, false, start + FRAME, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(recorder.phase, RealtimePhase::Off);
        assert_eq!(recorder.next_wake(start + FRAME), None);
    }

    #[test]
    fn playback_uses_nominal_cadence_and_never_catches_up_in_batches() {
        let start = Instant::now();
        let mut scheduler = TasPlaybackScheduler::default();
        assert_eq!(scheduler.poll(true, true, start, FRAME), RealtimePoll::Idle);
        assert_eq!(scheduler.next_wake(start), Some(start + FRAME));
        assert_eq!(
            scheduler.poll(true, true, start + FRAME, FRAME),
            RealtimePoll::Record
        );
        scheduler.mark_sent();
        assert_eq!(
            scheduler.poll(true, false, start + FRAME * 20, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(
            scheduler.poll(true, true, start + FRAME * 20, FRAME),
            RealtimePoll::Record
        );
        scheduler.mark_sent();
        assert_eq!(
            scheduler.next_wake(start + FRAME * 20),
            Some(start + FRAME * 20 + RESPONSE_POLL_INTERVAL)
        );
        assert_eq!(
            scheduler.poll(true, true, start + FRAME * 20, FRAME),
            RealtimePoll::Idle
        );
    }

    #[test]
    fn playback_pause_clears_pending_cadence_without_input_focus_state() {
        let start = Instant::now();
        let mut scheduler = TasPlaybackScheduler::default();
        scheduler.poll(true, true, start, FRAME);
        scheduler.poll(true, true, start + FRAME, FRAME);
        scheduler.mark_sent();

        assert_eq!(
            scheduler.poll(false, false, start + FRAME * 2, FRAME),
            RealtimePoll::Idle
        );
        assert_eq!(scheduler.next_wake(start + FRAME * 2), None);
        assert_eq!(scheduler.phase, RealtimePhase::Off);
    }
}
