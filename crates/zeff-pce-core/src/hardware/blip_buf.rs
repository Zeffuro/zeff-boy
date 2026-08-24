// Derived from blip_buf 0.2.1.
//
// The MIT License (MIT)
//
// Copyright (c) 2015 Mathijs van de Nes
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use anyhow::bail;
use zeff_emu_common::save_state::{StateReader, StateWriter};

const MAX_RATIO: u64 = 1 << 20;
const PRE_SHIFT: usize = 32;
const TIME_BITS: usize = PRE_SHIFT + 20;
const TIME_UNIT: u64 = 1 << TIME_BITS;
const BASS_SHIFT: usize = 9;
const END_FRAME_EXTRA: usize = 2;
const HALF_WIDTH: usize = 8;
const BUF_EXTRA: usize = HALF_WIDTH * 2 + END_FRAME_EXTRA;
const PHASE_BITS: usize = 5;
const PHASE_COUNT: usize = 1 << PHASE_BITS;
const DELTA_BITS: usize = 15;
const DELTA_UNIT: usize = 1 << DELTA_BITS;
const FRAC_BITS: usize = TIME_BITS - PRE_SHIFT;

pub(super) struct BlipBuf {
    factor: u64,
    offset: u64,
    integrator: i32,
    avail: usize,
    samples: Vec<i32>,
}

impl BlipBuf {
    pub(super) fn new(sample_count: u32) -> Self {
        let sample_count = sample_count as usize;
        const FACTOR: u64 = TIME_UNIT / MAX_RATIO;
        Self {
            factor: FACTOR,
            offset: FACTOR / 2,
            integrator: 0,
            avail: 0,
            samples: vec![0; sample_count + BUF_EXTRA],
        }
    }

    pub(super) fn set_rates(
        &mut self,
        clock_rate: f64,
        sample_rate: f64,
    ) -> Result<(), &'static str> {
        let factor = TIME_UNIT as f64 * sample_rate / clock_rate;
        let factor_int = factor as u64;
        let in_bounds = 0.0 <= factor - factor_int as f64 && factor - (factor_int as f64) < 1.0;
        if !in_bounds {
            return Err("clock_rate exceeds maximum, relative to sample_rate");
        }

        self.factor = factor.ceil() as u64;
        Ok(())
    }

    pub(super) fn add_delta(&mut self, clock_time: u32, delta: i32) -> Result<(), &'static str> {
        let fixed = ((u64::from(clock_time) * self.factor + self.offset) >> PRE_SHIFT) as usize;
        let out_index = self.avail + (fixed >> FRAC_BITS);
        if out_index + 16 > self.samples.len() {
            return Err("buffer size was exceeded");
        }

        const PHASE_SHIFT: usize = FRAC_BITS - PHASE_BITS;
        let phase = fixed >> PHASE_SHIFT & (PHASE_COUNT - 1);
        let phase_rev = PHASE_COUNT - phase;
        let interp = (fixed >> (PHASE_SHIFT - DELTA_BITS) & (DELTA_UNIT - 1)) as i32;
        let delta2 = (delta * interp) >> DELTA_BITS;
        let delta1 = delta - delta2;

        let steps = BL_STEP[phase].iter().zip(BL_STEP[phase + 1].iter());
        for (sample, (&step1, &step2)) in
            self.samples[out_index..out_index + 8].iter_mut().zip(steps)
        {
            let contribution = step1 * delta1 + step2 * delta2;
            *sample = sample.wrapping_add(contribution);
        }
        let steps = BL_STEP[phase_rev]
            .iter()
            .rev()
            .zip(BL_STEP[phase_rev - 1].iter().rev());
        for (sample, (&step1, &step2)) in self.samples[out_index + 8..out_index + 16]
            .iter_mut()
            .zip(steps)
        {
            let contribution = step1 * delta1 + step2 * delta2;
            *sample = sample.wrapping_add(contribution);
        }
        Ok(())
    }

    pub(super) fn end_frame(&mut self, clock_duration: u32) -> Result<(), &'static str> {
        let offset = u64::from(clock_duration) * self.factor + self.offset;
        let avail = self.avail + (offset >> TIME_BITS) as usize;
        if avail > self.samples.len() {
            return Err("buffer size was exceeded");
        }

        self.avail = avail;
        self.offset = offset & (TIME_UNIT - 1);
        Ok(())
    }

    pub(super) fn samples_avail(&self) -> u32 {
        self.avail as u32
    }

    pub(super) fn read_samples(&mut self, output: &mut [i16], stereo: bool) -> usize {
        let step = if stereo { 2 } else { 1 };
        let count = (output.len() / step).min(self.avail);
        if count == 0 {
            return 0;
        }

        let mut sum = self.integrator;
        for (index, sample) in self.samples.iter().take(count).enumerate() {
            let output_sample = (sum >> DELTA_BITS).clamp(i16::MIN.into(), i16::MAX.into());
            output[index * step] = output_sample as i16;
            sum = sum.wrapping_add(*sample);
            sum = sum.wrapping_sub(output_sample << (DELTA_BITS - BASS_SHIFT));
        }
        self.integrator = sum;
        self.remove_samples(count);
        count
    }

    fn remove_samples(&mut self, count: usize) {
        let remain = (self.avail + BUF_EXTRA).saturating_sub(count);
        self.avail = self.avail.saturating_sub(count);
        self.samples.copy_within(count..count + remain, 0);
        self.samples[remain..].fill(0);
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u64(self.factor);
        writer.write_u64(self.offset);
        writer.write_u32(self.integrator as u32);
        writer.write_u32(self.avail as u32);
        writer.write_u32(self.samples.len() as u32);
        for sample in &self.samples {
            writer.write_u32(*sample as u32);
        }
    }

    pub(super) fn read_state(&mut self, reader: &mut StateReader<'_>) -> anyhow::Result<()> {
        let factor = reader.read_u64()?;
        if factor != self.factor {
            bail!("resampler factor does not match the destination sample rate");
        }
        let offset = reader.read_u64()?;
        if offset >= TIME_UNIT {
            bail!("invalid resampler time offset in save-state");
        }
        let integrator = reader.read_u32()? as i32;
        let avail = reader.read_u32()? as usize;
        if avail > self.samples.len() - BUF_EXTRA {
            bail!("resampler available-sample count exceeds its fixed buffer");
        }
        let sample_count = reader.read_u32()? as usize;
        if sample_count != self.samples.len() {
            bail!(
                "resampler buffer length mismatch: state has {sample_count}, expected {}",
                self.samples.len()
            );
        }
        let mut samples = Vec::with_capacity(sample_count);
        for _ in 0..sample_count {
            samples.push(reader.read_u32()? as i32);
        }

        self.offset = offset;
        self.integrator = integrator;
        self.avail = avail;
        self.samples = samples;
        Ok(())
    }

    pub(super) fn timing_matches(&self, other: &Self) -> bool {
        self.factor == other.factor && self.offset == other.offset && self.avail == other.avail
    }

    #[cfg(test)]
    pub(super) fn state_sample_count(&self) -> usize {
        self.samples.len()
    }
}

