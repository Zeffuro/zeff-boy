pub(super) struct NesOutputFilter {
    high_pass_90hz: OnePoleHighPass,
    high_pass_440hz: OnePoleHighPass,
    low_pass_14khz: OnePoleLowPass,
    sample_rate: f64,
}

impl NesOutputFilter {
    pub(super) fn new(sample_rate: f64) -> Self {
        let mut filter = Self {
            high_pass_90hz: OnePoleHighPass::new(90.0),
            high_pass_440hz: OnePoleHighPass::new(440.0),
            low_pass_14khz: OnePoleLowPass::new(14_000.0),
            sample_rate: 0.0,
        };
        filter.configure(sample_rate);
        filter
    }

    pub(super) fn configure(&mut self, sample_rate: f64) {
        let sample_rate = sample_rate.max(1.0);
        self.sample_rate = sample_rate;
        self.high_pass_90hz.configure(sample_rate);
        self.high_pass_440hz.configure(sample_rate);
        self.low_pass_14khz.configure(sample_rate);
    }

    pub(super) fn process(&mut self, sample: f32, sample_rate: f64) -> f32 {
        if (self.sample_rate - sample_rate.max(1.0)).abs() > f64::EPSILON {
            self.configure(sample_rate);
        }

        let sample = self.high_pass_90hz.process(sample);
        let sample = self.high_pass_440hz.process(sample);
        self.low_pass_14khz.process(sample)
    }

    pub(super) fn reset(&mut self) {
        self.high_pass_90hz.reset();
        self.high_pass_440hz.reset();
        self.low_pass_14khz.reset();
    }
}

struct OnePoleHighPass {
    cutoff_hz: f64,
    alpha: f32,
    prev_input: f32,
    prev_output: f32,
    primed: bool,
}

impl OnePoleHighPass {
    fn new(cutoff_hz: f64) -> Self {
        Self {
            cutoff_hz,
            alpha: 0.0,
            prev_input: 0.0,
            prev_output: 0.0,
            primed: false,
        }
    }

    fn configure(&mut self, sample_rate: f64) {
        let rc = 1.0 / (std::f64::consts::TAU * self.cutoff_hz);
        let dt = 1.0 / sample_rate.max(1.0);
        self.alpha = (rc / (rc + dt)) as f32;
    }

    fn process(&mut self, input: f32) -> f32 {
        if !self.primed {
            self.prev_input = input;
            self.prev_output = 0.0;
            self.primed = true;
            return 0.0;
        }

        let output = self.alpha * (self.prev_output + input - self.prev_input);
        self.prev_input = input;
        self.prev_output = output;
        output
    }

    fn reset(&mut self) {
        self.prev_input = 0.0;
        self.prev_output = 0.0;
        self.primed = false;
    }
}

struct OnePoleLowPass {
    cutoff_hz: f64,
    alpha: f32,
    output: f32,
    primed: bool,
}

impl OnePoleLowPass {
    fn new(cutoff_hz: f64) -> Self {
        Self {
            cutoff_hz,
            alpha: 0.0,
            output: 0.0,
            primed: false,
        }
    }

    fn configure(&mut self, sample_rate: f64) {
        let rc = 1.0 / (std::f64::consts::TAU * self.cutoff_hz);
        let dt = 1.0 / sample_rate.max(1.0);
        self.alpha = (dt / (rc + dt)) as f32;
    }

    fn process(&mut self, input: f32) -> f32 {
        if !self.primed {
            self.output = input;
            self.primed = true;
            return input;
        }

        self.output += self.alpha * (input - self.output);
        self.output
    }

    fn reset(&mut self) {
        self.output = 0.0;
        self.primed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::NesOutputFilter;

    #[test]
    fn removes_dc_bias() {
        let mut filter = NesOutputFilter::new(48_000.0);
        let mut output = 0.0;

        for _ in 0..48_000 {
            output = filter.process(0.5, 48_000.0);
        }

        assert!(
            output.abs() < 0.0001,
            "constant DC input should decay near zero, got {output}"
        );
    }

    #[test]
    fn passes_changes_without_clipping() {
        let mut filter = NesOutputFilter::new(48_000.0);
        let mut peak = 0.0f32;

        for i in 0..4096 {
            let input = if i & 0x20 == 0 { 0.2 } else { 0.5 };
            peak = peak.max(filter.process(input, 48_000.0).abs());
        }

        assert!(peak > 0.01, "changing input should survive filtering");
        assert!(
            peak < 1.0,
            "filter should not amplify normal NES mix into clipping"
        );
    }
}
