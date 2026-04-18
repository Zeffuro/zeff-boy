use anyhow::Context;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig, SupportedStreamConfig};

use super::AudioQueueConfig;
use super::resampler;

const NORMAL_QUEUE_MS: usize = 200;
const FAST_FORWARD_QUEUE_MS: usize = 40;
const STEREO_MIX_FACTOR: f32 = 0.5;
const AUDIO_LOW_PASS_MIN_CUTOFF_HZ: u32 = 20;
const AUDIO_LOW_PASS_MAX_CUTOFF_HZ: u32 = 20_000;

pub(super) fn ring_buffer_capacity(sample_rate: u32) -> usize {
    sample_rate as usize * 2 * NORMAL_QUEUE_MS / 1000
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

pub(crate) struct AudioOutput {
    _stream: cpal::Stream,
    producer: rtrb::Producer<f32>,
    sample_rate: u32,
    capacity: usize,
    low_pass_filter: OnePoleLowPass,
    resampler: Option<resampler::AudioResampler>,
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
    pub(crate) fn new(preferred_sample_rate: Option<u32>) -> anyhow::Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .context("no audio output device found")?;

        let configs = Self::select_output_configs(&device, preferred_sample_rate)
            .context("failed to pick audio output config")?;

        let mut last_err = None;
        for config in configs {
            let sample_rate = config.sample_rate();
            let channels = config.channels();
            let capacity = ring_buffer_capacity(sample_rate);
            let (producer, consumer) = rtrb::RingBuffer::new(capacity);

            match Self::build_stream_for_config(&device, &config, consumer) {
                Ok(stream) => {
                    stream.play().context("failed to start audio playback")?;
                    if let Some(target) = preferred_sample_rate
                        && sample_rate != target
                    {
                        log::warn!(
                            "requested audio sample rate {target} Hz not available; using {sample_rate} Hz ({:?}, {}ch)",
                            config.sample_format(),
                            channels
                        );
                    }

                    let resampler = resampler::AudioResampler::new(sample_rate, sample_rate)
                        .map_err(|e| {
                            log::warn!("Audio resampler init failed: {e}; using passthrough")
                        })
                        .ok();

                    return Ok(Self {
                        _stream: stream,
                        producer,
                        sample_rate,
                        capacity,
                        low_pass_filter: OnePoleLowPass::default(),
                        resampler,
                    });
                }
                Err(err) => {
                    log::warn!(
                        "audio output config failed: {:?} {} Hz ({}ch): {err}",
                        config.sample_format(),
                        sample_rate,
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
        consumer: rtrb::Consumer<f32>,
    ) -> anyhow::Result<cpal::Stream> {
        let channels = config.channels();
        let stream_config: StreamConfig = config.clone().into();
        match config.sample_format() {
            SampleFormat::F32 => Self::build_stream_f32(device, &stream_config, channels, consumer)
                .context("failed to build F32 audio stream"),
            SampleFormat::I16 => {
                Self::build_stream_converting(device, &stream_config, channels, consumer, |s| {
                    (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
                })
                .context("failed to build I16 audio stream")
            }
            SampleFormat::U16 => {
                Self::build_stream_converting(device, &stream_config, channels, consumer, |s| {
                    ((s.clamp(-1.0, 1.0) + 1.0) * 0.5 * u16::MAX as f32) as u16
                })
                .context("failed to build U16 audio stream")
            }
            SampleFormat::U8 => {
                Self::build_stream_converting(device, &stream_config, channels, consumer, |s| {
                    ((s.clamp(-1.0, 1.0) + 1.0) * 0.5 * u8::MAX as f32) as u8
                })
                .context("failed to build U8 audio stream")
            }
            other => anyhow::bail!("unsupported audio sample format: {other:?}"),
        }
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn queue_samples(&mut self, samples: &[f32], config: &AudioQueueConfig) {
        if config.fast_forward_active && config.mute_during_fast_forward {
            return;
        }

        let gain = config.master_volume.clamp(0.0, 1.0);

        let queue_ms = if config.fast_forward_active {
            FAST_FORWARD_QUEUE_MS
        } else {
            NORMAL_QUEUE_MS
        };
        let max_queued = (self.sample_rate as usize * 2 * queue_ms / 1000).max(2);

        let occupied = self.capacity - self.producer.slots();
        if occupied > max_queued {
            return;
        }

        let fill_ratio = occupied as f32 / self.capacity as f32;

        let resampled;
        let samples = if let Some(ref mut resampler) = self.resampler {
            resampled = resampler.process(samples, fill_ratio);
            &resampled
        } else {
            samples
        };

        let available = self.producer.slots().min(samples.len());
        if available == 0 {
            return;
        }

        if !config.low_pass_enabled {
            self.low_pass_filter.reset();
        }
        let alpha = low_pass_alpha(self.sample_rate, config.low_pass_cutoff_hz);

        if let Ok(mut chunk) = self.producer.write_chunk_uninit(available) {
            let (first, second) = chunk.as_mut_slices();
            let first_len = first.len();
            let mut global_idx = 0usize;
            for slices in [first.iter_mut().zip(&samples[..first_len]), second.iter_mut().zip(&samples[first_len..available])] {
                for (dst, &src) in slices {
                    let mut out = src * gain;
                    if config.low_pass_enabled {
                        out = self.low_pass_filter.apply_sample(out, global_idx, alpha);
                    }
                    dst.write(out);
                    global_idx += 1;
                }
            }
            unsafe {
                chunk.commit_all();
            }
        }
    }

    fn build_stream_f32(
        device: &cpal::Device,
        config: &StreamConfig,
        channels: u16,
        mut consumer: rtrb::Consumer<f32>,
    ) -> Result<cpal::Stream, cpal::BuildStreamError> {
        device.build_output_stream(
            config,
            move |data: &mut [f32], _| {
                fill_output_f32(data, channels, &mut consumer);
            },
            |err| log::error!("audio stream error: {err}"),
            None,
        )
    }

    fn build_stream_converting<S: cpal::SizedSample + Send + 'static>(
        device: &cpal::Device,
        config: &StreamConfig,
        channels: u16,
        mut consumer: rtrb::Consumer<f32>,
        convert: fn(f32) -> S,
    ) -> Result<cpal::Stream, cpal::BuildStreamError> {
        let mut scratch = Vec::<f32>::with_capacity(4096);
        device.build_output_stream(
            config,
            move |data: &mut [S], _| {
                scratch.resize(data.len(), 0.0);
                fill_output_f32(&mut scratch, channels, &mut consumer);
                for (dst, &sample) in data.iter_mut().zip(scratch.iter()) {
                    *dst = convert(sample);
                }
            },
            |err| log::error!("audio stream error: {err}"),
            None,
        )
    }
}

pub(super) fn fill_output_f32(data: &mut [f32], channels: u16, consumer: &mut rtrb::Consumer<f32>) {
    if channels < 2 {
        let available = consumer.slots().min(data.len());
        if let Ok(chunk) = consumer.read_chunk(available) {
            let (first, second) = chunk.as_slices();
            data[..first.len()].copy_from_slice(first);
            data[first.len()..first.len() + second.len()].copy_from_slice(second);
            chunk.commit_all();
            for sample in &mut data[available..] {
                *sample = 0.0;
            }
        } else {
            data.fill(0.0);
        }
        return;
    }

    let stereo_samples_needed = data.len() / channels as usize * 2;
    let available = consumer.slots().min(stereo_samples_needed);
    let even_available = available & !1;

    if even_available > 0 {
        if let Ok(chunk) = consumer.read_chunk(even_available) {
            let (first, second) = chunk.as_slices();
            let mut src_iter = first.iter().chain(second.iter());
            let frames_from_chunk = even_available / 2;
            for frame in data.chunks_mut(channels as usize).take(frames_from_chunk) {
                let left = *src_iter.next().unwrap_or(&0.0);
                let right = *src_iter.next().unwrap_or(&left);
                frame[0] = left;
                frame[1] = right;
                for channel in frame.iter_mut().skip(2) {
                    *channel = (left + right) * STEREO_MIX_FACTOR;
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