const BL_STEP: &[[i32; 8]] = &[
    [43, -115, 350, -488, 1136, -914, 5861, 21022],
    [44, -118, 348, -473, 1076, -799, 5274, 21001],
    [45, -121, 344, -454, 1011, -677, 4706, 20936],
    [46, -122, 336, -431, 942, -549, 4156, 20829],
    [47, -123, 327, -404, 868, -418, 3629, 20679],
    [47, -122, 316, -375, 792, -285, 3124, 20488],
    [47, -120, 303, -344, 714, -151, 2644, 20256],
    [46, -117, 289, -310, 634, -17, 2188, 19985],
    [46, -114, 273, -275, 553, 117, 1758, 19675],
    [44, -108, 255, -237, 471, 247, 1356, 19327],
    [43, -103, 237, -199, 390, 373, 981, 18944],
    [42, -98, 218, -160, 310, 495, 633, 18527],
    [40, -91, 198, -121, 231, 611, 314, 18078],
    [38, -84, 178, -81, 153, 722, 22, 17599],
    [36, -76, 157, -43, 80, 824, -241, 17092],
    [34, -68, 135, -3, 8, 919, -476, 16558],
    [32, -61, 115, 34, -60, 1006, -683, 16001],
    [29, -52, 94, 70, -123, 1083, -862, 15422],
    [27, -44, 73, 106, -184, 1152, -1015, 14824],
    [25, -36, 53, 139, -239, 1211, -1142, 14210],
    [22, -27, 34, 170, -290, 1261, -1244, 13582],
    [20, -20, 16, 199, -335, 1301, -1322, 12942],
    [18, -12, -3, 226, -375, 1331, -1376, 12293],
    [15, -4, -19, 250, -410, 1351, -1408, 11638],
    [13, 3, -35, 272, -439, 1361, -1419, 10979],
    [11, 9, -49, 292, -464, 1362, -1410, 10319],
    [9, 16, -63, 309, -483, 1354, -1383, 9660],
    [7, 22, -75, 322, -496, 1337, -1339, 9005],
    [6, 26, -85, 333, -504, 1312, -1280, 8355],
    [4, 31, -94, 341, -507, 1278, -1205, 7713],
    [3, 35, -102, 347, -506, 1238, -1119, 7082],
    [1, 40, -110, 350, -499, 1190, -1021, 6464],
    [0, 43, -115, 350, -488, 1136, -914, 5861],
];

#[cfg(test)]
mod tests {
    use super::BlipBuf;

    #[test]
    fn local_port_matches_upstream_across_rates_and_frame_boundaries() {
        const CLOCK_RATE: f64 = 630_000_000.0 / 88.0;
        const FRAME_CLOCKS: u32 = 65_536;
        const EVENTS: &[(u32, i32)] = &[
            (0, 7_000),
            (1, -2_000),
            (37, 9_000),
            (1_024, -16_000),
            (32_768, 8_000),
            (65_535, -6_000),
        ];

        for sample_rate in [1, 44_100, 48_000, 192_000] {
            let mut local = BlipBuf::new(2_048);
            let mut upstream = blip_buf::BlipBuf::new(2_048);
            local.set_rates(CLOCK_RATE, f64::from(sample_rate)).unwrap();
            upstream
                .set_rates(CLOCK_RATE, f64::from(sample_rate))
                .unwrap();

            for frame in 0..6 {
                for &(clock, delta) in EVENTS {
                    let delta = if frame & 1 == 0 { delta } else { -delta };
                    local.add_delta(clock, delta).unwrap();
                    upstream.add_delta(clock, delta).unwrap();
                }
                local.end_frame(FRAME_CLOCKS).unwrap();
                upstream.end_frame(FRAME_CLOCKS).unwrap();
                assert_eq!(local.samples_avail(), upstream.samples_avail());

                let stereo = frame & 1 != 0;
                let step = if stereo { 2 } else { 1 };
                let len = local.samples_avail() as usize * step;
                let mut local_output = vec![0; len];
                let mut upstream_output = vec![0; len];
                assert_eq!(
                    local.read_samples(&mut local_output, stereo),
                    upstream.read_samples(&mut upstream_output, stereo)
                );
                assert_eq!(local_output, upstream_output);
            }
        }
    }
}
