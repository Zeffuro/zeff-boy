use std::collections::VecDeque;

use super::{Apu, FIFO_CAPACITY, FifoDmaRequests, GBA_CPU_HZ};

const GBA_MASTER_OUTPUT_GAIN: f32 = 0.75;
const GBA_OUTPUT_LOW_PASS_CUTOFF_HZ: f32 = 8_000.0;

impl Apu {
    pub(crate) fn write_fifo_halfword(&mut self, fifo: usize, value: u16) {
        self.write_fifo_byte(fifo, value as u8);
        self.write_fifo_byte(fifo, (value >> 8) as u8);
    }

    pub(crate) fn reset_fifo(&mut self, fifo: usize) {
        let queue = self.fifo_mut(fifo);
        queue.clear();
        if fifo == 0 {
            self.current_a = 0;
        } else {
            self.current_b = 0;
        }
    }

    pub(crate) fn step_output(
        &mut self,
        cycles: u32,
        soundcnt_h: u16,
        soundcnt_x: u16,
        soundbias: u16,
    ) {
        self.step_psg(cycles);
        if !self.sample_generation_enabled {
            return;
        }

        let dac_samples = self.advance_dac_phase(cycles, soundbias);
        self.mix_dac_samples(dac_samples, soundcnt_h, soundcnt_x);

        let output_pairs = advance_phase(&mut self.output_phase, cycles, self.sample_rate);
        self.emit_host_samples(output_pairs, soundcnt_h, soundcnt_x);
    }

    pub(crate) fn on_timer_overflow(&mut self, timer: usize, soundcnt_h: u16) -> FifoDmaRequests {
        let mut requests = FifoDmaRequests::default();
        if direct_sound_uses_timer(soundcnt_h, 10, timer) && direct_sound_enabled(soundcnt_h, 8) {
            self.current_a = self.fifo_a.pop_front().unwrap_or(0);
            requests.a = self.fifo_a.len() <= 16;
        }
        if direct_sound_uses_timer(soundcnt_h, 14, timer) && direct_sound_enabled(soundcnt_h, 12) {
            self.current_b = self.fifo_b.pop_front().unwrap_or(0);
            requests.b = self.fifo_b.len() <= 16;
        }
        requests
    }

    #[cfg(test)]
    pub(crate) fn on_timer_overflows(
        &mut self,
        overflows: crate::hardware::timer::TimerOverflowCounts,
        soundcnt_h: u16,
    ) -> FifoDmaRequests {
        let mut requests = FifoDmaRequests::default();
        for (timer, count) in overflows.into_iter().enumerate() {
            for _ in 0..count {
                let request = self.on_timer_overflow(timer, soundcnt_h);
                requests.a |= request.a;
                requests.b |= request.b;
            }
        }
        requests
    }

    fn advance_dac_phase(&mut self, cycles: u32, soundbias: u16) -> usize {
        let dac_rate = GBA_CPU_HZ as u32 / soundbias_sample_interval(soundbias).max(1);
        advance_phase(&mut self.dac_phase, cycles, dac_rate)
    }

    fn mix_dac_samples(&mut self, samples: usize, soundcnt_h: u16, soundcnt_x: u16) {
        if samples == 0 {
            return;
        }

        for _ in 0..samples {
            let (direct_left, direct_right) = self.mix_direct_sound(soundcnt_h, soundcnt_x);
            let (psg_left, psg_right) = self.mix_psg(soundcnt_h);
            self.last_dac_left = gba_output_sample(direct_left + psg_left);
            self.last_dac_right = gba_output_sample(direct_right + psg_right);
            self.dac_accum_left += self.last_dac_left;
            self.dac_accum_right += self.last_dac_right;
            self.dac_accum_count = self.dac_accum_count.saturating_add(1);
        }
    }

    fn emit_host_samples(&mut self, pairs: usize, soundcnt_h: u16, soundcnt_x: u16) {
        if pairs == 0 {
            return;
        }

        self.output_pairs_generated = self.output_pairs_generated.wrapping_add(pairs as u64);
        self.direct_pairs_generated = self.direct_pairs_generated.wrapping_add(pairs as u64);
        self.psg_pairs_generated = self.psg_pairs_generated.wrapping_add(pairs as u64);

        self.sample_buffer.reserve(pairs * 2);
        for _ in 0..pairs {
            let (left, right) = if self.dac_accum_count == 0 {
                (self.last_dac_left, self.last_dac_right)
            } else {
                let count = self.dac_accum_count as f32;
                let sample = (self.dac_accum_left / count, self.dac_accum_right / count);
                self.dac_accum_left = 0.0;
                self.dac_accum_right = 0.0;
                self.dac_accum_count = 0;
                sample
            };
            let alpha = output_low_pass_alpha(self.sample_rate);
            self.output_filter_left += alpha * (left - self.output_filter_left);
            self.output_filter_right += alpha * (right - self.output_filter_right);
            if self.debug_capture_enabled {
                self.capture_debug_samples(soundcnt_h, soundcnt_x);
            }
            self.sample_buffer.push(self.output_filter_left);
            self.sample_buffer.push(self.output_filter_right);
        }
    }

