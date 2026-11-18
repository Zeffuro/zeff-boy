use super::*;

pub(super) const ENABLED: bool = true;

#[derive(Clone, Copy)]
pub(super) struct IntervalHint {
    region: Region,
    expected_now: MasterTicks,
    end: MasterTicks,
}

impl PartialEq for Vdp {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state && self.memory == other.memory && self.owned == other.owned
    }
}

impl Eq for Vdp {}

impl std::fmt::Debug for Vdp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Vdp")
            .field("state", &self.state)
            .field("memory", &self.memory)
            .field("owned", &self.owned)
            .finish()
    }
}

impl Vdp {
    pub(crate) fn rollback_supplements(&self) -> (u64, Option<(u8, u64, u64)>) {
        let Self {
            state: _,
            memory: _,
            interval_hint,
            dma_activity_epoch,
            owned: _,
        } = self;
        let hint = interval_hint.map(
            |IntervalHint {
                 region,
                 expected_now,
                 end,
             }| {
                let region = match region {
                    Region::NtscUs => 0,
                    Region::NtscJapan => 1,
                    Region::Pal => 2,
                };
                (region, expected_now.get(), end.get())
            },
        );
        (*dma_activity_epoch, hint)
    }

    #[cfg(all(
        test,
        not(any(
            feature = "sound-shadow-observer",
            feature = "first-failure-diagnostic"
        ))
    ))]
    pub(crate) fn rollback_test_supplements(
        &mut self,
        epoch: u64,
        hint: Option<(Region, MasterTicks, MasterTicks)>,
    ) {
        self.dma_activity_epoch = epoch;
        self.interval_hint = hint.map(|(region, expected_now, end)| IntervalHint {
            region,
            expected_now,
            end,
        });
    }

    fn interval_idle(&self) -> bool {
        self.owned.is_none()
            && self.state.fifo.is_empty()
            && self.state.read_phase.is_none()
            && self.state.dma.is_none()
            && self.state.registers.pending().is_none()
    }

    pub(super) fn invalidate_interval(&mut self) {
        self.assert_legacy();
        self.interval_hint = None;
        #[cfg(test)]
        record(Metric::Mutation);
    }

    pub(super) fn seed_interval(&mut self, region: Region) {
        self.interval_hint = self
            .interval_idle()
            .then(|| {
                self.state
                    .interrupts
                    .interval_end(region, &self.state.registers)
            })
            .flatten()
            .map(|end| IntervalHint {
                region,
                expected_now: self.now(),
                end,
            });
        #[cfg(test)]
        record(if self.interval_hint.is_some() {
            Metric::Seed
        } else {
            Metric::Refused
        });
    }

    #[inline]
    pub(super) fn advance_interval(&mut self, at: MasterTicks, region: Region) -> bool {
        let Some(hint) = self.interval_hint else {
            #[cfg(test)]
            record(Metric::Cold);
            return false;
        };
        if hint.region != region {
            #[cfg(test)]
            record(Metric::Region);
            return false;
        }
        let now = self.now();
        if hint.expected_now != now || at <= now {
            #[cfg(test)]
            record(Metric::Time);
            return false;
        }
        if at >= hint.end || !self.interval_idle() {
            #[cfg(test)]
            record(Metric::Boundary);
            return false;
        }
        self.state.interrupts.advance_interval_clock(at);
        self.state.registers.advance_interval_clock(at);
        self.interval_hint.as_mut().unwrap().expected_now = at;
        #[cfg(test)]
        record(Metric::Hit);
        true
    }
}

#[cfg(test)]
#[derive(Clone, Copy)]
pub(super) enum Metric {
    Hit,
    Seed,
    Refused,
    Cold,
    Mutation,
    Region,
    Time,
    Boundary,
    Walker,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Counts {
    values: [u64; 9],
    consecutive: u64,
    longest: u64,
}

#[cfg(test)]
thread_local! {
    static COUNTS: std::cell::Cell<Counts> = const { std::cell::Cell::new(Counts {
        values: [0; 9], consecutive: 0, longest: 0,
    }) };
}

#[cfg(test)]
pub(super) fn record(metric: Metric) {
    COUNTS.with(|cell| {
        let mut counts = cell.get();
        counts.values[metric as usize] += 1;
        if matches!(metric, Metric::Hit) {
            counts.consecutive += 1;
            counts.longest = counts.longest.max(counts.consecutive);
        } else {
            counts.consecutive = 0;
        }
        cell.set(counts);
    });
}

#[cfg(test)]
#[path = "interval_tests.rs"]
mod tests;
