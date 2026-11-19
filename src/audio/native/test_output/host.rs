use super::*;
use serde_json::{Value, json};
use std::time::Instant;

mod timeline;
use timeline::QueueRecord;

#[derive(Clone, Copy, serde::Serialize)]
struct CallbackRecord {
    elapsed_ms: f64,
    period_ms: f64,
    duration_ms: f64,
    requested: usize,
    consumed: usize,
    nonzero: usize,
    invalid: usize,
    initial: bool,
    incident: bool,
    hash: u64,
    queue: Option<QueueRecord>,
}

pub(crate) struct SilentHostDriver {
    records: rtrb::Consumer<CallbackRecord>,
    captured: Vec<CallbackRecord>,
    errors: Arc<AtomicU64>,
    lost: Arc<AtomicU64>,
    metadata: Value,
}

impl SilentHostDriver {
    pub(crate) fn poll(&mut self) {
        while let Ok(record) = self.records.pop() {
            self.captured.push(record);
        }
    }

    pub(crate) fn report(&mut self) -> Value {
        self.poll();
        let sum =
            |field: fn(&CallbackRecord) -> usize| self.captured.iter().map(field).sum::<usize>();
        let periods: Vec<_> = self.captured.iter().skip(1).map(|r| r.period_ms).collect();
        let durations: Vec<_> = self.captured.iter().map(|r| r.duration_ms).collect();
        let mut longest = 0;
        let mut pending = 0;
        for record in &self.captured {
            if record.consumed == 0 && !record.initial {
                pending += record.requested;
                longest = longest.max(pending);
            } else {
                pending = 0;
            }
        }
        json!({
            "host": self.metadata,
            "callbacks": self.captured.len(),
            "requested_frames": sum(|r| r.requested),
            "consumed_frames": sum(|r| r.consumed),
            "nonzero_stereo_scalars": sum(|r| r.nonzero),
            "invalid_stereo_scalars": sum(|r| r.invalid),
            "initial_silent_frames": sum(|r| if r.initial { r.requested } else { 0 }),
            "rebuffer_silent_frames": sum(|r| if r.consumed == 0 && !r.initial { r.requested } else { 0 }),
            "longest_rebuffer_silent_frames": longest,
            "underrun_incidents": self.captured.iter().filter(|r| r.incident).count(),
            "callback_period_ms": distribution(periods),
            "callback_execution_ms": distribution(durations),
            "last_callback_ms": self.captured.last().map(|r| r.elapsed_ms),
            "last_consumed_ms": self.captured.iter().rev().find(|r| r.consumed != 0).map(|r| r.elapsed_ms),
            "consumed_frames_after_1500ms": sum(|r| if r.elapsed_ms >= 1500.0 { r.consumed } else { 0 }),
            "consumed_pcm_fnv64": self.captured.last().map(|r| format!("{:016x}", r.hash)),
            "stream_errors": self.errors.load(Ordering::Relaxed),
            "lost_callback_records": self.lost.load(Ordering::Relaxed),
            "timeline": self.captured.iter().filter(|record| record.queue.is_some()).collect::<Vec<_>>(),
        })
    }
}

fn distribution(mut samples: Vec<f64>) -> Value {
    samples.sort_by(f64::total_cmp);
    if samples.is_empty() {
        return json!({"samples": 0});
    }
    let at = |percent: usize| samples[(samples.len() * percent).div_ceil(100) - 1];
    json!({"samples": samples.len(), "min": samples[0], "p50": at(50), "p95": at(95), "max": samples.last()})
}

