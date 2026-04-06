use super::AudioQueueConfig;
use wasm_bindgen::prelude::*;
use web_sys::AudioContext;

const WEB_SAMPLE_RATE: u32 = 48000;
const BUFFER_SIZE: usize = 2048;

pub(crate) struct AudioOutput {
    ctx: AudioContext,
    sample_rate: u32,
    buffer: Vec<f32>,
}

impl AudioOutput {
    pub(crate) fn new(_preferred_sample_rate: Option<u32>) -> anyhow::Result<Self> {
        let ctx = AudioContext::new()
            .map_err(|e| anyhow::anyhow!("failed to create AudioContext: {e:?}"))?;

        let sample_rate = ctx.sample_rate() as u32;

        Ok(Self {
            ctx,
            sample_rate,
            buffer: Vec::with_capacity(BUFFER_SIZE * 2),
        })
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn queue_samples(&mut self, samples: &[f32], config: &AudioQueueConfig) {
        if config.fast_forward_active && config.mute_during_fast_forward {
            return;
        }

        let gain = config.master_volume.clamp(0.0, 1.0);

        for &s in samples {
            self.buffer.push(s * gain);
        }

        if self.buffer.len() < BUFFER_SIZE * 2 {
            return;
        }

        let frames = self.buffer.len() / 2;
        let Ok(audio_buffer) = self.ctx.create_buffer(2, frames as u32, self.sample_rate as f32)
        else {
            self.buffer.clear();
            return;
        };

        let mut left = Vec::with_capacity(frames);
        let mut right = Vec::with_capacity(frames);
        for chunk in self.buffer.chunks_exact(2) {
            left.push(chunk[0]);
            right.push(chunk[1]);
        }

        let _ = audio_buffer.copy_to_channel(&left, 0);
        let _ = audio_buffer.copy_to_channel(&right, 1);

        if let Ok(source) = self.ctx.create_buffer_source() {
            source.set_buffer(Some(&audio_buffer));
            let _ = source.connect_with_audio_node(&self.ctx.destination());
            let _ = source.start();
        }

        self.buffer.clear();
    }
}

