use super::chip_constants::{PAN_LEFT, PAN_RIGHT};
use super::constants::{OPERATORS, OUTPUT_PHASE_SHIFT, REGISTER_SLOTS};
#[cfg(test)]
use super::dac;
use super::operator::Operator;
#[cfg(test)]
use super::operator::Sample;
mod render;
mod routing;
mod state;
use render::Render;
use routing::Routing;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Channel {
    operators: [Operator; OPERATORS],
    frequency: (u16, u8),
    algorithm: u8,
    feedback: u8,
    stereo: u8,
    history: [i32; 2],
    previous_s2: i32,
    render: Render,
}

impl Default for Channel {
    fn default() -> Self {
        Self {
            operators: [Operator::default(); OPERATORS],
            frequency: (0, 0),
            algorithm: 0,
            feedback: 0,
            stereo: PAN_LEFT | PAN_RIGHT,
            history: [0; 2],
            previous_s2: 0,
            render: Render::default(),
        }
    }
}

impl Channel {
    pub(super) fn write_operator(&mut self, address: u8, value: u8) {
        let group = address >> 4;
        if (3..=9).contains(&group) && address & 3 != 3 {
            let slot = REGISTER_SLOTS[usize::from(address >> 2 & 3)];
            self.operators[slot].write(group, value);
        }
    }

    pub(super) fn set_frequency(&mut self, fnum: u16, block: u8) {
        self.frequency = (fnum & 0x7ff, block & 7);
    }

    pub(super) fn write_channel(&mut self, address: u8, value: u8) {
        match address & 0xfc {
            0xb0 => {
                self.algorithm = value & 7;
                self.feedback = value >> 3 & 7;
            }
            0xb4 => self.stereo = value & 0xf7,
            _ => {}
        }
    }

    #[cfg(test)]
    pub(super) fn key(&mut self, mask: u8) {
        self.key_with_special(mask, None);
    }

    #[cfg(test)]
    pub(super) fn key_with_special(&mut self, mask: u8, special: Option<[(u16, u8); 3]>) {
        let frequencies = self.frequencies(special);
        for (slot, operator) in self.operators.iter_mut().enumerate() {
            operator.key(mask & (1 << slot) != 0, frequencies[slot]);
        }
    }

    pub(super) fn active(&self) -> bool {
        self.operators.iter().any(Operator::active)
    }

    pub(super) fn pan(&self) -> u8 {
        self.stereo
    }
    pub(super) fn algorithm(&self) -> u8 {
        self.algorithm
    }

    pub(super) fn frequencies(&self, special: Option<[(u16, u8); 3]>) -> [(u16, u8); OPERATORS] {
        let mut frequencies = [self.frequency; OPERATORS];
        if let Some(special) = special {
            frequencies[..3].copy_from_slice(&special);
        }
        frequencies
    }

    #[cfg(test)]
    pub(super) fn clock(&mut self, special: Option<[(u16, u8); 3]>) -> i32 {
        self.clock_with_lfo(special, 0, None)
    }

    #[cfg(test)]
    pub(super) fn clock_with_lfo(
        &mut self,
        special: Option<[(u16, u8); 3]>,
        lfo_counter: u8,
        envelope_counter: Option<u16>,
    ) -> i32 {
        let frequencies = self.frequencies(special);
        if let Some(counter) = envelope_counter {
            for (operator, frequency) in self.operators.iter_mut().zip(frequencies) {
                operator.clock_envelope(frequency, counter);
            }
        }
        let am = super::lfo::amplitude(lfo_counter, self.stereo >> 4 & 3);
        let pm = std::array::from_fn::<_, OPERATORS, _>(|slot| {
            super::lfo::frequency(frequencies[slot].0, lfo_counter, self.stereo & 7)
        });
        let samples = self.operators.map(|operator| operator.current_sample(am));
        let result = self.render_samples(samples, [self.algorithm | self.feedback << 3; OPERATORS]);
        for ((operator, frequency), fnum) in self.operators.iter_mut().zip(frequencies).zip(pm) {
            operator.clock_with_lfo(frequency, 0, fnum, am);
        }
        result
    }

    pub(super) fn operator(&self, index: usize) -> &Operator {
        &self.operators[index]
    }
    pub(super) fn operator_mut(&mut self, index: usize) -> &mut Operator {
        &mut self.operators[index]
    }
    pub(super) fn pms(&self) -> u8 {
        self.stereo & 7
    }
    pub(super) fn ams(&self) -> u8 {
        self.stereo >> 4 & 3
    }
    pub(super) fn quiet_history(&self) -> bool {
        self.history == [0; 2] && self.previous_s2 == 0 && self.render.quiet()
    }

