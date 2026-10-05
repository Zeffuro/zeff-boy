use std::collections::{BTreeMap, btree_map::Entry};
use std::time::Duration;

use crate::platform::Instant;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Stats {
    pub(crate) datagrams: bool,
    pub(crate) sent: u64,
    pub(crate) received: u64,
    pub(crate) sent_bytes: u64,
    pub(crate) received_bytes: u64,
    pub(crate) repeated_batches: u64,
    pub(crate) ack_wait: Option<Duration>,
    pub(crate) ack_age: Option<Duration>,
    pub(crate) ack_samples: u64,
}

#[derive(Default)]
pub(crate) struct Metrics {
    stats: Stats,
    flights: BTreeMap<u64, Instant>,
    acknowledged: u64,
    sampled_at: Option<Instant>,
}

impl Metrics {
    pub(crate) fn direct() -> Self {
        Self {
            stats: Stats {
                datagrams: true,
                ..Stats::default()
            },
            ..Self::default()
        }
    }

    pub(crate) fn sent(&mut self, bytes: usize) {
        self.stats.sent = self.stats.sent.saturating_add(1);
        self.stats.sent_bytes = self.stats.sent_bytes.saturating_add(bytes as u64);
    }

    pub(crate) fn received(&mut self, bytes: usize) {
        self.stats.received = self.stats.received.saturating_add(1);
        self.stats.received_bytes = self.stats.received_bytes.saturating_add(bytes as u64);
    }

    pub(crate) fn input_attempt(&mut self, frames: impl Iterator<Item = u64>, now: Instant) {
        let mut repeated = false;
        for frame in frames {
            if frame < self.acknowledged {
                continue;
            }
            match self.flights.entry(frame) {
                Entry::Occupied(_) => repeated = true,
                Entry::Vacant(entry) => {
                    entry.insert(now);
                }
            }
            while self.flights.len() > 64 {
                self.flights.pop_first();
            }
        }
        if repeated {
            self.stats.repeated_batches = self.stats.repeated_batches.saturating_add(1);
        }
    }

    pub(crate) fn acknowledge(&mut self, ack: u64, now: Instant) {
        if ack <= self.acknowledged {
            return;
        }
        let mut oldest = None;
        while self
            .flights
            .first_key_value()
            .is_some_and(|(&frame, _)| frame < ack)
        {
            let (_, first_attempt) = self.flights.pop_first().unwrap();
            oldest.get_or_insert(first_attempt);
        }
        self.acknowledged = ack;
        if let Some(sent_at) = oldest {
            self.stats.ack_wait = Some(now.saturating_duration_since(sent_at));
            self.stats.ack_samples = self.stats.ack_samples.saturating_add(1);
            self.sampled_at = Some(now);
        }
    }

    pub(crate) fn snapshot(&self, now: Instant) -> Stats {
        Stats {
            ack_age: self
                .sampled_at
                .map(|sample| now.saturating_duration_since(sample)),
            ..self.stats
        }
    }
}

#[cfg(test)]
mod tests;
