use std::time::{Duration, Instant};

pub(super) const BATTERY_FLUSH_INTERVAL: Duration = Duration::from_secs(30);

pub(super) struct BatteryFlushSchedule {
    next_attempt: Instant,
    potentially_dirty: bool,
}

impl BatteryFlushSchedule {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            next_attempt: now + BATTERY_FLUSH_INTERVAL,
            potentially_dirty: false,
        }
    }

    pub(super) fn mark_potentially_dirty(&mut self) {
        self.potentially_dirty = true;
    }

    pub(super) fn is_due(&self, now: Instant) -> bool {
        self.potentially_dirty && now >= self.next_attempt
    }

    pub(super) fn wait_timeout(&self, now: Instant) -> Option<Duration> {
        self.potentially_dirty
            .then_some(self.next_attempt.saturating_duration_since(now))
    }

    pub(super) fn finish_attempt(&mut self, now: Instant, succeeded: bool) {
        self.next_attempt = now + BATTERY_FLUSH_INTERVAL;
        if succeeded {
            self.potentially_dirty = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dirty_schedule_uses_deadlines_without_sleeping() {
        let start = Instant::now();
        let mut schedule = BatteryFlushSchedule::new(start);

        assert_eq!(schedule.wait_timeout(start), None);
        schedule.mark_potentially_dirty();
        assert_eq!(schedule.wait_timeout(start), Some(BATTERY_FLUSH_INTERVAL));
        assert!(!schedule.is_due(start + BATTERY_FLUSH_INTERVAL - Duration::from_nanos(1)));
        assert!(schedule.is_due(start + BATTERY_FLUSH_INTERVAL));

        schedule.finish_attempt(start + BATTERY_FLUSH_INTERVAL, true);
        assert_eq!(schedule.wait_timeout(start), None);
    }

    #[test]
    fn failed_attempt_remains_dirty_and_retries_on_the_next_deadline() {
        let start = Instant::now();
        let first_deadline = start + BATTERY_FLUSH_INTERVAL;
        let mut schedule = BatteryFlushSchedule::new(start);
        schedule.mark_potentially_dirty();
        schedule.finish_attempt(first_deadline, false);

        assert!(!schedule.is_due(first_deadline));
        assert!(schedule.is_due(first_deadline + BATTERY_FLUSH_INTERVAL));

        schedule.finish_attempt(first_deadline + BATTERY_FLUSH_INTERVAL, true);
        assert_eq!(schedule.wait_timeout(first_deadline), None);
    }

    #[test]
    fn mutations_after_a_success_reuse_the_armed_deadline() {
        let start = Instant::now();
        let first_deadline = start + BATTERY_FLUSH_INTERVAL;
        let mut schedule = BatteryFlushSchedule::new(start);
        schedule.mark_potentially_dirty();
        schedule.finish_attempt(first_deadline, true);
        schedule.mark_potentially_dirty();

        assert_eq!(
            schedule.wait_timeout(first_deadline),
            Some(BATTERY_FLUSH_INTERVAL)
        );
    }
}
