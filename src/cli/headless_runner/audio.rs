#[derive(Clone, Copy, Debug, Default)]
pub(super) struct AudioStats {
    pub(super) frames_with_samples: u64,
    pub(super) sample_count: u64,
    pub(super) nonzero_samples: u64,
    pub(super) peak_abs: f32,
    pub(super) sum_abs: f64,
}

impl AudioStats {
    pub(super) fn observe(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        self.frames_with_samples = self.frames_with_samples.wrapping_add(1);
        self.sample_count = self.sample_count.wrapping_add(samples.len() as u64);
        for &sample in samples {
            let abs = sample.abs();
            if abs > 0.000_001 {
                self.nonzero_samples = self.nonzero_samples.wrapping_add(1);
            }
            self.peak_abs = self.peak_abs.max(abs);
            self.sum_abs += f64::from(abs);
        }
    }

    pub(super) fn mean_abs(self) -> f64 {
        if self.sample_count == 0 {
            0.0
        } else {
            self.sum_abs / self.sample_count as f64
        }
    }
}
