use super::*;

#[derive(Default)]
pub(super) struct NetplayBufferBudget {
    enabled: bool,
    recovered: bool,
    recovery: UnderrunRecovery,
}

pub(super) struct QueueBudgetConfig {
    pub(super) rate: u32,
    pub(super) policy: AudioBufferPolicy,
    pub(super) speed: usize,
    pub(super) callback_samples: usize,
    pub(super) underruns: u64,
    pub(super) elapsed: Duration,
}

impl NetplayBufferBudget {
    pub(super) fn enabled(&self) -> bool {
        self.enabled
    }

    pub(super) fn set_enabled(&mut self, enabled: bool) -> bool {
        if self.enabled == enabled {
            return false;
        }
        *self = Self {
            enabled,
            ..Self::default()
        };
        true
    }

    pub(super) fn inherit(&mut self, previous: &Self) {
        *self = Self {
            enabled: previous.enabled,
            recovered: previous.recovered,
            ..Self::default()
        };
    }

    pub(super) fn discard(&mut self) {
        self.recovery = UnderrunRecovery::default();
    }

    pub(super) fn policy(&self, requested: AudioBufferPolicy) -> AudioBufferPolicy {
        if self.enabled && !self.recovered && requested == AudioBufferPolicy::Auto {
            AudioBufferPolicy::LowLatency
        } else {
            requested
        }
    }

    fn observe(&mut self, config: &QueueBudgetConfig) -> bool {
        if !self.enabled || self.recovered {
            return false;
        }
        let exceeds_budget = config.callback_samples
            > ring_buffer_capacity_for_policy(config.rate, AudioBufferPolicy::LowLatency);
        if exceeds_budget
            || self
                .recovery
                .observe(
                    AudioBufferPolicy::LowLatency,
                    config.underruns,
                    config.elapsed,
                )
                .is_some()
        {
            self.recovered = true;
            return true;
        }
        false
    }
}

impl NativeSampleQueue {
    pub(super) fn configure_budget(
        &mut self,
        config: QueueBudgetConfig,
        preroll: &AtomicUsize,
    ) -> bool {
        let recovered = self.netplay_budget.observe(&config);
        let queue_ms = if config.speed > 1 {
            FAST_FORWARD_QUEUE_MS
        } else {
            self.netplay_budget.policy(config.policy).normal_queue_ms()
        };
        self.limit = (config.rate as usize * 2 * queue_ms / 1000)
            .max(2)
            .min(self.producer.buffer().capacity());
        if self.netplay_budget.enabled() {
            self.limit &= !1;
        }
        preroll.store(
            playback_preroll_samples(config.rate, queue_ms),
            Ordering::Relaxed,
        );
        recovered
    }

    pub(super) fn fill_ratio(&self, fast_forward: bool) -> f32 {
        let capacity = self.producer.buffer().capacity();
        let buffered = (capacity - self.producer.slots()).saturating_add(self.staged_samples.len());
        buffered as f32 / if fast_forward { capacity } else { self.limit } as f32
    }
}

#[cfg(test)]
mod tests;
