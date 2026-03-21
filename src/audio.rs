use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};

const NORMAL_QUEUE_MS: usize = 200;
const FAST_FORWARD_QUEUE_MS: usize = 40;

pub(crate) struct AudioOutput {
    _stream: cpal::Stream,
    buffer: Arc<Mutex<VecDeque<f32>>>,
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
        let buffer: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));

        let stream = match config.sample_format() {
            SampleFormat::F32 => {
                Self::build_stream_f32(&device, &stream_config, channels, buffer.clone())?
            }
            SampleFormat::I16 => {
                Self::build_stream_i16(&device, &stream_config, channels, buffer.clone())?
            }
            SampleFormat::U16 => {
                Self::build_stream_u16(&device, &stream_config, channels, buffer.clone())?
            }
            _ => return None,
        };

        stream.play().ok()?;
        Some(Self {
            _stream: stream,
            buffer,
            sample_rate,
        })
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn queue_samples(
        &self,
        samples: &[f32],
        master_volume: f32,
        fast_forward_active: bool,
        mute_during_fast_forward: bool,
    ) {
        let mut buf = self.buffer.lock().expect("audio queue mutex poisoned");
        if fast_forward_active && mute_during_fast_forward {
            buf.clear();
            return;
        }

        let gain = master_volume.clamp(0.0, 1.0);

        buf.reserve(samples.len());
        for sample in samples {
            buf.push_back(*sample * gain);
        }

        let queue_ms = if fast_forward_active {
            FAST_FORWARD_QUEUE_MS
        } else {
            NORMAL_QUEUE_MS
        };
        let max_samples = (self.sample_rate as usize * queue_ms / 1000).max(2);
        if buf.len() > max_samples {
            let trim = buf.len() - max_samples;
            drop(buf.drain(..trim));
        }
    }

    fn build_stream_f32(
        device: &cpal::Device,
        config: &StreamConfig,
        channels: u16,
        buffer: Arc<Mutex<VecDeque<f32>>>,
    ) -> Option<cpal::Stream> {
        device
            .build_output_stream(
                config,
                move |data: &mut [f32], _| {
                    Self::fill_output_f32(data, channels, &buffer);
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
        buffer: Arc<Mutex<VecDeque<f32>>>,
    ) -> Option<cpal::Stream> {
        device
            .build_output_stream(
                config,
                move |data: &mut [i16], _| {
                    let mut temp = vec![0.0f32; data.len()];
                    Self::fill_output_f32(&mut temp, channels, &buffer);
                    for (dst, sample) in data.iter_mut().zip(temp.iter()) {
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
        buffer: Arc<Mutex<VecDeque<f32>>>,
    ) -> Option<cpal::Stream> {
        device
            .build_output_stream(
                config,
                move |data: &mut [u16], _| {
                    let mut temp = vec![0.0f32; data.len()];
                    Self::fill_output_f32(&mut temp, channels, &buffer);
                    for (dst, sample) in data.iter_mut().zip(temp.iter()) {
                        let normalized = (sample.clamp(-1.0, 1.0) + 1.0) * 0.5;
                        *dst = (normalized * u16::MAX as f32) as u16;
                    }
                },
                |err| eprintln!("audio error: {err}"),
                None,
            )
            .ok()
    }

    fn fill_output_f32(data: &mut [f32], channels: u16, buffer: &Arc<Mutex<VecDeque<f32>>>) {
        let mut buf = buffer.lock().expect("audio callback mutex poisoned");
        if channels < 2 {
            for sample in data.iter_mut() {
                *sample = buf.pop_front().unwrap_or(0.0);
            }
            return;
        }

        for frame in data.chunks_mut(channels as usize) {
            let left = buf.pop_front().unwrap_or(0.0);
            let right = buf.pop_front().unwrap_or(left);
            frame[0] = left;
            frame[1] = right;
            for channel in frame.iter_mut().skip(2) {
                *channel = (left + right) * 0.5;
            }
        }
    }
}
