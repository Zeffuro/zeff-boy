use std::fmt;
use std::time::Duration;

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MasterTicks(u64);

impl MasterTicks {
    pub const ZERO: Self = Self(0);

    pub const fn new(ticks: u64) -> Self {
        Self(ticks)
    }

    pub const fn get(self) -> u64 {
        self.0
    }

    pub const fn checked_duration_since(self, earlier: Self) -> Option<Self> {
        match self.0.checked_sub(earlier.0) {
            Some(ticks) => Some(Self(ticks)),
            None => None,
        }
    }

    pub const fn wrapping_duration_since(self, earlier: Self) -> Self {
        Self(self.0.wrapping_sub(earlier.0))
    }
}

impl From<u64> for MasterTicks {
    fn from(ticks: u64) -> Self {
        Self(ticks)
    }
}

impl From<MasterTicks> for u64 {
    fn from(ticks: MasterTicks) -> Self {
        ticks.0
    }
}

impl fmt::Display for MasterTicks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ClockRate {
    numerator_hz: u64,
    denominator: u64,
}

impl ClockRate {
    pub const fn from_hz(hz: u64) -> Self {
        Self::from_ratio(hz, 1)
    }

    pub const fn from_ratio(numerator_hz: u64, denominator: u64) -> Self {
        assert!(numerator_hz != 0, "clock numerator must be non-zero");
        assert!(denominator != 0, "clock denominator must be non-zero");
        let divisor = gcd(numerator_hz, denominator);
        Self {
            numerator_hz: numerator_hz / divisor,
            denominator: denominator / divisor,
        }
    }

    pub const fn numerator_hz(self) -> u64 {
        self.numerator_hz
    }

    pub const fn denominator(self) -> u64 {
        self.denominator
    }

    pub fn duration_for(self, ticks: MasterTicks) -> Option<Duration> {
        let nanoseconds = u128::from(ticks.get())
            .checked_mul(u128::from(self.denominator))?
            .checked_mul(1_000_000_000)?
            / u128::from(self.numerator_hz);
        let seconds = u64::try_from(nanoseconds / 1_000_000_000).ok()?;
        let subsec_nanos = u32::try_from(nanoseconds % 1_000_000_000).ok()?;
        Some(Duration::new(seconds, subsec_nanos))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TimingSnapshot {
    now: MasterTicks,
    rate: ClockRate,
}

impl TimingSnapshot {
    pub const fn new(now: MasterTicks, rate: ClockRate) -> Self {
        Self { now, rate }
    }

    pub const fn now(self) -> MasterTicks {
        self.now
    }

    pub const fn rate(self) -> ClockRate {
        self.rate
    }

    pub const fn elapsed_since(self, earlier: Self) -> Option<MasterTicks> {
        if self.rate.numerator_hz != earlier.rate.numerator_hz
            || self.rate.denominator != earlier.rate.denominator
        {
            return None;
        }
        self.now.checked_duration_since(earlier.now)
    }

    pub fn elapsed_duration_since(self, earlier: Self) -> Option<Duration> {
        self.rate.duration_for(self.elapsed_since(earlier)?)
    }
}

pub trait MachineTiming {
    fn timing_snapshot(&self) -> TimingSnapshot;
}

const fn gcd(mut left: u64, mut right: u64) -> u64 {
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    left
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_rates_are_reduced() {
        assert_eq!(ClockRate::from_ratio(8, 4), ClockRate::from_hz(2));
    }

    #[test]
    fn converts_exact_rational_ticks_to_duration() {
        let rate = ClockRate::from_ratio(21_477_272, 12);
        let duration = rate.duration_for(MasterTicks::new(21_477_272)).unwrap();

        assert_eq!(duration, Duration::from_secs(12));
    }

    #[test]
    fn snapshots_require_the_same_clock_domain() {
        let start = TimingSnapshot::new(MasterTicks::new(10), ClockRate::from_hz(4));
        let end = TimingSnapshot::new(MasterTicks::new(18), ClockRate::from_hz(4));
        let other_rate = TimingSnapshot::new(MasterTicks::new(18), ClockRate::from_hz(8));

        assert_eq!(end.elapsed_since(start), Some(MasterTicks::new(8)));
        assert_eq!(
            end.elapsed_duration_since(start),
            Some(Duration::from_secs(2))
        );
        assert_eq!(other_rate.elapsed_since(start), None);
        assert_eq!(start.elapsed_since(end), None);
    }
}
