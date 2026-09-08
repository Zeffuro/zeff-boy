use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{DeviceId, SampleFormat, StreamConfig, SupportedStreamConfig};
use std::collections::VecDeque;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use super::resampler;
use super::{AudioQueueConfig, copy_stereo_at_speed};
use crate::settings::AudioBufferPolicy;

const FAST_FORWARD_QUEUE_MS: usize = 40;
const MAX_STAGED_AUDIO_MS: usize = 10_000;
const STEREO_MIX_FACTOR: f32 = 0.5;
const AUDIO_LOW_PASS_MIN_CUTOFF_HZ: u32 = 20;
const AUDIO_LOW_PASS_MAX_CUTOFF_HZ: u32 = 20_000;
const LOW_LATENCY_UNDERRUN_REPORTS_BEFORE_FALLBACK: u8 = 3;
const LOW_LATENCY_UNDERRUN_WINDOW: Duration = Duration::from_secs(5);

#[cfg(test)]
pub(super) fn ring_buffer_capacity(sample_rate: u32) -> usize {
    ring_buffer_capacity_for_policy(sample_rate, AudioBufferPolicy::Auto)
}

pub(super) fn ring_buffer_capacity_for_policy(
    sample_rate: u32,
    policy: AudioBufferPolicy,
) -> usize {
    sample_rate as usize * 2 * policy.normal_queue_ms() / 1000
}

pub(super) fn playback_preroll_samples(sample_rate: u32, queue_ms: usize) -> usize {
    sample_rate as usize * 2 * (queue_ms / 2) / 1000
}

pub(super) fn sample_format_rank(format: SampleFormat) -> u8 {
    match format {
        SampleFormat::F32 => 0,
        SampleFormat::I16 => 1,
        SampleFormat::U16 => 2,
        SampleFormat::U8 => 3,
        _ => 4,
    }
}

fn same_config(a: &SupportedStreamConfig, b: &SupportedStreamConfig) -> bool {
    a.sample_rate() == b.sample_rate()
        && a.channels() == b.channels()
        && a.sample_format() == b.sample_format()
}

pub(super) fn emulator_source_sample_rate(
    preferred_sample_rate: Option<u32>,
    output_sample_rate: u32,
) -> u32 {
    preferred_sample_rate.unwrap_or(output_sample_rate)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudioOutputDevice {
    pub(crate) id: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AudioBufferFallback {
    pub(crate) requested: AudioBufferPolicy,
    pub(crate) active: AudioBufferPolicy,
    pub(crate) underrun_reports: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct AudioHostStatus {
    pub(crate) active_device: Option<AudioOutputDevice>,
    pub(crate) device_fallback: Option<String>,
    pub(crate) buffer_fallback: Option<AudioBufferFallback>,
}

pub(crate) struct AudioHostConfig<'a> {
    pub(crate) preferred_sample_rate: Option<u32>,
    pub(crate) output_device_id: Option<&'a str>,
    pub(crate) buffer_policy: AudioBufferPolicy,
}

pub(crate) struct AudioOutputInit {
    pub(crate) output: AudioOutput,
    pub(crate) status: AudioHostStatus,
}

pub(crate) fn output_devices() -> anyhow::Result<Vec<AudioOutputDevice>> {
    let host = cpal::default_host();
    let mut devices = host
        .output_devices()
        .context("failed to enumerate audio output devices")?
        .filter_map(|device| output_device_info(&device).ok())
        .collect::<Vec<_>>();
    devices.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id)));
    Ok(devices)
}

fn output_device_info(device: &cpal::Device) -> anyhow::Result<AudioOutputDevice> {
    Ok(AudioOutputDevice {
        id: device
            .id()
            .context("failed to obtain audio output device ID")?
            .to_string(),
        name: device
            .description()
            .map(|description| description.name().to_owned())
            .unwrap_or_else(|_| "Unnamed output".to_owned()),
    })
}

