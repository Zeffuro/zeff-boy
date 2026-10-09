use super::*;
use crate::audio::{AudioQueueConfig, copy_stereo_at_speed};

pub(super) struct NativeSampleQueue {
    pub(super) producer: rtrb::Producer<QueuedAudioSample>,
    pub(super) staged_samples: VecDeque<f32>,
    pub(super) generation: u64,
    pub(super) limit: usize,
    pub(super) netplay_budget: NetplayBufferBudget,
}

impl NativeSampleQueue {
    pub(super) fn new(producer: rtrb::Producer<QueuedAudioSample>) -> Self {
        let limit = producer.buffer().capacity();
        Self {
            producer,
            staged_samples: VecDeque::new(),
            generation: 0,
            limit,
            netplay_budget: NetplayBufferBudget::default(),
        }
    }

    pub(super) fn pump(&mut self) {
        let occupied = self.producer.buffer().capacity() - self.producer.slots();
        let mut available = self.limit.saturating_sub(occupied);
        if self.netplay_budget.enabled() {
            available &= !1;
        }
        flush_staged_samples_with_limit(
            &mut self.producer,
            &mut self.staged_samples,
            self.generation,
            available,
        );
    }
}

impl AudioOutput {
    pub(crate) fn set_netplay_active(&mut self, active: bool) {
        if self
            .queue
            .netplay_budget
            .set_enabled(active && self.buffer_policy == AudioBufferPolicy::Auto)
        {
            self.underruns.store(0, Ordering::Relaxed);
            self.refresh_queue_budget(0);
        }
    }

    pub(crate) fn inherit_netplay_buffer(&mut self, previous: &Self) {
        if self.buffer_policy == AudioBufferPolicy::Auto {
            self.queue
                .netplay_budget
                .inherit(&previous.queue.netplay_budget);
            self.refresh_queue_budget(0);
        }
    }

    fn refresh_queue_budget(&mut self, underruns: u64) {
        if self.queue.configure_budget(
            QueueBudgetConfig {
                rate: self.output_sample_rate,
                policy: self.buffer_policy,
                speed: self.playback_speed,
                callback_samples: self.callback_samples.load(Ordering::Relaxed),
                underruns,
                elapsed: self.underrun_clock.elapsed(),
            },
            &self.playback_preroll_samples,
        ) {
            log::warn!("netplay audio budget recovered to the Auto queue in place");
        }
    }

    pub(crate) fn pump_queued_samples(&mut self) {
        let underruns = if self.queue.netplay_budget.enabled() {
            self.underruns.swap(0, Ordering::Relaxed)
        } else {
            0
        };
        self.refresh_queue_budget(underruns);
        self.queue.pump();
    }

    pub(crate) fn emulator_sample_rate(&self) -> u32 {
        self.source_sample_rate
    }

    pub(crate) fn take_buffer_fallback_request(&mut self) -> Option<AudioBufferFallback> {
        self.buffer_fallback_pending.take()
    }

    pub(crate) fn take_device_error(&self) -> Option<String> {
        stream_error_message(self.stream_errors.swap(0, Ordering::Relaxed))
    }

    pub(crate) fn discard_queued_samples(&mut self) {
        self.queue.generation = self.queue.generation.wrapping_add(1);
        self.playback_generation
            .store(self.queue.generation, Ordering::Release);
        self.queue.staged_samples.clear();
        self.low_pass_filter.reset();
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
        self.underruns.store(0, Ordering::Relaxed);
        self.underrun_recovery = UnderrunRecovery::default();
        self.queue.netplay_budget.discard();
        self.buffer_fallback_pending = None;
    }

