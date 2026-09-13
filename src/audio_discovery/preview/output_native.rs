use std::sync::{Arc, atomic::Ordering};

use anyhow::{Context, Result, bail, ensure};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, Stream, SupportedStreamConfig};

use super::Shared;

const QUEUE_ALIGNMENT_FRAMES: usize = 256;
const PREFERRED_SAMPLE_RATES: [u32; 3] =
    [super::super::render::DEFAULT_SAMPLE_RATE, 44_100, 96_000];

pub(super) fn queue_frames(rate: u32) -> usize {
    (rate as usize / 2).div_ceil(QUEUE_ALIGNMENT_FRAMES) * QUEUE_ALIGNMENT_FRAMES
}

#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub(super) generation: u64,
    pub(super) position: usize,
    pub(super) pcm: [i16; 2],
}

pub(crate) struct Callback {
    consumer: rtrb::Consumer<Frame>,
    shared: Arc<Shared>,
    held: Option<Frame>,
    generation: u32,
    buffering: bool,
}

impl Callback {
    fn new(consumer: rtrb::Consumer<Frame>, shared: Arc<Shared>) -> Self {
        Self {
            consumer,
            shared,
            held: None,
            generation: 0,
            buffering: true,
        }
    }

    fn next(&mut self) -> [f32; 2] {
        self.next_after_pop(|| {})
    }

    fn next_after_pop(&mut self, after_pop: impl FnOnce()) -> [f32; 2] {
        let shared = &self.shared;
        if shared.cancel.load(Ordering::Acquire) {
            self.held = None;
            return [0.0; 2];
        }
        let generation = shared.generation();
        if self
            .held
            .is_some_and(|frame| frame.generation != u64::from(generation))
        {
            self.held = None;
        }
        // A seek invalidates queued audio before the worker reconstructs synthesis state.
        for _ in 0..shared.queue_capacity.load(Ordering::Relaxed) {
            if self
                .consumer
                .peek()
                .is_ok_and(|frame| frame.generation != u64::from(generation))
            {
                let _ = self.consumer.pop();
            } else {
                break;
            }
        }
        if !shared.playing.load(Ordering::Acquire)
            || shared.ready.load(Ordering::Acquire) != generation
        {
            return [0.0; 2];
        }
        let Some(frame) = self.held.take().or_else(|| self.consumer.pop().ok()) else {
            return [0.0; 2];
        };
        after_pop();
        if frame.generation != u64::from(generation)
            || shared.generation() != generation
            || shared.cancel.load(Ordering::Acquire)
        {
            return [0.0; 2];
        }
        if !shared.playing.load(Ordering::Acquire) {
            self.held = Some(frame);
            return [0.0; 2];
        }
        if shared
            .cursor
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |cursor| {
                ((cursor >> 32) as u32 == generation)
                    .then_some((u64::from(generation) << 32) | (frame.position + 1) as u64)
            })
            .is_err()
        {
            return [0.0; 2];
        }
        let gain = shared.volume.load(Ordering::Relaxed) as f32 / 100.0 / 32768.0;
        [
            f32::from(frame.pcm[0]) * gain,
            f32::from(frame.pcm[1]) * gain,
        ]
    }

    pub(crate) fn fill<T: SizedSample + FromSample<f32>>(
        &mut self,
        data: &mut [T],
        channels: usize,
    ) {
        let frames = data.len().div_ceil(channels);
        self.shared
            .callback_frames
            .fetch_max(frames, Ordering::Relaxed);
        if !self.begin_callback(frames) {
            data.fill(T::from_sample(0.0));
            return;
        }
        for frame in data.chunks_mut(channels) {
            let [left, right] = self.next();
            for (channel, sample) in frame.iter_mut().enumerate() {
                let value = match (channels, channel) {
                    (1, _) => (left + right) * 0.5,
                    (_, 0) => left,
                    (_, 1) => right,
                    _ => 0.0,
                };
                *sample = T::from_sample(value);
            }
        }
    }

    fn begin_callback(&mut self, frames: usize) -> bool {
        let shared = &self.shared;
        let generation = shared.generation();
        if self.generation != generation {
            self.generation = generation;
            self.buffering = true;
            self.held = None;
        }
        for _ in 0..shared.queue_capacity.load(Ordering::Relaxed) {
            if self
                .consumer
                .peek()
                .is_ok_and(|frame| frame.generation != u64::from(generation))
            {
                let _ = self.consumer.pop();
            } else {
                break;
            }
        }
        if frames > shared.queue_capacity.load(Ordering::Relaxed) {
            shared.device_error.store(true, Ordering::Release);
            return false;
        }
        if shared.cancel.load(Ordering::Acquire)
            || !shared.playing.load(Ordering::Acquire)
            || shared.ready.load(Ordering::Acquire) != generation
        {
            return false;
        }
        let cursor = shared.cursor.load(Ordering::Acquire);
        let remaining = shared
            .duration
            .load(Ordering::Acquire)
            .saturating_sub(cursor as u32 as usize);
        if remaining == 0 {
            return false;
        }
        let needed = if self.buffering {
            shared.preroll_frames()
        } else {
            frames
        }
        .min(remaining);
        if self.consumer.slots() + usize::from(self.held.is_some()) < needed {
            if !self.buffering {
                shared.underruns.fetch_add(1, Ordering::Relaxed);
            }
            // Resume at a callback boundary after rebuilding reserve, never between empty pops.
            self.buffering = true;
            return false;
        }
        self.buffering = false;
        true
    }
}