pub(super) fn buffer_fallback_for_underruns(
    policy: AudioBufferPolicy,
    underrun_reports: u8,
) -> Option<AudioBufferFallback> {
    (policy == AudioBufferPolicy::LowLatency
        && underrun_reports >= LOW_LATENCY_UNDERRUN_REPORTS_BEFORE_FALLBACK)
        .then_some(AudioBufferFallback {
            requested: policy,
            active: AudioBufferPolicy::Auto,
            underrun_reports,
        })
}

#[derive(Default)]
pub(super) struct UnderrunRecovery {
    incidents: VecDeque<Duration>,
}

impl UnderrunRecovery {
    pub(super) fn observe(
        &mut self,
        policy: AudioBufferPolicy,
        underruns: u64,
        elapsed: Duration,
    ) -> Option<AudioBufferFallback> {
        if policy != AudioBufferPolicy::LowLatency {
            *self = Self::default();
            return None;
        }
        while self
            .incidents
            .front()
            .is_some_and(|start| elapsed.saturating_sub(*start) > LOW_LATENCY_UNDERRUN_WINDOW)
        {
            self.incidents.pop_front();
        }
        if underruns == 0 {
            return None;
        }
        let reports =
            (self.incidents.len() as u8).saturating_add(underruns.min(u8::MAX as u64) as u8);
        let fallback = buffer_fallback_for_underruns(policy, reports);
        if fallback.is_some() {
            *self = Self::default();
        } else {
            self.incidents
                .extend(std::iter::repeat_n(elapsed, underruns as usize));
        }
        fallback
    }
}

pub(super) fn stream_error_message(errors: u64) -> Option<String> {
    (errors != 0).then(|| {
        format!("Audio output device reported {errors} stream error(s); retrying System default.")
    })
}

pub(crate) struct AudioOutput {
    _stream: cpal::Stream,
    producer: rtrb::Producer<QueuedAudioSample>,
    staged_samples: VecDeque<f32>,
    source_sample_rate: u32,
    output_sample_rate: u32,
    capacity: usize,
    low_pass_filter: OnePoleLowPass,
    resampler: Option<resampler::AudioResampler>,
    underruns: Arc<AtomicU64>,
    playback_preroll_samples: Arc<AtomicUsize>,
    session_generation: u64,
    playback_generation: Arc<AtomicU64>,
    playback_speed: usize,
    speedup_samples: Vec<f32>,
    buffer_policy: AudioBufferPolicy,
    underrun_recovery: UnderrunRecovery,
    underrun_clock: crate::platform::Instant,
    buffer_fallback_pending: Option<AudioBufferFallback>,
    stream_errors: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct QueuedAudioSample {
    pub(super) generation: u64,
    pub(super) value: f32,
}

pub(super) struct AudioPlaybackState {
    primed: bool,
    observed_preroll_samples: usize,
    preroll_samples: Arc<AtomicUsize>,
    underruns: Arc<AtomicU64>,
    generation: Arc<AtomicU64>,
    observed_generation: u64,
}

impl AudioPlaybackState {
    pub(super) fn new(
        preroll_samples: Arc<AtomicUsize>,
        underruns: Arc<AtomicU64>,
        generation: Arc<AtomicU64>,
    ) -> Self {
        let observed_preroll_samples = preroll_samples.load(Ordering::Relaxed);
        let observed_generation = generation.load(Ordering::Acquire);
        Self {
            primed: false,
            observed_preroll_samples,
            preroll_samples,
            underruns,
            generation,
            observed_generation,
        }
    }

    fn discard_stale_samples(&mut self, consumer: &mut rtrb::Consumer<QueuedAudioSample>) {
        let generation = self.generation.load(Ordering::Acquire);
        if generation != self.observed_generation {
            self.observed_generation = generation;
            self.primed = false;
        }
        while consumer
            .peek()
            .is_ok_and(|sample| sample.generation != generation)
        {
            let _ = consumer.pop();
        }
    }

