use super::AudioQueueConfig;
use wasm_bindgen::prelude::*;
use web_sys::AudioContext;

const BUFFER_FRAMES: usize = 1024;

const MAX_QUEUE_AHEAD_SECS: f64 = 0.12;

const CATCHUP_OFFSET_SECS: f64 = 0.010;

pub(crate) struct AudioOutput {
    ctx: AudioContext,
    sample_rate: u32,
    buffer: Vec<f32>,
    next_play_time: f64,
    left: Vec<f32>,
    right: Vec<f32>,
}

impl AudioOutput {
    pub(crate) fn new(_preferred_sample_rate: Option<u32>) -> anyhow::Result<Self> {
        let ctx = AudioContext::new()
            .map_err(|e| anyhow::anyhow!("failed to create AudioContext: {e:?}"))?;

        let sample_rate = ctx.sample_rate() as u32;

        Ok(Self {
            ctx,
            sample_rate,
            buffer: Vec::with_capacity(BUFFER_FRAMES * 4),
            next_play_time: 0.0,
            left: Vec::with_capacity(BUFFER_FRAMES),
            right: Vec::with_capacity(BUFFER_FRAMES),
        })
    }

    pub(crate) fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(crate) fn queue_samples(&mut self, samples: &[f32], config: &AudioQueueConfig) {
        if config.fast_forward_active && config.mute_during_fast_forward {
            self.buffer.clear();
            return;
        }

        let gain = config.master_volume.clamp(0.0, 1.0);

        for &s in samples {
            self.buffer.push(s * gain);
        }

        while self.buffer.len() >= BUFFER_FRAMES * 2 {
            let current_time = self.ctx.current_time();

            if self.next_play_time < current_time {
                self.next_play_time = current_time + CATCHUP_OFFSET_SECS;
            }

            if self.next_play_time > current_time + MAX_QUEUE_AHEAD_SECS {
                break;
            }

            let Ok(audio_buffer) =
                self.ctx
                    .create_buffer(2, BUFFER_FRAMES as u32, self.sample_rate as f32)
            else {
                self.buffer.drain(..BUFFER_FRAMES * 2);
                continue;
            };

            self.left.clear();
            self.right.clear();
            for pair in self.buffer[..BUFFER_FRAMES * 2].chunks_exact(2) {
                self.left.push(pair[0]);
                self.right.push(pair[1]);
            }
            self.buffer.drain(..BUFFER_FRAMES * 2);

            let _ = audio_buffer.copy_to_channel(&self.left, 0);
            let _ = audio_buffer.copy_to_channel(&self.right, 1);

            if let Ok(source) = self.ctx.create_buffer_source() {
                source.set_buffer(Some(&audio_buffer));
                let _ = source.connect_with_audio_node(&self.ctx.destination());
                let _ = source.start_with_when(self.next_play_time);
            }

            self.next_play_time += BUFFER_FRAMES as f64 / self.sample_rate as f64;
        }

        let max_buffered = self.sample_rate as usize * 2 * 200 / 1000;
        if self.buffer.len() > max_buffered {
            let excess = self.buffer.len() - max_buffered;
            let drop = excess & !1;
            self.buffer.drain(..drop);
        }
    }
}