    #[cfg(test)]
    fn render_samples(&mut self, samples: [Sample; OPERATORS], routing: [u8; OPERATORS]) -> i32 {
        let depth = routing[0] >> 3;
        let feedback = if depth == 0 {
            0
        } else {
            (self.history[0] + self.history[1]) >> (10 - depth)
        };
        let previous_s1 = self.history[0];
        let s1 = Operator::render(samples[0], feedback);
        self.history = [s1, self.history[0]];
        let s3_input = match routing[2] & 7 {
            0 | 2 => self.previous_s2,
            1 => previous_s1 + self.previous_s2,
            5 => previous_s1,
            _ => 0,
        };
        let s3 = Operator::render(samples[2], s3_input >> OUTPUT_PHASE_SHIFT);
        let s2_input = match routing[1] & 7 {
            0 | 3..=6 => s1,
            _ => 0,
        };
        let s2 = Operator::render(samples[1], s2_input >> OUTPUT_PHASE_SHIFT);
        let s4_input = match routing[3] & 7 {
            0 | 1 | 4 => s3,
            2 => s1 + s3,
            3 => self.previous_s2 + s3,
            5 => s1,
            _ => 0,
        };
        let s4 = Operator::render(samples[3], s4_input >> OUTPUT_PHASE_SHIFT);
        self.previous_s2 = s2;
        dac::sum_carriers([s1, s3, s2, s4], self.algorithm)
    }
}

#[cfg(test)]
mod tests {
    use super::Channel;

    fn frozen(algorithm: u8, mask: u8, phases: [usize; 4], level: u8) -> Channel {
        let mut channel = Channel::default();
        channel.write_channel(0xb0, algorithm);
        for (slot, (operator, clocks)) in channel.operators.iter_mut().zip(phases).enumerate() {
            operator.write(3, 1);
            operator.write(4, level);
            operator.write(5, 31);
            operator.key(mask & (1 << slot) != 0, (0, 0));
            for _ in 0..clocks {
                operator.clock((1024, 3), 0);
            }
        }
        channel
    }

    #[test]
    fn raw_operator_bits_ten_through_one_reach_the_carrier_phase() {
        let mut positive = frozen(4, 3, [64, 0, 0, 0], 0);
        assert_eq!(positive.clock(None), -608);
        assert_eq!(positive.history[0], 8168);
        assert_eq!(positive.previous_s2, -577);
        let mut negative = frozen(4, 3, [192, 0, 0, 0], 0);
        assert_eq!(negative.clock(None), 608);
        assert_eq!(negative.history[0], -8168);
        assert_eq!(negative.previous_s2, 625);
    }

    #[test]
    fn multiple_modulators_are_summed_before_discarding_the_low_bit() {
        for (clocks, expected) in [(64, 64), (192, -32)] {
            let mut channel = frozen(2, 13, [clocks, 0, clocks, 0], 96);
            channel.operators[3].write(4, 0);
            assert_eq!(channel.clock(None), expected);
        }
    }

    #[test]
    fn feedback_shifts_histories_directly_into_phase_units() {
        for (feedback, expected) in [0, 800, 1600, 3136, 5792, 8160, -32, 0]
            .into_iter()
            .enumerate()
        {
            let mut channel = frozen(7 | (feedback as u8) << 3, 1, [0; 4], 0);
            channel.history = [4096; 2];
            assert_eq!(channel.clock(None), expected);
        }
        let mut negative = frozen(0x3f, 1, [0; 4], 0);
        negative.history = [-1024; 2];
        assert_eq!(negative.clock(None), -8192);
        for (sign, expected) in [
            (1, [0, 96, 224, 416, 800, 1600, 3136, 5792]),
            (-1, [0, -128, -256, -448, -832, -1632, -3168, -5792]),
        ] {
            for (feedback, expected) in expected.into_iter().enumerate() {
                let mut channel = frozen(7 | (feedback as u8) << 3, 1, [0; 4], 0);
                channel.history = [513 * sign, 514 * sign];
                assert_eq!(channel.clock(None), expected);
            }
        }
    }

    #[test]
    fn maximum_feedback_preserves_logarithmic_raw_history() {
        let mut channel = frozen(0x3f, 1, [0; 4], 0);
        channel.set_frequency(1024, 3);
        for expected in [
            25, 376, 2872, 4432, -4480, 726, -3246, -8016, -6816, 8060, 7616, -2204, -7888, -7412,
            7532, 3656, 3558, -2684, 7332, -6368, 7696, 8168, 1172, 8104,
        ] {
            let previous = channel.history[0];
            channel.clock(None);
            assert_eq!(channel.history, [expected, previous]);
        }
    }

    #[test]
    fn mixed_sign_carriers_clip_in_s1_s3_s2_s4_order() {
        let mut channel = Channel::default();
        channel.set_frequency(1024, 3);
        channel.write_channel(0xb0, 7);
        for (operator, clocks) in channel.operators.iter_mut().zip([64, 192, 64, 0]) {
            operator.write(3, 1);
            operator.write(5, 31);
            operator.key(true, (1024, 3));
            for _ in 0..clocks {
                operator.clock((1024, 3), 0);
            }
        }
        assert_eq!(channel.clock(None), -32);
        assert_eq!(channel.history[0], 8168);
        assert_eq!(channel.previous_s2, -8168);
    }
}