    fn ready(&mut self, available: usize, needed: usize) -> bool {
        let preroll_samples = self.preroll_samples.load(Ordering::Relaxed);
        if preroll_samples > self.observed_preroll_samples {
            self.primed = false;
        }
        self.observed_preroll_samples = preroll_samples;
        if self.primed {
            if available >= needed {
                return true;
            }
            self.primed = false;
            self.underruns.fetch_add(1, Ordering::Relaxed);
        }
        if available >= preroll_samples.max(needed) {
            self.primed = true;
            true
        } else {
            false
        }
    }
}

#[derive(Default)]
pub(super) struct OnePoleLowPass {
    left: f32,
    right: f32,
}

impl OnePoleLowPass {
    pub(super) fn reset(&mut self) {
        self.left = 0.0;
        self.right = 0.0;
    }

    pub(super) fn apply_sample(&mut self, sample: f32, channel: usize, alpha: f32) -> f32 {
        if channel & 1 == 0 {
            self.left += alpha * (sample - self.left);
            self.left
        } else {
            self.right += alpha * (sample - self.right);
            self.right
        }
    }
}

pub(super) fn low_pass_alpha(sample_rate: u32, cutoff_hz: u32) -> f32 {
    let clamped_cutoff =
        cutoff_hz.clamp(AUDIO_LOW_PASS_MIN_CUTOFF_HZ, AUDIO_LOW_PASS_MAX_CUTOFF_HZ);
    let rc = 1.0 / (std::f32::consts::TAU * clamped_cutoff as f32);
    let dt = 1.0 / sample_rate.max(1) as f32;
    (dt / (rc + dt)).clamp(0.0, 1.0)
}

impl AudioOutput {
    pub(crate) fn new_with_host_config(
        config: AudioHostConfig<'_>,
    ) -> anyhow::Result<AudioOutputInit> {
        let host = cpal::default_host();
        let selected = config
            .output_device_id
            .filter(|id| !id.trim().is_empty())
            .map(|id| {
                DeviceId::from_str(id)
                    .ok()
                    .and_then(|id| host.device_by_id(&id))
            });
        let selection_failure = match (config.output_device_id, selected.as_ref()) {
            (Some(_), Some(Some(_))) => None,
            (Some(_), _) => {
                Some("Saved output device is unavailable; using System default.".to_owned())
            }
            (None, _) => None,
        };

        if let Some(Some(device)) = selected {
            match Self::new_for_device(&device, config.preferred_sample_rate, config.buffer_policy)
            {
                Ok(output) => {
                    return Ok(AudioOutputInit {
                        output,
                        status: AudioHostStatus {
                            active_device: output_device_info(&device).ok(),
                            device_fallback: None,
                            buffer_fallback: None,
                        },
                    });
                }
                Err(error) => {
                    let fallback = format!(
                        "Saved output device could not start ({error}); using System default."
                    );
                    let default_device = host
                        .default_output_device()
                        .context("saved output device failed and no System default audio output is available")?;
                    let default_info = output_device_info(&default_device).ok();
                    return Self::new_default_with_status(
                        &default_device,
                        default_info,
                        config.preferred_sample_rate,
                        config.buffer_policy,
                        Some(fallback),
                    );
                }
            }
        }

        let default_device = host
            .default_output_device()
            .context("no audio output device found")?;
        let default_info = output_device_info(&default_device).ok();

        Self::new_default_with_status(
            &default_device,
            default_info,
            config.preferred_sample_rate,
            config.buffer_policy,
            selection_failure,
        )
    }