    pub(crate) fn queue_samples(&mut self, samples: &[f32], config: &AudioQueueConfig) {
        let playback_speed = config.playback_speed.max(1);
        if playback_speed != self.playback_speed {
            self.discard_queued_samples();
            self.playback_speed = playback_speed;
        }

        self.pump_queued_samples();

        let fast_forward_active = playback_speed > 1;
        if fast_forward_active && config.mute_during_fast_forward {
            return;
        }

        let underruns = self.underruns.swap(0, Ordering::Relaxed);
        if underruns != 0 {
            log::warn!("audio output underrun ({underruns}); rebuffering before playback resumes");
        }
        if self.queue.netplay_budget.enabled() {
            self.refresh_queue_budget(underruns);
        } else if self.buffer_fallback_pending.is_none() {
            self.buffer_fallback_pending = self.underrun_recovery.observe(
                self.buffer_policy,
                underruns,
                self.underrun_clock.elapsed(),
            );
        }

        let gain = config.master_volume.clamp(0.0, 1.0);

        let max_queued = self.queue.limit;

        let occupied = self.capacity - self.queue.producer.slots();
        if occupied > max_queued && !self.queue.netplay_budget.enabled() {
            return;
        }

        let fill_ratio = self.queue.fill_ratio(fast_forward_active);

        let resampled;
        let samples = if let Some(ref mut resampler) = self.resampler {
            resampled = resampler.process(samples, fill_ratio);
            &resampled
        } else {
            samples
        };

        let samples = if fast_forward_active {
            copy_stereo_at_speed(samples, playback_speed, &mut self.speedup_samples);
            self.speedup_samples.as_slice()
        } else {
            samples
        };

        if samples.is_empty() {
            return;
        }

        if !config.low_pass_enabled {
            self.low_pass_filter.reset();
        }
        let alpha = low_pass_alpha(self.output_sample_rate, config.low_pass_cutoff_hz);

        if fast_forward_active {
            let available = self.queue.producer.slots().min(samples.len()) & !1;
            write_processed_samples(
                &mut self.queue.producer,
                &samples[..available],
                &mut self.low_pass_filter,
                ProcessedSampleConfig {
                    gain,
                    low_pass_enabled: config.low_pass_enabled,
                    alpha,
                    generation: self.queue.generation,
                },
            );
            return;
        }

        let max_staged = self.output_sample_rate as usize * 2 * MAX_STAGED_AUDIO_MS / 1000;
        let recovery = long_stall_recovery_range(
            occupied.saturating_add(self.queue.staged_samples.len()),
            samples.len(),
            max_staged,
            max_queued,
        );
        let recovered_samples;
        let samples = if let Some(range) = recovery {
            let dropped = occupied
                .saturating_add(self.queue.staged_samples.len())
                .saturating_add(samples.len())
                .saturating_sub(range.len());
            recovered_samples = samples[range].to_vec();
            self.discard_queued_samples();
            log::warn!(
                "audio staging exceeded {MAX_STAGED_AUDIO_MS} ms; dropped {dropped} stale samples"
            );
            &recovered_samples
        } else {
            samples
        };
        if samples.is_empty() {
            return;
        }
        if self
            .queue
            .staged_samples
            .try_reserve(samples.len())
            .is_err()
        {
            log::error!(
                "failed to reserve audio staging buffer; dropping {} samples",
                samples.len()
            );
            return;
        }
        for (index, &sample) in samples.iter().enumerate() {
            let mut out = sample * gain;
            if config.low_pass_enabled {
                out = self.low_pass_filter.apply_sample(out, index, alpha);
            }
            self.queue.staged_samples.push_back(out);
        }
        self.pump_queued_samples();
    }
}

#[cfg(test)]
pub(in crate::audio) fn flush_staged_samples(
    producer: &mut rtrb::Producer<QueuedAudioSample>,
    staged_samples: &mut VecDeque<f32>,
    generation: u64,
) {
    flush_staged_samples_with_limit(producer, staged_samples, generation, usize::MAX);
}

fn flush_staged_samples_with_limit(
    producer: &mut rtrb::Producer<QueuedAudioSample>,
    staged_samples: &mut VecDeque<f32>,
    generation: u64,
    limit: usize,
) {
    let available = producer.slots().min(staged_samples.len()).min(limit);
    if available == 0 {
        return;
    }
    if let Ok(mut chunk) = producer.write_chunk_uninit(available) {
        let (first, second) = chunk.as_mut_slices();
        for dst in first.iter_mut().chain(second.iter_mut()) {
            dst.write(QueuedAudioSample {
                generation,
                value: staged_samples
                    .pop_front()
                    .expect("staged audio length was checked before reserving the ring chunk"),
            });
        }
        unsafe {
            chunk.commit_all();
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod latency;
