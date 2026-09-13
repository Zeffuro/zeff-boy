use blip_buf::BlipBuf;

const CLOCK_RATE: f64 = 4_194_304.0;
const BLOCK_CLOCKS: u32 = 4096;
const LEVEL_SCALE: f32 = 16_384.0;

#[derive(Clone, Copy)]
struct OutputEvent {
    clock: u64,
    delta: [i32; 2],
}

pub(super) struct StereoOutput {
    left: BlipBuf,
    right: BlipBuf,
    rate: u32,
    block_clocks: u32,
    clocks: u32,
    gains: [[f32; 2]; 4],
    samples: [f32; 4],
    levels: [[i32; 2]; 4],
    events: Vec<OutputEvent>,
}

impl StereoOutput {
    pub(super) fn new(rate: u32) -> Self {
        let block_clocks = (4_194_304_u64 * 3000 / u64::from(rate.max(1)))
            .clamp(1, u64::from(BLOCK_CLOCKS)) as u32;
        let capacity = (u64::from(block_clocks) * u64::from(rate) / 4_194_304 + 32) as u32;
        let mut left = BlipBuf::new(capacity);
        let mut right = BlipBuf::new(capacity);
        left.set_rates(CLOCK_RATE, f64::from(rate)).unwrap();
        right.set_rates(CLOCK_RATE, f64::from(rate)).unwrap();
        left.clear();
        right.clear();
        Self {
            left,
            right,
            rate,
            block_clocks,
            clocks: 0,
            gains: [[0.0; 2]; 4],
            samples: [f32::NAN; 4],
            levels: [[0; 2]; 4],
            events: Vec::with_capacity(16),
        }
    }

    pub(super) fn reset(&mut self, rate: u32) {
        if rate != self.rate {
            *self = Self::new(rate);
            return;
        }
        self.left.clear();
        self.right.clear();
        self.clocks = 0;
        self.levels = [[0; 2]; 4];
        self.samples = [f32::NAN; 4];
        self.events.clear();
    }

    pub(super) fn set_mixer(&mut self, nr50: u8, nr51: u8, mutes: [bool; 4]) {
        self.samples = [f32::NAN; 4];
        let master = [f32::from((nr50 >> 4) & 7), f32::from(nr50 & 7)];
        for (channel, gains) in self.gains.iter_mut().enumerate() {
            for side in 0..2 {
                let routed = nr51 & (1 << (channel + if side == 0 { 4 } else { 0 })) != 0;
                gains[side] = if routed && !mutes[channel] {
                    master[side] / 7.0 * (LEVEL_SCALE / 4.0)
                } else {
                    0.0
                };
            }
        }
    }

    pub(super) fn record_channel(&mut self, channel: usize, clock: u64, sample: f32) {
        if self.samples[channel] == sample {
            return;
        }
        self.samples[channel] = sample;
        let level = self.gains[channel].map(|gain| (sample * gain).round() as i32);
        let previous = &mut self.levels[channel];
        let delta = [level[0] - previous[0], level[1] - previous[1]];
        *previous = level;
        if delta != [0; 2] {
            self.events.push(OutputEvent { clock, delta });
        }
    }

    #[inline]
    pub(super) fn finish_step(&mut self, clocks: u64, output: &mut Vec<f32>) {
        if self.events.is_empty() && clocks < u64::from(self.block_clocks - self.clocks) {
            self.clocks += clocks as u32;
        } else {
            self.finish_step_full(clocks, output);
        }
    }

    fn finish_step_full(&mut self, clocks: u64, output: &mut Vec<f32>) {
        // Hardware clocks advance each channel separately; mixer deltas commute at equal times.
        self.events.sort_unstable_by_key(|event| event.clock);
        let mut previous = 0;
        for index in 0..self.events.len() {
            let event = self.events[index];
            self.advance(event.clock - previous, output);
            if event.delta[0] != 0 {
                self.left.add_delta(self.clocks, event.delta[0]).unwrap();
            }
            if event.delta[1] != 0 {
                self.right.add_delta(self.clocks, event.delta[1]).unwrap();
            }
            previous = event.clock;
        }
        self.advance(clocks - previous, output);
        self.events.clear();
    }

    fn advance(&mut self, mut clocks: u64, output: &mut Vec<f32>) {
        while clocks != 0 {
            let step = clocks.min(u64::from(self.block_clocks - self.clocks)) as u32;
            self.clocks += step;
            clocks -= u64::from(step);
            if self.clocks == self.block_clocks {
                self.flush(output);
            }
        }
    }

    pub(super) fn flush(&mut self, output: &mut Vec<f32>) {
        if self.clocks == 0 {
            return;
        }
        self.left.end_frame(self.clocks).unwrap();
        self.right.end_frame(self.clocks).unwrap();
        self.clocks = 0;
        let mut left = [0; 256];
        let mut right = [0; 256];
        while self.left.samples_avail() != 0 {
            let count = self.left.read_samples(&mut left, false);
            let right_count = self.right.read_samples(&mut right, false);
            debug_assert_eq!(count, right_count);
            output.reserve(count * 2);
            for (&left, &right) in left[..count].iter().zip(&right[..count]) {
                output.push((f32::from(left) / LEVEL_SCALE).clamp(-1.0, 1.0));
                output.push((f32::from(right) / LEVEL_SCALE).clamp(-1.0, 1.0));
            }
        }
    }
}