    fn new_default_with_status(
        default_device: &cpal::Device,
        default_info: Option<AudioOutputDevice>,
        preferred_sample_rate: Option<u32>,
        buffer_policy: AudioBufferPolicy,
        device_fallback: Option<String>,
    ) -> anyhow::Result<AudioOutputInit> {
        let output = Self::new_for_device(default_device, preferred_sample_rate, buffer_policy)
            .context("failed to start System default audio output")?;
        Ok(AudioOutputInit {
            output,
            status: AudioHostStatus {
                active_device: default_info,
                device_fallback,
                buffer_fallback: None,
            },
        })
    }

    fn new_for_device(
        device: &cpal::Device,
        preferred_sample_rate: Option<u32>,
        buffer_policy: AudioBufferPolicy,
    ) -> anyhow::Result<Self> {
        let configs = Self::select_output_configs(device, preferred_sample_rate)
            .context("failed to pick audio output config")?;

        let mut last_err = None;
        for config in configs {
            let output_sample_rate = config.sample_rate();
            let source_sample_rate =
                emulator_source_sample_rate(preferred_sample_rate, output_sample_rate);
            let channels = config.channels();
            let capacity = ring_buffer_capacity_for_policy(output_sample_rate, buffer_policy);
            let (producer, consumer) = rtrb::RingBuffer::new(capacity);
            let underruns = Arc::new(AtomicU64::new(0));
            let stream_errors = Arc::new(AtomicU64::new(0));
            let playback_generation = Arc::new(AtomicU64::new(0));
            let playback_preroll_samples = Arc::new(AtomicUsize::new(playback_preroll_samples(
                output_sample_rate,
                buffer_policy.normal_queue_ms(),
            )));
            let playback_state = AudioPlaybackState::new(
                Arc::clone(&playback_preroll_samples),
                Arc::clone(&underruns),
                Arc::clone(&playback_generation),
            );

            match Self::build_stream_for_config(
                device,
                &config,
                consumer,
                playback_state,
                Arc::clone(&stream_errors),
            ) {
                Ok(stream) => {
                    stream.play().context("failed to start audio playback")?;
                    if source_sample_rate != output_sample_rate {
                        log::warn!(
                            "audio output stream is {output_sample_rate} Hz; resampling {source_sample_rate} Hz emulator audio ({:?}, {}ch)",
                            config.sample_format(),
                            channels
                        );
                    }

                    let resampler = match resampler::AudioResampler::new(
                        source_sample_rate,
                        output_sample_rate,
                    ) {
                        Ok(resampler) => Some(resampler),
                        Err(error) if source_sample_rate == output_sample_rate => {
                            log::warn!("Audio resampler init failed: {error}; using passthrough");
                            None
                        }
                        Err(error) => {
                            return Err(error).context(format!(
                                "failed to resample emulator audio from {source_sample_rate} Hz to device rate {output_sample_rate} Hz"
                            ));
                        }
                    };

                    return Ok(Self {
                        _stream: stream,
                        producer,
                        staged_samples: VecDeque::new(),
                        source_sample_rate,
                        output_sample_rate,
                        capacity,
                        low_pass_filter: OnePoleLowPass::default(),
                        resampler,
                        underruns,
                        playback_preroll_samples,
                        session_generation: 0,
                        playback_generation,
                        playback_speed: 1,
                        speedup_samples: Vec::new(),
                        buffer_policy,
                        underrun_recovery: UnderrunRecovery::default(),
                        underrun_clock: crate::platform::Instant::now(),
                        buffer_fallback_pending: None,
                        stream_errors,
                    });
                }
                Err(err) => {
                    log::warn!(
                        "audio output config failed: {:?} {} Hz ({}ch): {err}",
                        config.sample_format(),
                        output_sample_rate,
                        channels
                    );
                    last_err = Some(err);
                }
            }
        }

        if let Some(err) = last_err {
            Err(err)
        } else {
            anyhow::bail!("no audio output configs available")
        }
    }