    fn capture_debug_samples(&mut self, soundcnt_h: u16, soundcnt_x: u16) {
        let direct_a = self.direct_channel_debug_sample(0, soundcnt_h, soundcnt_x);
        let direct_b = self.direct_channel_debug_sample(1, soundcnt_h, soundcnt_x);
        self.direct_debug_history[0].push(direct_a);
        self.direct_debug_history[1].push(direct_b);
        self.master_debug_history
            .push((self.output_filter_left + self.output_filter_right) * 0.5);
    }

    fn direct_channel_debug_sample(&self, fifo: usize, soundcnt_h: u16, soundcnt_x: u16) -> f32 {
        if soundcnt_x & 0x0080 == 0 {
            return 0.0;
        }

        let (right_bit, left_bit, volume_bit, mute_index, current) = if fifo == 0 {
            (8, 9, 2, 4, self.current_a)
        } else {
            (12, 13, 3, 5, self.current_b)
        };
        if self.channel_mutes[mute_index] {
            return 0.0;
        }
        if soundcnt_h & ((1 << right_bit) | (1 << left_bit)) == 0 {
            return 0.0;
        }

        let volume = if soundcnt_h & (1 << volume_bit) != 0 {
            1.0
        } else {
            0.5
        };
        f32::from(current) / 128.0 * volume
    }

    fn write_fifo_byte(&mut self, fifo: usize, value: u8) {
        let queue = self.fifo_mut(fifo);
        if queue.len() >= FIFO_CAPACITY {
            queue.pop_front();
        }
        queue.push_back(value as i8);
    }

    fn fifo_mut(&mut self, fifo: usize) -> &mut VecDeque<i8> {
        if fifo == 0 {
            &mut self.fifo_a
        } else {
            &mut self.fifo_b
        }
    }

    fn mix_direct_sound(&self, soundcnt_h: u16, soundcnt_x: u16) -> (f32, f32) {
        if soundcnt_x & 0x0080 == 0 {
            return (0.0, 0.0);
        }
        let mut right = 0.0;
        let mut left = 0.0;
        let volume_a = if soundcnt_h & (1 << 2) != 0 { 1.0 } else { 0.5 };
        let volume_b = if soundcnt_h & (1 << 3) != 0 { 1.0 } else { 0.5 };
        let sample_a = f32::from(self.current_a) / 128.0 * volume_a;
        let sample_b = f32::from(self.current_b) / 128.0 * volume_b;

        if !self.channel_mutes[4] {
            if soundcnt_h & (1 << 8) != 0 {
                right += sample_a;
            }
            if soundcnt_h & (1 << 9) != 0 {
                left += sample_a;
            }
        }
        if !self.channel_mutes[5] {
            if soundcnt_h & (1 << 12) != 0 {
                right += sample_b;
            }
            if soundcnt_h & (1 << 13) != 0 {
                left += sample_b;
            }
        }

        (left, right)
    }

    fn mix_psg(&self, soundcnt_h: u16) -> (f32, f32) {
        let (left, right) = self.psg.mix_current_sample();
        let volume = match soundcnt_h & 0x3 {
            0 => 0.25,
            1 => 0.5,
            2 | 3 => 1.0,
            _ => 1.0,
        };
        (left * volume, right * volume)
    }
}

fn gba_output_sample(sample: f32) -> f32 {
    sample.clamp(-1.0, 1.0) * GBA_MASTER_OUTPUT_GAIN
}

fn advance_phase(phase: &mut f64, cycles: u32, rate: u32) -> usize {
    let mut samples = 0;
    *phase += f64::from(cycles) * f64::from(rate.max(1));
    while *phase >= GBA_CPU_HZ {
        *phase -= GBA_CPU_HZ;
        samples += 1;
    }
    samples
}

fn soundbias_sample_interval(soundbias: u16) -> u32 {
    let resolution = u32::from((soundbias >> 14) & 0x3);
    0x200 >> resolution
}

fn output_low_pass_alpha(sample_rate: u32) -> f32 {
    let cutoff = GBA_OUTPUT_LOW_PASS_CUTOFF_HZ.min(sample_rate.max(1) as f32 * 0.45);
    let rc = 1.0 / (std::f32::consts::TAU * cutoff);
    let dt = 1.0 / sample_rate.max(1) as f32;
    (dt / (rc + dt)).clamp(0.0, 1.0)
}

fn direct_sound_uses_timer(soundcnt_h: u16, timer_select_bit: u16, timer: usize) -> bool {
    let selected = if soundcnt_h & (1 << timer_select_bit) != 0 {
        1
    } else {
        0
    };
    selected == timer
}

fn direct_sound_enabled(soundcnt_h: u16, right_enable_bit: u16) -> bool {
    soundcnt_h & ((1 << right_enable_bit) | (1 << (right_enable_bit + 1))) != 0
}
