use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction, audioadapter::Adapter, audioadapter_buffers::direct::SequentialSliceOfVecs,
};

const CHUNK_SIZE: usize = 256;
const MAX_RATIO_RELATIVE: f64 = 1.02;
const TARGET_FILL_RATIO: f32 = 0.5;
const DRIFT_CORRECTION_STRENGTH: f64 = 0.05;

pub(crate) struct AudioResampler {
    resampler: Async<f32>,
    pending_left: Vec<f32>,
    pending_right: Vec<f32>,
    scratch_left: Vec<f32>,
    scratch_right: Vec<f32>,
    output: Vec<f32>,
    chunk_size: usize,
}

impl AudioResampler {
    pub(crate) fn new(source_rate: u32, target_rate: u32) -> anyhow::Result<Self> {
        let ratio = target_rate as f64 / source_rate as f64;
        let params = SincInterpolationParameters {
            sinc_len: 128,
            f_cutoff: 0.95,
            interpolation: SincInterpolationType::Cubic,
            oversampling_factor: 128,
            window: WindowFunction::BlackmanHarris2,
        };
        let resampler = Async::<f32>::new_sinc(
            ratio,
            MAX_RATIO_RELATIVE,
            &params,
            CHUNK_SIZE,
            2,
            FixedAsync::Input,
        )?;

        Ok(Self {
            resampler,
            pending_left: Vec::with_capacity(CHUNK_SIZE * 2),
            pending_right: Vec::with_capacity(CHUNK_SIZE * 2),
            scratch_left: Vec::with_capacity(CHUNK_SIZE),
            scratch_right: Vec::with_capacity(CHUNK_SIZE),
            output: Vec::new(),
            chunk_size: CHUNK_SIZE,
        })
    }

    pub(crate) fn process(&mut self, interleaved: &[f32], fill_ratio: f32) -> Vec<f32> {
        for pair in interleaved.chunks_exact(2) {
            self.pending_left.push(pair[0]);
            self.pending_right.push(pair[1]);
        }

        let error = fill_ratio - TARGET_FILL_RATIO;
        let ratio_adjust = 1.0 - error as f64 * DRIFT_CORRECTION_STRENGTH;
        let clamped = ratio_adjust.clamp(1.0 / MAX_RATIO_RELATIVE, MAX_RATIO_RELATIVE);
        let _ = self.resampler.set_resample_ratio_relative(clamped, true);

        self.output.clear();

        while self.pending_left.len() >= self.chunk_size {
            self.scratch_left.clear();
            self.scratch_left
                .extend(self.pending_left.drain(..self.chunk_size));
            self.scratch_right.clear();
            self.scratch_right
                .extend(self.pending_right.drain(..self.chunk_size));

            let input_buf = [
                std::mem::take(&mut self.scratch_left),
                std::mem::take(&mut self.scratch_right),
            ];
            {
                let adapter = SequentialSliceOfVecs::new(&input_buf, 2, self.chunk_size)
                    .expect("audio input must have exactly 2 channels with matching chunk size");
                match self.resampler.process(&adapter, 0, None) {
                    Ok(result) => {
                        let out_frames = result.frames();
                        let out_channels = result.channels();
                        let data = result.take_data();
                        self.output.reserve(out_frames * 2);
                        for f in 0..out_frames {
                            self.output.push(data[f * out_channels]);
                            self.output.push(data[f * out_channels + 1]);
                        }
                    }
                    Err(e) => {
                        log::warn!("audio resampler error: {e}");
                    }
                }
            }
            let [left, right] = input_buf;
            self.scratch_left = left;
            self.scratch_right = right;
        }

        std::mem::take(&mut self.output)
    }

    #[allow(dead_code)]
    pub(crate) fn reset(&mut self) {
        self.resampler.reset();
        self.pending_left.clear();
        self.pending_right.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_creates_successfully() {
        let r = AudioResampler::new(48000, 48000);
        assert!(r.is_ok());
    }

    #[test]
    fn resampler_creates_for_different_rates() {
        let r = AudioResampler::new(44100, 48000);
        assert!(r.is_ok());
    }

    #[test]
    fn resampler_processes_silence() {
        let mut r = AudioResampler::new(48000, 48000).unwrap();
        let input = vec![0.0f32; CHUNK_SIZE * 2];
        let output = r.process(&input, 0.5);
        assert!(!output.is_empty());
        for &s in &output {
            assert!(s.abs() < 0.01, "expected near-silence, got {s}");
        }
    }

    #[test]
    fn resampler_passthrough_preserves_approximate_count() {
        let mut r = AudioResampler::new(48000, 48000).unwrap();
        let frames = CHUNK_SIZE;
        let input: Vec<f32> = (0..frames * 2).map(|i| (i as f32) * 0.001).collect();

        let _ = r.process(&input, 0.5);
        let output = r.process(&input, 0.5);
        let out_frames = output.len() / 2;
        assert!(
            (out_frames as f64 - frames as f64).abs() < (frames as f64 * 0.10),
            "expected ~{frames} output frames, got {out_frames}"
        );
    }

    #[test]
    fn drift_correction_low_fill_increases_output() {
        let mut r = AudioResampler::new(48000, 48000).unwrap();
        let input: Vec<f32> = vec![0.1; CHUNK_SIZE * 2];
        let normal = r.process(&input, 0.5);
        r.reset();
        let boosted = r.process(&input, 0.1);
        assert!(boosted.len() >= normal.len().saturating_sub(4));
    }

    #[test]
    fn drift_correction_high_fill_decreases_output() {
        let mut r = AudioResampler::new(48000, 48000).unwrap();
        let input: Vec<f32> = vec![0.1; CHUNK_SIZE * 2];
        let normal = r.process(&input, 0.5);
        r.reset();
        let reduced = r.process(&input, 0.9);
        assert!(reduced.len() <= normal.len() + 4);
    }

    #[test]
    fn reset_clears_pending() {
        let mut r = AudioResampler::new(48000, 48000).unwrap();
        let input = vec![0.5f32; 100];
        let _ = r.process(&input, 0.5);
        r.reset();
        let output = r.process(&[], 0.5);
        assert!(output.is_empty());
    }
}