impl AudioOutput {
    pub(crate) fn silent_host_output(
        source_rate: u32,
        policy: AudioBufferPolicy,
        timeline_epoch: Option<Instant>,
    ) -> anyhow::Result<(Self, SilentHostDriver)> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("no default output device")?;
        let mut rejected = Vec::new();
        for config in Self::select_output_configs(&device, Some(source_rate))? {
            if config.sample_format() != SampleFormat::F32 {
                continue;
            }
            match Self::silent_host_config(&device, &config, source_rate, policy, timeline_epoch) {
                Ok((output, mut driver)) => {
                    driver.metadata = json!({
                        "backend": format!("{:?}", host.id()),
                        "device": output_device_info(&device)?.name,
                        "source_rate": source_rate, "output_rate": config.sample_rate(),
                        "channels": config.channels(), "format": "F32", "policy": format!("{policy:?}"),
                        "rejected_configs": rejected,
                    });
                    return Ok((output, driver));
                }
                Err(error) => rejected.push(json!({
                    "rate": config.sample_rate(), "channels": config.channels(), "error": error.to_string(),
                })),
            }
        }
        anyhow::bail!("no usable F32 output configuration: {rejected:?}")
    }

    fn silent_host_config(
        device: &cpal::Device,
        config: &SupportedStreamConfig,
        source_rate: u32,
        policy: AudioBufferPolicy,
        timeline_epoch: Option<Instant>,
    ) -> anyhow::Result<(Self, SilentHostDriver)> {
        let rate = config.sample_rate();
        let channels = config.channels() as usize;
        anyhow::ensure!(channels != 0, "output has zero channels");
        let capacity = ring_buffer_capacity_for_policy(rate, policy);
        let (producer, mut consumer) = rtrb::RingBuffer::new(capacity);
        let underruns = Arc::new(AtomicU64::new(0));
        let generation = Arc::new(AtomicU64::new(0));
        let preroll = Arc::new(AtomicUsize::new(playback_preroll_samples(
            rate,
            policy.normal_queue_ms(),
        )));
        let mut state = AudioPlaybackState::new(
            Arc::clone(&preroll),
            Arc::clone(&underruns),
            Arc::clone(&generation),
        );
        let callback_samples = Arc::clone(&state.callback_samples);
        let errors = Arc::new(AtomicU64::new(0));
        let failed = Arc::clone(&errors);
        let pending_errors = Arc::new(AtomicU64::new(0));
        let pending_failure = Arc::clone(&pending_errors);
        let lost = Arc::new(AtomicU64::new(0));
        let overflow = Arc::clone(&lost);
        let (mut records, receiver) = rtrb::RingBuffer::new(8192);
        let started = Instant::now();
        let mut previous = None;
        let mut ever_primed = false;
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let stream = device.build_output_stream(
            config.config(),
            move |pcm: &mut [f32], _| {
                let begin = Instant::now();
                let period_ms = previous.map_or(0.0, |time| {
                    begin.duration_since(time).as_secs_f64() * 1000.0
                });
                previous = Some(begin);
                let primed = state.primed;
                let requested = pcm.len() / channels;
                let mut queue = timeline_epoch.map(|epoch| {
                    QueueRecord::begin(epoch, begin, requested * 2, &mut consumer, &state)
                });
                fill_output_f32(pcm, channels as u16, &mut consumer, &mut state);
                if let Some(record) = &mut queue {
                    record.finish(timeline_epoch.unwrap(), &mut consumer, &state);
                }
                let supported = requested * 2 <= capacity && pcm.len().is_multiple_of(channels);
                let consumed = if supported && state.primed {
                    requested
                } else {
                    0
                };
                let initial = !ever_primed && consumed == 0;
                ever_primed |= state.primed;
                let mut nonzero = 0;
                let mut invalid = usize::from(!supported);
                for frame in pcm.chunks_exact(channels).take(consumed) {
                    for &sample in frame.iter().take(2) {
                        nonzero += usize::from(sample != 0.0);
                        invalid += usize::from(!sample.is_finite());
                        hash = (hash ^ u64::from(sample.to_bits())).wrapping_mul(0x100_0000_01b3);
                    }
                }
                pcm.fill(0.0);
                let record = CallbackRecord {
                    elapsed_ms: begin.duration_since(started).as_secs_f64() * 1000.0,
                    period_ms,
                    duration_ms: begin.elapsed().as_secs_f64() * 1000.0,
                    requested,
                    consumed,
                    nonzero,
                    invalid,
                    initial,
                    incident: primed && !state.primed,
                    hash,
                    queue,
                };
                if records.push(record).is_err() {
                    overflow.fetch_add(1, Ordering::Relaxed);
                }
            },
            move |_| {
                failed.fetch_add(1, Ordering::Relaxed);
                pending_failure.fetch_add(1, Ordering::Relaxed);
            },
            None,
        )?;
        let output = Self {
            _stream: Some(stream),
            queue: NativeSampleQueue::new(producer),
            source_sample_rate: source_rate,
            output_sample_rate: rate,
            capacity,
            low_pass_filter: OnePoleLowPass::default(),
            resampler: Some(resampler::AudioResampler::new(source_rate, rate)?),
            underruns,
            playback_preroll_samples: preroll,
            callback_samples,
            playback_generation: generation,
            playback_speed: 1,
            speedup_samples: Vec::new(),
            buffer_policy: policy,
            underrun_recovery: UnderrunRecovery::default(),
            underrun_clock: crate::platform::Instant::now(),
            buffer_fallback_pending: None,
            stream_errors: pending_errors,
        };
        output._stream.as_ref().unwrap().play()?;
        Ok((
            output,
            SilentHostDriver {
                records: receiver,
                captured: Vec::new(),
                errors,
                lost,
                metadata: Value::Null,
            },
        ))
    }

    pub(crate) fn test_queue_snapshot(&self) -> Value {
        json!({
            "ring_scalars": self.capacity - self.queue.producer.slots(),
            "staged_scalars": self.queue.staged_samples.len(),
            "preroll_scalars": self.playback_preroll_samples.load(Ordering::Relaxed),
            "generation": self.queue.generation,
        })
    }

    pub(crate) fn test_queue_levels(&self) -> (usize, usize) {
        (
            self.capacity - self.queue.producer.slots(),
            self.queue.staged_samples.len(),
        )
    }

    pub(crate) fn test_queue_timing(&self) -> [u64; 5] {
        [
            (self.capacity - self.queue.producer.slots()) as u64,
            self.queue.staged_samples.len() as u64,
            self.playback_preroll_samples.load(Ordering::Relaxed) as u64,
            self.queue.generation,
            self.playback_generation.load(Ordering::Acquire),
        ]
    }
}