    fn select_output_configs(
        device: &cpal::Device,
        preferred_sample_rate: Option<u32>,
    ) -> anyhow::Result<Vec<SupportedStreamConfig>> {
        let default = device
            .default_output_config()
            .context("failed to get default audio output config")?;
        let Some(target_rate) = preferred_sample_rate else {
            return Ok(vec![default]);
        };

        let mut candidates: Vec<SupportedStreamConfig> = match device.supported_output_configs() {
            Ok(configs) => configs,
            Err(err) => {
                log::warn!(
                    "failed to enumerate supported output configs for sample rate {target_rate} Hz: {err}"
                );
                return Ok(vec![default]);
            }
        }
        .map(|range| {
            let min_rate = range.min_sample_rate();
            let max_rate = range.max_sample_rate();
            range.with_sample_rate(target_rate.clamp(min_rate, max_rate))
        })
        .collect();

        if candidates.is_empty() {
            return Ok(vec![default]);
        }

        let default_channels = default.channels();
        candidates.sort_by_key(|config| {
            (
                sample_format_rank(config.sample_format()),
                config.sample_rate().abs_diff(target_rate),
                config.channels().abs_diff(default_channels),
            )
        });

        if !candidates
            .iter()
            .any(|config| same_config(config, &default))
        {
            candidates.push(default);
        }

        Ok(candidates)
    }

    fn build_stream_for_config(
        device: &cpal::Device,
        config: &SupportedStreamConfig,
        consumer: rtrb::Consumer<QueuedAudioSample>,
        playback_state: AudioPlaybackState,
        stream_errors: Arc<AtomicU64>,
    ) -> anyhow::Result<cpal::Stream> {
        let channels = config.channels();
        let stream_config: StreamConfig = (*config).into();
        match config.sample_format() {
            SampleFormat::F32 => Self::build_stream_f32(
                device,
                stream_config,
                channels,
                consumer,
                playback_state,
                stream_errors,
            )
            .context("failed to build F32 audio stream"),
            SampleFormat::I16 => Self::build_stream_converting(
                device,
                stream_config,
                channels,
                consumer,
                playback_state,
                stream_errors,
                |s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16,
            )
            .context("failed to build I16 audio stream"),
            SampleFormat::U16 => Self::build_stream_converting(
                device,
                stream_config,
                channels,
                consumer,
                playback_state,
                stream_errors,
                |s| ((s.clamp(-1.0, 1.0) + 1.0) * 0.5 * u16::MAX as f32) as u16,
            )
            .context("failed to build U16 audio stream"),
            SampleFormat::U8 => Self::build_stream_converting(
                device,
                stream_config,
                channels,
                consumer,
                playback_state,
                stream_errors,
                |s| ((s.clamp(-1.0, 1.0) + 1.0) * 0.5 * u8::MAX as f32) as u8,
            )
            .context("failed to build U8 audio stream"),
            other => anyhow::bail!("unsupported audio sample format: {other:?}"),
        }
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
        self.session_generation = self.session_generation.wrapping_add(1);
        self.playback_generation
            .store(self.session_generation, Ordering::Release);
        self.staged_samples.clear();
        self.low_pass_filter.reset();
        if let Some(resampler) = &mut self.resampler {
            resampler.reset();
        }
        self.underruns.store(0, Ordering::Relaxed);
        self.underrun_recovery = UnderrunRecovery::default();
        self.buffer_fallback_pending = None;
    }

