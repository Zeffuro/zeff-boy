use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

const NORMAL_QUEUE_MS: usize = 200;
const FAST_FORWARD_QUEUE_MS: usize = 40;

const RING_BUFFER_CAPACITY: usize = 48_000 * 2 * NORMAL_QUEUE_MS / 1000;

pub(crate) struct AudioOutput {
    _stream: cpal::Stream,
    producer: rtrb::Producer<f32>,
    sample_rate: u32,
}

impl AudioOutput {
    pub(crate) fn new() -> Option<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let config = device.default_output_config().ok()?;

        let sample_rate = config.sample_rate();
        let channels = config.channels();
        let stream_config: StreamConfig = config.clone().into();

        let (producer, consumer) = rtrb::RingBuffer::new(RING_BUFFER_CAPACITY);

        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                Self::build_stream_f32(&device, &stream_config, channels, consumer)?
            }
            SampleFormat::I16 => {
                Self::build_stream_i16(&device, &stream_config, channels, consumer)?
            }
            SampleFormat::U16 => {
                Self::build_stream_u16(&device, &stream_config, channels, consumer)?
            }
            _ => return None,
        };

        stream.play().ok()?;
        Some(Self {
            _stream: stream,
            producer,
            sample_rate,
        })
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn queue_samples(
        &mut self,
        samples: &[f32],
        master_volume: f32,
        fast_forward_active: bool,
        mute_during_fast_forward: bool,
    ) {
        if fast_forward_active && mute_during_fast_forward {
            return;
        }

        let gain = master_volume.clamp(0.0, 1.0);

        let queue_ms = if fast_forward_active {
            FAST_FORWARD_QUEUE_MS
        } else {
            NORMAL_QUEUE_MS
        };
        let max_samples = (self.sample_rate as usize * queue_ms / 1000).max(2);

        let occupied = RING_BUFFER_CAPACITY - self.producer.slots();
        if occupied > max_samples {
            return;
        }

        for &sample in samples {
            if self.producer.push(sample * gain).is_err() {
                break;
            }
        }
    }

    fn build_stream_f32(
        device: &cpal::Device,
        config: &StreamConfig,
        channels: u16,
        mut consumer: rtrb::Consumer<f32>,
    ) -> Option<cpal::Stream> {
        device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    fill_output_f32(data, channels, &mut consumer);
                },
                |err| eprintln!("audio error: {err}"),
                None,
            )
            .ok()
    }

    fn build_stream_i16(
        device: &cpal::Device,
        config: &StreamConfig,
        channels: u16,
        mut consumer: rtrb::Consumer<f32>,
    ) -> Option<cpal::Stream> {
        let mut scratch = Vec::<f32>::new();
        device
            .build_output_stream(
                config,
                move |data: &mut [i16], _| {
                    scratch.resize(data.len(), 0.0);
                    fill_output_f32(&mut scratch, channels, &mut consumer);
                    for (dst, sample) in data.iter_mut().zip(scratch.iter()) {
                        *dst = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                },
                |err| eprintln!("audio error: {err}"),
                None,
            )
            .ok()
    }

    fn build_stream_u16(
        device: &cpal::Device,
        config: &StreamConfig,
        channels: u16,
        mut consumer: rtrb::Consumer<f32>,
    ) -> Option<cpal::Stream> {
        let mut scratch = Vec::<f32>::new();
        device
            .build_output_stream(
                config,
                move |data: &mut [u16], _| {
                    scratch.resize(data.len(), 0.0);
                    fill_output_f32(&mut scratch, channels, &mut consumer);
                    for (dst, sample) in data.iter_mut().zip(scratch.iter()) {
                        let normalized = (sample.clamp(-1.0, 1.0) + 1.0) * 0.5;
                        *dst = (normalized * u16::MAX as f32) as u16;
                    }
                },
                |err| eprintln!("audio error: {err}"),
                None,
            )
            .ok()
    }
}

fn fill_output_f32(data: &mut [f32], channels: u16, consumer: &mut rtrb::Consumer<f32>) {
    if channels < 2 {
        for sample in data.iter_mut() {
            *sample = consumer.pop().unwrap_or(0.0);
        }
        return;
    }

    for frame in data.chunks_mut(channels as usize) {
        let left = consumer.pop().unwrap_or(0.0);
        let right = consumer.pop().unwrap_or(left);
        frame[0] = left;
        frame[1] = right;
        for channel in frame.iter_mut().skip(2) {
            *channel = (left + right) * 0.5;
        }
    }
}
