use std::time::Duration;

use crate::app::App;
use crate::platform::Instant;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum RealtimePhase {
    #[default]
    Off,
    Armed,
    Running {
        next_frame_at: Instant,
        frame_pending: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RealtimePoll {
    Idle,
    Record,
    Stop,
}

#[derive(Debug, Default)]
pub(in crate::app) struct TasRealtimeRecorder {
    phase: RealtimePhase,
}

impl TasRealtimeRecorder {
    pub(super) fn reset(&mut self) {
        self.phase = RealtimePhase::Off;
    }

    pub(super) fn next_wake(&self) -> Option<Instant> {
        match self.phase {
            RealtimePhase::Running {
                next_frame_at,
                frame_pending: false,
            } => Some(next_frame_at),
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
                RealtimePhase::Armed
            };
            return RealtimePoll::Idle;
        }

        match self.phase {
            RealtimePhase::Off => unreachable!(),
            RealtimePhase::Armed if input_owned => {
                self.phase = RealtimePhase::Running {
                    next_frame_at: now + frame_duration,
                    frame_pending: false,
                };
            }
            RealtimePhase::Armed => return RealtimePoll::Idle,
            RealtimePhase::Running { .. } if !input_owned => return RealtimePoll::Stop,
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

impl App {
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
        Ok(())
    }

    pub(in crate::app) fn stop_realtime_tas_recording(&mut self) {
        self.tas_control.stop_realtime_recording();
        self.tas_realtime_recorder.reset();
        self.finish_realtime_tas_history_group_if_idle();
    }

    pub(in crate::app) fn realtime_tas_recording_active(&self) -> bool {
        self.tas_control.realtime_recording_active()
    }

    pub(in crate::app) fn realtime_tas_recording_next_wake(&self) -> Option<Instant> {
        self.tas_realtime_recorder.next_wake()
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
        let input_owned =
            self.game_window_focused && self.game_view_focused && !self.egui_wants_keyboard;
        let can_record = self.tas_control.can_record_live_input();
        let now = Instant::now();
        let frame_duration = Duration::from_nanos(self.nominal_frame_duration_ns().max(1));
        match self
            .tas_realtime_recorder
            .poll(active, input_owned, can_record, now, frame_duration)
        {
            RealtimePoll::Idle => {}
            RealtimePoll::Stop => self.stop_realtime_tas_recording(),
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
        assert_eq!(recorder.next_wake(), Some(start + FRAME));
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
        assert_eq!(recorder.next_wake(), Some(start + FRAME * 5));
    }

    #[test]
    fn running_recording_stops_on_input_ownership_loss() {
        let start = Instant::now();
        let mut recorder = TasRealtimeRecorder::default();
        recorder.poll(true, true, true, start, FRAME);

        assert_eq!(
            recorder.poll(true, false, true, start + FRAME, FRAME),
            RealtimePoll::Stop
        );
    }

    #[test]
    fn cleared_intent_resets_armed_running_and_pending_states() {
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
        assert_eq!(recorder.next_wake(), None);
    }
}