pub(super) enum OutputMode {
    Device,
    #[cfg(test)]
    Capture(std::sync::mpsc::SyncSender<Callback>),
}

pub(super) fn open(
    shared: &Arc<Shared>,
    mode: OutputMode,
) -> Result<(Option<Stream>, rtrb::Producer<Frame>, u32)> {
    #[cfg(test)]
    if let OutputMode::Capture(sender) = mode {
        let (producer, callback) = queue(shared, super::super::render::DEFAULT_SAMPLE_RATE);
        sender
            .send(callback)
            .map_err(|_| anyhow::anyhow!("preview test output disconnected"))?;
        return Ok((None, producer, super::super::render::DEFAULT_SAMPLE_RATE));
    }
    let _ = mode;
    ensure!(
        std::env::var("ZEFF_MUTE_AUDIO").as_deref() != Ok("1"),
        "preview output is disabled by ZEFF_MUTE_AUDIO"
    );
    let device = cpal::default_host()
        .default_output_device()
        .context("no default audio output device")?;
    let config = choose_config(&device)?;
    let rate = config.sample_rate();
    let (producer, callback) = queue(shared, rate);
    let stream = match config.sample_format() {
        SampleFormat::F32 => build::<f32>(&device, config, callback),
        SampleFormat::F64 => build::<f64>(&device, config, callback),
        SampleFormat::I16 => build::<i16>(&device, config, callback),
        SampleFormat::U16 => build::<u16>(&device, config, callback),
        SampleFormat::I8 => build::<i8>(&device, config, callback),
        SampleFormat::U8 => build::<u8>(&device, config, callback),
        SampleFormat::I32 => build::<i32>(&device, config, callback),
        SampleFormat::U32 => build::<u32>(&device, config, callback),
        SampleFormat::I64 => build::<i64>(&device, config, callback),
        SampleFormat::U64 => build::<u64>(&device, config, callback),
        format => bail!("preview does not support audio output format {format:?}"),
    }?;
    if let Ok(frames) = stream.buffer_size() {
        shared
            .callback_frames
            .fetch_max(frames as usize, Ordering::Relaxed);
    }
    ensure!(
        shared.callback_frames.load(Ordering::Relaxed) <= queue_frames(rate),
        "preview device buffer exceeds the supported half-second queue"
    );
    stream
        .play()
        .context("could not start preview audio output")?;
    Ok((Some(stream), producer, rate))
}

fn queue(shared: &Arc<Shared>, rate: u32) -> (rtrb::Producer<Frame>, Callback) {
    let capacity = queue_frames(rate);
    shared.sample_rate.store(rate, Ordering::Release);
    shared.queue_capacity.store(capacity, Ordering::Release);
    let (producer, consumer) = rtrb::RingBuffer::new(capacity);
    (producer, Callback::new(consumer, Arc::clone(shared)))
}

fn choose_config(device: &cpal::Device) -> Result<SupportedStreamConfig> {
    let supported = |config: &SupportedStreamConfig| {
        config.channels() != 0 && PREFERRED_SAMPLE_RATES.contains(&config.sample_rate())
    };
    if let Ok(config) = device.default_output_config()
        && supported(&config)
    {
        return Ok(config);
    }
    let configs = device.supported_output_configs()?.collect::<Vec<_>>();
    for rate in PREFERRED_SAMPLE_RATES {
        for config in &configs {
            if config.channels() != 0
                && config.min_sample_rate() <= rate
                && rate <= config.max_sample_rate()
            {
                return Ok((*config).with_sample_rate(rate));
            }
        }
    }
    bail!("preview needs an output device supporting 44.1, 48, or 96 kHz")
}

fn build<T: SizedSample + FromSample<f32>>(
    device: &cpal::Device,
    config: SupportedStreamConfig,
    mut callback: Callback,
) -> Result<Stream> {
    let channels = usize::from(config.channels());
    let shared = Arc::clone(&callback.shared);
    device
        .build_output_stream(
            config.config(),
            move |data: &mut [T], _| callback.fill(data, channels),
            move |_| {
                shared.device_error.store(true, Ordering::Release);
            },
            None,
        )
        .context("could not open preview audio output")
}

#[cfg(test)]
#[path = "output/tests.rs"]
mod tests;
