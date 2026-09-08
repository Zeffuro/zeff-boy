use std::collections::VecDeque;

use crate::platform::Instant;

const WINDOW_SIZE: usize = 256;

#[derive(Debug, Clone, Copy)]
pub(crate) struct InputPollTiming {
    pub(crate) latest_event: Option<Instant>,
    pub(crate) observed_events: u64,
    pub(crate) snapshot_complete: Instant,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct TimingDistribution {
    pub(crate) samples: usize,
    pub(crate) p50_ms: f64,
    pub(crate) p95_ms: f64,
    pub(crate) p99_ms: f64,
    pub(crate) max_ms: f64,
}

#[derive(Debug, Clone, Default)]
struct TimingWindow(VecDeque<f64>);

impl TimingWindow {
    fn push(&mut self, milliseconds: f64) {
        if self.0.len() == WINDOW_SIZE {
            self.0.pop_front();
        }
        self.0.push_back(milliseconds);
    }

    fn summary(&self) -> TimingDistribution {
        let mut values: Vec<_> = self.0.iter().copied().collect();
        values.sort_by(f64::total_cmp);
        if values.is_empty() {
            return TimingDistribution::default();
        }
        let percentile = |percent: usize| values[(values.len() * percent).div_ceil(100) - 1];
        TimingDistribution {
            samples: values.len(),
            p50_ms: percentile(50),
            p95_ms: percentile(95),
            p99_ms: percentile(99),
            max_ms: *values.last().expect("nonempty timing window"),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct InputTimingSummary {
    pub(crate) observed_events: u64,
    pub(crate) polls: u64,
    pub(crate) submitted_frames: u64,
    pub(crate) coalesced_events: u64,
    pub(crate) duplicate_frames: u64,
    pub(crate) invalid_samples: u64,
    pub(crate) poll_interval: TimingDistribution,
    pub(crate) event_to_snapshot: TimingDistribution,
    pub(crate) snapshot_to_frame: TimingDistribution,
    pub(crate) frame_to_submission: TimingDistribution,
    pub(crate) event_to_submission: TimingDistribution,
}

#[derive(Debug, Clone, Copy)]
struct FrameObservation {
    reached: Instant,
    snapshot: Option<Instant>,
    latest_event: Option<Instant>,
    observed_events: u64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct InputTiming {
    enabled: bool,
    observed_events: u64,
    polls: u64,
    submitted_frames: u64,
    coalesced_events: u64,
    duplicate_frames: u64,
    invalid_samples: u64,
    submitted_event_cursor: u64,
    latest_snapshot: Option<Instant>,
    latest_event: Option<Instant>,
    frame: Option<FrameObservation>,
    poll_interval: TimingWindow,
    event_to_snapshot: TimingWindow,
    snapshot_to_frame: TimingWindow,
    frame_to_submission: TimingWindow,
    event_to_submission: TimingWindow,
}

impl InputTiming {
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn frame_pending(&self) -> bool {
        self.enabled && self.frame.is_some()
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            *self = Self {
                enabled,
                ..Self::default()
            };
        }
    }

    pub(crate) fn reset(&mut self) {
        *self = Self {
            enabled: self.enabled,
            ..Self::default()
        };
    }

    pub(crate) fn pause(&mut self) {
        self.latest_snapshot = None;
        self.latest_event = None;
        self.frame = None;
        self.submitted_event_cursor = self.observed_events;
    }

    pub(crate) fn observe_poll(&mut self, poll: InputPollTiming) {
        if !self.enabled {
            return;
        }
        if poll.latest_event.is_some() != (poll.observed_events > 0)
            || poll
                .latest_event
                .is_some_and(|event| event > poll.snapshot_complete)
            || self
                .latest_snapshot
                .is_some_and(|previous| previous > poll.snapshot_complete)
        {
            self.invalid_samples = self.invalid_samples.saturating_add(1);
            return;
        }
        if let Some(previous) = self.latest_snapshot {
            self.poll_interval
                .push(milliseconds(previous, poll.snapshot_complete));
        }
        if let Some(event) = poll.latest_event {
            self.event_to_snapshot
                .push(milliseconds(event, poll.snapshot_complete));
            self.latest_event = Some(event);
        }
        self.latest_snapshot = Some(poll.snapshot_complete);
        self.observed_events = self.observed_events.saturating_add(poll.observed_events);
        self.polls = self.polls.saturating_add(1);
    }

    pub(crate) fn frame_reached(&mut self, reached: Instant) {
        if !self.enabled {
            return;
        }
        self.frame = None;
        if self
            .latest_snapshot
            .is_some_and(|snapshot| snapshot > reached)
        {
            self.invalid_samples = self.invalid_samples.saturating_add(1);
            return;
        }
        self.frame = Some(FrameObservation {
            reached,
            snapshot: self.latest_snapshot,
            latest_event: self.latest_event,
            observed_events: self.observed_events,
        });
    }

    pub(crate) fn frame_submitted(&mut self, submitted: Instant) {
        if !self.enabled {
            return;
        }
        let Some(frame) = self.frame.take() else {
            self.invalid_samples = self.invalid_samples.saturating_add(1);
            return;
        };
        if submitted < frame.reached {
            self.invalid_samples = self.invalid_samples.saturating_add(1);
            return;
        }
        self.frame_to_submission
            .push(milliseconds(frame.reached, submitted));
        if let Some(snapshot) = frame.snapshot {
            self.snapshot_to_frame
                .push(milliseconds(snapshot, frame.reached));
        }
        let events = frame
            .observed_events
            .saturating_sub(self.submitted_event_cursor);
        if events == 0 {
            self.duplicate_frames = self.duplicate_frames.saturating_add(1);
        } else {
            self.coalesced_events = self.coalesced_events.saturating_add(events - 1);
            if let Some(event) = frame.latest_event {
                self.event_to_submission
                    .push(milliseconds(event, submitted));
            }
        }
        self.submitted_event_cursor = frame.observed_events;
        self.submitted_frames = self.submitted_frames.saturating_add(1);
    }

    pub(crate) fn summary(&self) -> InputTimingSummary {
        InputTimingSummary {
            observed_events: self.observed_events,
            polls: self.polls,
            submitted_frames: self.submitted_frames,
            coalesced_events: self.coalesced_events,
            duplicate_frames: self.duplicate_frames,
            invalid_samples: self.invalid_samples,
            poll_interval: self.poll_interval.summary(),
            event_to_snapshot: self.event_to_snapshot.summary(),
            snapshot_to_frame: self.snapshot_to_frame.summary(),
            frame_to_submission: self.frame_to_submission.summary(),
            event_to_submission: self.event_to_submission.summary(),
        }
    }

    pub(crate) fn report(&self) -> String {
        use std::fmt::Write;
        let summary = self.summary();
        let mut report = format!(
            "Input timing (enabled: {})\nT0: latest gilrs backend event observed in each poll; T1: normalized snapshot complete; T2: diagnostic UI frame reached; T3: CPU queue.submit returned. T4/display and physical latency unavailable.\nLast {WINDOW_SIZE} samples per lane; counters since measurement start/reset. Collection pauses outside the visible Input Test page; unsubmitted timestamp links are discarded on pause.\nBackend events: {}; polls: {}; submitted diagnostic frames: {}; coalesced backend events: {}; frames without new backend events: {}; rejected timing samples: {}.\nCoalescing and repeated snapshots do not measure missed physical input.\n",
            self.enabled,
            summary.observed_events,
            summary.polls,
            summary.submitted_frames,
            summary.coalesced_events,
            summary.duplicate_frames,
            summary.invalid_samples,
        );
        for (label, distribution) in [
            ("Poll interval", summary.poll_interval),
            ("T0 to T1", summary.event_to_snapshot),
            ("T1 to T2", summary.snapshot_to_frame),
            ("T2 to T3", summary.frame_to_submission),
            ("T0 to T3 (new events only)", summary.event_to_submission),
        ] {
            let _ = writeln!(
                report,
                "{label}: n={}, p50={:.3} ms, p95={:.3} ms, p99={:.3} ms, max={:.3} ms",
                distribution.samples,
                distribution.p50_ms,
                distribution.p95_ms,
                distribution.p99_ms,
                distribution.max_ms
            );
        }
        report
    }
}

fn milliseconds(start: Instant, end: Instant) -> f64 {
    end.duration_since(start).as_secs_f64() * 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn disabled_collection_is_empty_and_enabling_starts_a_new_session() {
        let mut timing = InputTiming::default();
        let now = Instant::now();
        let poll = InputPollTiming {
            latest_event: Some(now),
            observed_events: 1,
            snapshot_complete: now,
        };
        timing.observe_poll(poll);
        timing.frame_reached(now);
        timing.frame_submitted(now);
        assert_eq!(timing.summary().polls, 0);
        timing.set_enabled(true);
        timing.observe_poll(poll);
        assert_eq!(timing.summary().observed_events, 1);
        timing.reset();
        assert!(timing.enabled());
        assert_eq!(timing.summary().observed_events, 0);
        timing.set_enabled(false);
        assert!(!timing.enabled());
    }

    #[test]
    fn event_coalescing_counts_only_successfully_submitted_snapshot_prefix() {
        let mut timing = InputTiming::default();
        timing.set_enabled(true);
        let now = Instant::now();
        let at = |ms| now + Duration::from_millis(ms);
        timing.observe_poll(InputPollTiming {
            latest_event: Some(at(1)),
            observed_events: 3,
            snapshot_complete: at(2),
        });
        timing.frame_reached(at(4));
        timing.observe_poll(InputPollTiming {
            latest_event: Some(at(5)),
            observed_events: 2,
            snapshot_complete: at(6),
        });
        timing.frame_submitted(at(8));
        let summary = timing.summary();
        assert_eq!(summary.coalesced_events, 2);
        assert_eq!(summary.event_to_submission.p50_ms, 7.0);
        assert_eq!(summary.snapshot_to_frame.p50_ms, 2.0);
        timing.frame_reached(at(10));
        timing.frame_submitted(at(12));
        timing.frame_reached(at(14));
        timing.frame_submitted(at(15));
        let summary = timing.summary();
        assert_eq!(summary.coalesced_events, 3);
        assert_eq!(summary.duplicate_frames, 1);
        assert_eq!(summary.submitted_frames, 3);
        assert_eq!(summary.event_to_submission.samples, 2);
    }

    #[test]
    fn invalid_ordering_and_unsubmitted_frames_do_not_consume_events() {
        let mut timing = InputTiming::default();
        timing.set_enabled(true);
        let now = Instant::now();
        let later = now + Duration::from_millis(1);
        timing.observe_poll(InputPollTiming {
            latest_event: Some(later),
            observed_events: 1,
            snapshot_complete: now,
        });
        assert_eq!(timing.summary().invalid_samples, 1);
        timing.observe_poll(InputPollTiming {
            latest_event: Some(now),
            observed_events: 2,
            snapshot_complete: now,
        });
        timing.frame_reached(later);
        timing.frame_submitted(now);
        assert_eq!(timing.summary().submitted_frames, 0);
        timing.frame_reached(later);
        timing.frame_reached(later);
        timing.frame_submitted(later);
        assert_eq!(timing.summary().coalesced_events, 1);
        assert_eq!(timing.summary().invalid_samples, 2);
    }

    #[test]
    fn timing_window_is_bounded_and_uses_nearest_rank_percentiles() {
        let mut window = TimingWindow::default();
        for value in 1..=300 {
            window.push(f64::from(value));
        }
        let distribution = window.summary();
        assert_eq!(distribution.samples, 256);
        assert_eq!(distribution.p50_ms, 172.0);
        assert_eq!(distribution.p95_ms, 288.0);
        assert_eq!(distribution.p99_ms, 298.0);
        assert_eq!(distribution.max_ms, 300.0);
    }

    #[test]
    fn pause_preserves_totals_without_linking_hidden_time_or_pending_events() {
        let mut timing = InputTiming::default();
        timing.set_enabled(true);
        let now = Instant::now();
        timing.observe_poll(InputPollTiming {
            latest_event: Some(now),
            observed_events: 2,
            snapshot_complete: now,
        });
        timing.frame_reached(now);
        timing.pause();
        let later = now + Duration::from_secs(600);
        timing.observe_poll(InputPollTiming {
            latest_event: None,
            observed_events: 0,
            snapshot_complete: later,
        });
        timing.frame_reached(later);
        timing.frame_submitted(later);
        let summary = timing.summary();
        assert_eq!(summary.observed_events, 2);
        assert_eq!(summary.polls, 2);
        assert_eq!(summary.poll_interval.samples, 0);
        assert_eq!(summary.event_to_submission.samples, 0);
        assert_eq!(summary.duplicate_frames, 1);
        assert_eq!(summary.coalesced_events, 0);
        assert!(timing.enabled());
    }
}