    pub(crate) fn queue_samples(&mut self, samples: &[f32], config: &AudioQueueConfig) {
        let playback_speed = config.playback_speed.max(1);
        if playback_speed != self.playback_speed {
            self.discard_queued_samples();
            self.playback_speed = playback_speed;
        }

        flush_staged_samples(
            &mut self.producer,
            &mut self.staged_samples,
            self.session_generation,
        );

        let fast_forward_active = playback_speed > 1;
        if fast_forward_active && config.mute_during_fast_forward {
            return;
        }

        let underruns = self.underruns.swap(0, Ordering::Relaxed);
        if underruns != 0 {
            log::warn!("audio output underrun ({underruns}); rebuffering before playback resumes");
        }
        if self.buffer_fallback_pending.is_none() {
            self.buffer_fallback_pending = self.underrun_recovery.observe(
                self.buffer_policy,
                underruns,
                self.underrun_clock.elapsed(),
            );
        }

        let gain = config.master_volume.clamp(0.0, 1.0);

        let queue_ms = if fast_forward_active {
            FAST_FORWARD_QUEUE_MS
        } else {
            self.buffer_policy.normal_queue_ms()
        };
        self.playback_preroll_samples.store(
            playback_preroll_samples(self.output_sample_rate, queue_ms),
            Ordering::Relaxed,
        );
        let max_queued = (self.output_sample_rate as usize * 2 * queue_ms / 1000).max(2);

        let occupied = self.capacity - self.producer.slots();
        if occupied > max_queued {
            return;
        }

        let buffered = occupied.saturating_add(self.staged_samples.len());
        let fill_ratio = buffered as f32 / self.capacity as f32;

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
            let available = self.producer.slots().min(samples.len()) & !1;
            write_processed_samples(
                &mut self.producer,
                &samples[..available],
                &mut self.low_pass_filter,
                ProcessedSampleConfig {
                    gain,
                    low_pass_enabled: config.low_pass_enabled,
                    alpha,
                    generation: self.session_generation,
                },
            );
            return;
        }

        let max_staged = self.output_sample_rate as usize * 2 * MAX_STAGED_AUDIO_MS / 1000;
        let recovery = long_stall_recovery_range(
            occupied.saturating_add(self.staged_samples.len()),
            samples.len(),
            max_staged,
            max_queued,
        );
        let recovered_samples;
        let samples = if let Some(range) = recovery {
            let dropped = occupied
                .saturating_add(self.staged_samples.len())
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
        if self.staged_samples.try_reserve(samples.len()).is_err() {
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
            self.staged_samples.push_back(out);
        }
        flush_staged_samples(
            &mut self.producer,
            &mut self.staged_samples,
            self.session_generation,
        );
    }

