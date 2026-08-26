use super::*;

impl Psg {
    pub(super) fn generate_samples(&mut self, t_cycles: u64) {
        let cycles_per_sample = PSG_CLOCK_HZ as f64 / self.sample_rate as f64;
        self.sample_cycle_accum += t_cycles as f64;
        while self.sample_cycle_accum >= cycles_per_sample {
            self.sample_cycle_accum -= cycles_per_sample;
            let (left, right) = self.mix_sample();
            self.sample_buffer.push(left);
            self.sample_buffer.push(right);
        }
    }

    pub(super) fn mix_sample(&self) -> (f32, f32) {
        if !self.powered() {
            return (0.0, 0.0);
        }

        let nr50 = self.regs[(NR50 - NR10) as usize];
        let nr51 = self.regs[(NR51 - NR10) as usize];
        let left_master = ((nr50 >> 4) & 0x07) as f32 / 7.0;
        let right_master = (nr50 & 0x07) as f32 / 7.0;

        let mut channel_samples = [
            self.ch1_sample(),
            self.ch2_sample(),
            self.ch3_sample(),
            self.ch4_sample(),
        ];
        for (sample, muted) in channel_samples.iter_mut().zip(self.channel_muted.iter()) {
            if *muted {
                *sample = 0.0;
            }
        }

        let mut left = 0.0f32;
        let mut right = 0.0f32;
        for (i, sample) in channel_samples.iter().enumerate() {
            if (nr51 & (1 << i)) != 0 {
                right += *sample;
            }
            if (nr51 & (1 << (i + 4))) != 0 {
                left += *sample;
            }
        }

        left = (left / 4.0) * left_master;
        right = (right / 4.0) * right_master;
        (left.clamp(-1.0, 1.0), right.clamp(-1.0, 1.0))
    }

    fn channel_raw_samples(&self) -> [f32; 4] {
        [
            self.ch1_sample(),
            self.ch2_sample(),
            self.ch3_sample(),
            self.ch4_sample(),
        ]
    }

    pub(super) fn capture_debug_samples(&mut self, t_cycles: u64) {
        self.debug_capture_cycle_accum = self.debug_capture_cycle_accum.saturating_add(t_cycles);
        while self.debug_capture_cycle_accum >= DEBUG_CAPTURE_DECIMATION_T_CYCLES {
            self.debug_capture_cycle_accum -= DEBUG_CAPTURE_DECIMATION_T_CYCLES;
            let channels = self.channel_raw_samples();
            for (history, sample) in self.channel_debug_history.iter_mut().zip(channels.iter()) {
                history.push(*sample);
            }
            let (left, right) = self.mix_sample();
            self.master_debug_history.push((left + right) * 0.5);
        }
    }

    fn ch1_sample(&self) -> f32 {
        self.square_sample(0, self.ch1_duty_pos)
    }

    fn ch2_sample(&self) -> f32 {
        self.square_sample(1, self.ch2_duty_pos)
    }
}
