use std::collections::VecDeque;

use super::{Apu, ApuChannelSnapshot, DEBUG_SAMPLE_CAPACITY, INITIAL_SAMPLE_CAPACITY};
use crate::hardware::constants::*;

impl Apu {
    pub(super) fn generate_sample(&mut self) {
        if !self.sample_generation_enabled {
            return;
        }

        self.sample_accumulator += self.output_sample_rate;
        if self.sample_accumulator >= APU_CPU_CLOCK_NTSC {
            self.sample_accumulator -= APU_CPU_CLOCK_NTSC;

            let p1_raw = self.pulse1.output() as f32;
            let p2_raw = self.pulse2.output() as f32;
            let tri_raw = self.triangle.output() as f32;
            let noi_raw = self.noise.output() as f32;
            let dmc_raw = self.dmc.output() as f32;

            let p1 = if self.channel_mutes[0] { 0.0 } else { p1_raw };
            let p2 = if self.channel_mutes[1] { 0.0 } else { p2_raw };
            let tri = if self.channel_mutes[2] { 0.0 } else { tri_raw };
            let noi = if self.channel_mutes[3] { 0.0 } else { noi_raw };
            let dmc = if self.channel_mutes[4] { 0.0 } else { dmc_raw };

            let pulse_sum = p1 + p2;
            let pulse_out = if pulse_sum > 0.0 {
                95.88 / (8128.0 / pulse_sum + 100.0)
            } else {
                0.0
            };

            let tnd_sum = tri / 8227.0 + noi / 12241.0 + dmc / 22638.0;
            let tnd_out = if tnd_sum > 0.0 {
                159.79 / (1.0 / tnd_sum + 100.0)
            } else {
                0.0
            };

            let sample = pulse_out + tnd_out + self.expansion_audio;
            self.sample_buffer.push(sample);

            if self.debug_collection_enabled {
                Self::push_debug_sample(&mut self.master_debug_samples, sample.clamp(-1.0, 1.0));
                Self::push_debug_sample(
                    &mut self.pulse1_debug_samples,
                    (p1_raw / 15.0).clamp(-1.0, 1.0),
                );
                Self::push_debug_sample(
                    &mut self.pulse2_debug_samples,
                    (p2_raw / 15.0).clamp(-1.0, 1.0),
                );
                Self::push_debug_sample(
                    &mut self.triangle_debug_samples,
                    ((tri_raw - 7.5) / 7.5).clamp(-1.0, 1.0),
                );
                Self::push_debug_sample(
                    &mut self.noise_debug_samples,
                    (noi_raw / 15.0).clamp(-1.0, 1.0),
                );
            }
        }
    }

    fn push_debug_sample(samples: &mut VecDeque<f32>, sample: f32) {
        if samples.len() >= DEBUG_SAMPLE_CAPACITY {
            let _ = samples.pop_front();
        }
        samples.push_back(sample);
    }

    pub fn drain_samples(&mut self) -> Vec<f32> {
        std::mem::replace(
            &mut self.sample_buffer,
            Vec::with_capacity(INITIAL_SAMPLE_CAPACITY),
        )
    }

    pub fn drain_samples_into_stereo(&mut self, buf: &mut Vec<f32>) {
        buf.clear();
        buf.reserve(self.sample_buffer.len() * 2);
        for &sample in &self.sample_buffer {
            buf.push(sample);
            buf.push(sample);
        }
        self.sample_buffer.clear();
    }

    pub fn channel_snapshot(&self) -> ApuChannelSnapshot {
        ApuChannelSnapshot {
            pulse1_enabled: self.pulse1.midi_active(),
            pulse1_timer_period: self.pulse1.timer_period(),
            pulse1_volume: self.pulse1.midi_volume(),
            pulse2_enabled: self.pulse2.midi_active(),
            pulse2_timer_period: self.pulse2.timer_period(),
            pulse2_volume: self.pulse2.midi_volume(),
            triangle_enabled: self.triangle.midi_active(),
            triangle_timer_period: self.triangle.timer_period(),
            triangle_volume: self.triangle.midi_volume(),
            noise_enabled: self.noise.midi_active(),
            noise_volume: self.noise.midi_volume(),
        }
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        self.sample_generation_enabled = enabled;
    }

    pub fn set_debug_collection_enabled(&mut self, enabled: bool) {
        self.debug_collection_enabled = enabled;
    }

    pub fn debug_collection_enabled(&self) -> bool {
        self.debug_collection_enabled
    }

    pub fn set_channel_mutes(&mut self, mutes: [bool; 5]) {
        self.channel_mutes = mutes;
    }

    pub fn channel_mutes(&self) -> [bool; 5] {
        self.channel_mutes
    }

    pub fn master_debug_samples_ordered(&self) -> Vec<f32> {
        self.master_debug_samples.iter().copied().collect()
    }

    pub fn channel_debug_samples_ordered(&self, channel: usize) -> Vec<f32> {
        match channel {
            0 => self.pulse1_debug_samples.iter().copied().collect(),
            1 => self.pulse2_debug_samples.iter().copied().collect(),
            2 => self.triangle_debug_samples.iter().copied().collect(),
            3 => self.noise_debug_samples.iter().copied().collect(),
            _ => Vec::new(),
        }
    }
}
