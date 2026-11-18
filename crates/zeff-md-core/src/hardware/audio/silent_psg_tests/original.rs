use super::*;

impl Audio {
    pub(super) fn advance_original(&mut self, at: u64) {
        let elapsed = at.checked_sub(self.now).expect("ordered audio publication");
        if let Some(admitted) = &mut self.deferred {
            assert!(at >= *admitted, "ordered immediate audio fence");
            *admitted = at;
        }
        let total =
            u128::from(self.sample_phase) + u128::from(elapsed) * u128::from(self.sample_rate);
        let count = (total / u128::from(self.master_hz)) as u64;
        if !self.generation_enabled {
            self.sample_phase = (total % u128::from(self.master_hz)) as u64;
            self.advance_chip(at);
            self.restart_filter();
            return;
        }
        let retained = (SAMPLE_CAPACITY + output::HISTORY) as u64;
        if count > retained {
            let delta = (u128::from(count - retained) * u128::from(self.master_hz)
                - u128::from(self.sample_phase))
            .div_ceil(u128::from(self.sample_rate)) as u64;
            self.sample_phase = ((u128::from(self.sample_phase)
                + u128::from(delta) * u128::from(self.sample_rate))
                % u128::from(self.master_hz)) as u64;
            self.advance_chip(self.now + delta);
            self.restart_filter();
            self.head = 0;
            self.len = 0;
        }
        while self.now < at {
            let psg = PSG_CLOCK_TICKS - self.chip_phase;
            let fm = self.opn2.clocks_to_sample() * OPN2_CLOCK_TICKS - self.opn2_phase;
            let sample = (self.master_hz - self.sample_phase).div_ceil(u64::from(self.sample_rate));
            let step = (at - self.now).min(psg).min(fm).min(sample);
            self.advance_chip(self.now + step);
            self.sample_phase += step * u64::from(self.sample_rate);
            if step == psg || step == fm {
                self.refresh_output();
            }
            if self.sample_phase >= self.master_hz {
                let sample = self.output.sample();
                self.push(sample);
                self.sample_phase -= self.master_hz;
            }
        }
    }
}