    fn build_stream_f32(
        device: &cpal::Device,
        config: StreamConfig,
        channels: u16,
        mut consumer: rtrb::Consumer<QueuedAudioSample>,
        mut playback_state: AudioPlaybackState,
        stream_errors: Arc<AtomicU64>,
    ) -> Result<cpal::Stream, cpal::Error> {
        device.build_output_stream(
            config,
            move |data: &mut [f32], _| {
                fill_output_f32(data, channels, &mut consumer, &mut playback_state);
            },
            move |err| {
                stream_errors.fetch_add(1, Ordering::Relaxed);
                log::error!("audio stream error: {err}");
            },
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn build_stream_converting<S: cpal::SizedSample + Send + 'static>(
        device: &cpal::Device,
        config: StreamConfig,
        channels: u16,
        mut consumer: rtrb::Consumer<QueuedAudioSample>,
        mut playback_state: AudioPlaybackState,
        stream_errors: Arc<AtomicU64>,
        convert: fn(f32) -> S,
    ) -> Result<cpal::Stream, cpal::Error> {
        let mut scratch = Vec::<f32>::with_capacity(4096);
        device.build_output_stream(
            config,
            move |data: &mut [S], _| {
                scratch.resize(data.len(), 0.0);
                fill_output_f32(&mut scratch, channels, &mut consumer, &mut playback_state);
                for (dst, &sample) in data.iter_mut().zip(scratch.iter()) {
                    *dst = convert(sample);
                }
            },
            move |err| {
                stream_errors.fetch_add(1, Ordering::Relaxed);
                log::error!("audio stream error: {err}");
            },
            None,
        )
    }
}

pub(super) fn long_stall_recovery_range(
    pending_samples: usize,
    incoming_samples: usize,
    max_pending_samples: usize,
    recent_samples: usize,
) -> Option<std::ops::Range<usize>> {
    if pending_samples.saturating_add(incoming_samples) <= max_pending_samples {
        return None;
    }
    let end = incoming_samples & !1;
    let retain = end.min(recent_samples & !1);
    Some(end - retain..end)
}

#[derive(Clone, Copy)]
struct ProcessedSampleConfig {
    gain: f32,
    low_pass_enabled: bool,
    alpha: f32,
    generation: u64,
}

fn write_processed_samples(
    producer: &mut rtrb::Producer<QueuedAudioSample>,
    samples: &[f32],
    low_pass_filter: &mut OnePoleLowPass,
    config: ProcessedSampleConfig,
) {
    if let Ok(mut chunk) = producer.write_chunk_uninit(samples.len()) {
        let (first, second) = chunk.as_mut_slices();
        let first_len = first.len();
        for (index, (dst, &src)) in first.iter_mut().zip(samples).enumerate() {
            let out = if config.low_pass_enabled {
                low_pass_filter.apply_sample(src * config.gain, index, config.alpha)
            } else {
                src * config.gain
            };
            dst.write(QueuedAudioSample {
                generation: config.generation,
                value: out,
            });
        }
        for (offset, (dst, &src)) in second.iter_mut().zip(&samples[first_len..]).enumerate() {
            let index = first_len + offset;
            let out = if config.low_pass_enabled {
                low_pass_filter.apply_sample(src * config.gain, index, config.alpha)
            } else {
                src * config.gain
            };
            dst.write(QueuedAudioSample {
                generation: config.generation,
                value: out,
            });
        }
        unsafe {
            chunk.commit_all();
        }
    }
}

pub(super) fn flush_staged_samples(
    producer: &mut rtrb::Producer<QueuedAudioSample>,
    staged_samples: &mut VecDeque<f32>,
    generation: u64,
) {
    let available = producer.slots().min(staged_samples.len());
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

pub(super) fn fill_output_f32(
    data: &mut [f32],
    channels: u16,
    consumer: &mut rtrb::Consumer<QueuedAudioSample>,
    playback_state: &mut AudioPlaybackState,
) {
    playback_state.discard_stale_samples(consumer);
    if channels == 0 {
        data.fill(0.0);
        return;
    }

    let stereo_samples_needed = (data.len() / channels as usize).saturating_mul(2) & !1;
    if stereo_samples_needed == 0 {
        data.fill(0.0);
        return;
    }
    let samples_to_read = if stereo_samples_needed > consumer.buffer().capacity() {
        playback_state.primed = false;
        playback_state.underruns.fetch_add(1, Ordering::Relaxed);
        consumer.slots() & !1
    } else if playback_state.ready(consumer.slots(), stereo_samples_needed) {
        stereo_samples_needed
    } else {
        0
    };

    if samples_to_read > 0 {
        if let Ok(chunk) = consumer.read_chunk(samples_to_read) {
            let (first, second) = chunk.as_slices();
            let mut src_iter = first.iter().chain(second.iter());
            let frames_from_chunk = samples_to_read / 2;
            for frame in data.chunks_mut(channels as usize).take(frames_from_chunk) {
                let left = src_iter.next().map_or(0.0, |sample| sample.value);
                let right = src_iter.next().map_or(left, |sample| sample.value);
                if channels == 1 {
                    frame[0] = (left + right) * STEREO_MIX_FACTOR;
                } else {
                    frame[0] = left;
                    frame[1] = right;
                    for channel in frame.iter_mut().skip(2) {
                        *channel = (left + right) * STEREO_MIX_FACTOR;
                    }
                }
            }
            chunk.commit_all();
            for frame in data.chunks_mut(channels as usize).skip(frames_from_chunk) {
                frame.fill(0.0);
            }
        } else {
            data.fill(0.0);
        }
    } else {
        data.fill(0.0);
    }
}
