use super::constants::*;
mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PsgDebugSnapshot {
    pub periods: [u16; PSG_TONES],
    pub volumes: [u8; PSG_CHANNELS],
    pub counters: [u16; PSG_TONES],
    pub outputs: [bool; PSG_TONES],
    pub latched_register: u8,
    pub noise_control: u8,
    pub noise_counter: u16,
    pub noise_clock: bool,
    pub noise_lfsr: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Psg {
    state: PsgDebugSnapshot,
}

impl Default for Psg {
    fn default() -> Self {
        Self {
            state: PsgDebugSnapshot {
                periods: [0; PSG_TONES],
                volumes: [PSG_DATA_MASK; PSG_CHANNELS],
                counters: [1; PSG_TONES],
                outputs: [true; PSG_TONES],
                latched_register: 0,
                noise_control: 0,
                noise_counter: PSG_NOISE_PERIOD,
                noise_clock: true,
                noise_lfsr: PSG_NOISE_SEED,
            },
        }
    }
}

impl Psg {
    pub fn debug_snapshot(&self) -> PsgDebugSnapshot {
        self.state
    }

    pub fn write_data(&mut self, value: u8) {
        let latch = value & PSG_LATCH_BIT != 0;
        if latch {
            self.state.latched_register = (value >> PSG_REGISTER_SHIFT) & PSG_REGISTER_MASK;
        }
        let register = self.state.latched_register;
        let channel = usize::from(register >> 1);
        if register & PSG_VOLUME_BIT != 0 {
            self.state.volumes[channel] = value & PSG_DATA_MASK;
        } else if register == PSG_NOISE_REGISTER {
            self.state.noise_control = value & PSG_NOISE_MASK;
            self.state.noise_lfsr = PSG_NOISE_SEED;
        } else if latch {
            self.state.periods[channel] = (self.state.periods[channel] & !PSG_TONE_LOW_MASK)
                | u16::from(value & PSG_DATA_MASK);
        } else {
            self.state.periods[channel] = (self.state.periods[channel] & PSG_TONE_LOW_MASK)
                | (u16::from(value & PSG_TONE_HIGH_MASK) << PSG_TONE_HIGH_SHIFT);
        }
    }

    pub fn advance(&mut self, clocks: u64) {
        let mut tone_rises = 0;
        for channel in 0..PSG_TONES {
            let rises = advance_counter(
                &mut self.state.counters[channel],
                &mut self.state.outputs[channel],
                self.state.periods[channel].max(1),
                clocks,
            );
            if channel == PSG_TONES - 1 {
                tone_rises = rises;
            }
        }
        let rate = self.state.noise_control & PSG_NOISE_RATE_MASK;
        let shifts = if rate == PSG_NOISE_TONE_RATE {
            tone_rises
        } else {
            advance_counter(
                &mut self.state.noise_counter,
                &mut self.state.noise_clock,
                PSG_NOISE_PERIOD << rate,
                clocks,
            )
        };
        self.state.noise_lfsr = advance_lfsr(
            self.state.noise_lfsr,
            shifts,
            self.state.noise_control & PSG_NOISE_WHITE != 0,
        );
    }

    pub fn sample(&self) -> i16 {
        self.sample_with_mutes([false; PSG_CHANNELS])
    }

    pub(super) fn sample_with_mutes(&self, mutes: [bool; PSG_CHANNELS]) -> i16 {
        let mut sample = 0;
        for (channel, muted) in mutes.into_iter().enumerate() {
            if muted {
                continue;
            }
            let high = if channel < PSG_TONES {
                self.state.periods[channel] <= 1 || self.state.outputs[channel]
            } else {
                self.state.noise_lfsr & 1 != 0
            };
            let level = PSG_VOLUME_LEVELS[usize::from(self.state.volumes[channel])];
            sample += if high { level } else { -level };
        }
        sample
    }
}

fn advance_counter(counter: &mut u16, high: &mut bool, period: u16, clocks: u64) -> u64 {
    if clocks < u64::from(*counter) {
        *counter -= clocks as u16;
        return 0;
    }
    let remaining = clocks - u64::from(*counter);
    let edges = 1 + remaining / u64::from(period);
    let rises = edges / 2 + (edges & 1) * u64::from(!*high);
    *counter = period - (remaining % u64::from(period)) as u16;
    *high ^= edges & 1 != 0;
    rises
}

fn lfsr_step(value: u16, white: bool) -> u16 {
    let feedback = if white {
        value ^ (value >> PSG_NOISE_TAP_SHIFT)
    } else {
        value
    } & 1;
    (value >> 1) | (feedback * PSG_NOISE_SEED)
}

fn advance_lfsr(mut value: u16, mut shifts: u64, white: bool) -> u16 {
    if shifts <= PSG_NOISE_BITS as u64 {
        for _ in 0..shifts {
            value = lfsr_step(value, white);
        }
        return value;
    }
    let mut powers = std::array::from_fn::<_, PSG_NOISE_BITS, _>(|bit| lfsr_step(1 << bit, white));
    let apply = |value: u16, powers: &[u16; PSG_NOISE_BITS]| {
        powers
            .iter()
            .enumerate()
            .filter(|(bit, _)| value & (1 << bit) != 0)
            .fold(0, |result, (_, &basis)| result ^ basis)
    };
    while shifts != 0 {
        if shifts & 1 != 0 {
            value = apply(value, &powers);
        }
        powers = std::array::from_fn(|bit| apply(powers[bit], &powers));
        shifts >>= 1;
    }
    value
}
