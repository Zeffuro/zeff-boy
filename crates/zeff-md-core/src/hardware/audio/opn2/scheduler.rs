use super::*;
mod state;

pub(super) const SLOT_CLOCKS: u64 = 6;
pub(super) const SLOTS: usize = CHANNELS * constants::OPERATORS;
pub(super) const CLOCK_PREFIX: u32 = 98;
pub(super) const CLOCK_PERIOD: u32 = 72 * (constants::ENVELOPE_COUNTER_MAX as u32);
pub(super) const CLOCK_END: u32 = CLOCK_PREFIX + CLOCK_PERIOD;
pub(super) const OPERATOR_ORDER: [usize; 4] = [0, 2, 1, 3];
const SHIFTED_OPERATORS: [u16; SLOTS] = {
    let offsets = [SLOTS - 5, SLOTS - 4, SLOTS - 2, SLOTS - 1, 1];
    let mut rows = [0; SLOTS];
    let mut cycle = 0;
    while cycle < SLOTS {
        let mut field = 0;
        while field < offsets.len() {
            let slot = (cycle + offsets[field]) % SLOTS;
            rows[cycle] |= (OPERATOR_ORDER[slot / CHANNELS] as u16) << (field * 2);
            field += 1;
        }
        cycle += 1;
    }
    rows
};
pub(super) const ALL_OPERATORS: u8 = (1 << constants::OPERATORS) - 1;
pub(super) const CARRIERS_AT_BOUNDARY: u8 = ALL_OPERATORS >> 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Command {
    pub(super) channel: u8,
    pub(super) mask: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Scheduler {
    pub(super) step: u32,
    pub(super) command: Command,
    pub(super) pending: Option<Command>,
    pub(super) frequency: (u16, u8),
    pub(super) frequency_valid: bool,
    pub(super) output: [i32; CHANNELS],
    pub(super) ready: u8,
    pub(super) accumulator: [i32; CHANNELS],
    pub(super) carriers: [u8; CHANNELS],
}

impl Default for Scheduler {
    fn default() -> Self {
        Self {
            step: 0,
            command: Command {
                channel: 0,
                mask: 0,
            },
            pending: None,
            frequency: (0, 0),
            frequency_valid: true,
            output: [0; CHANNELS],
            ready: (1 << CHANNELS) - 1,
            accumulator: [0; CHANNELS],
            carriers: [ALL_OPERATORS; CHANNELS],
        }
    }
}

impl Scheduler {
    pub(super) fn key(&mut self, value: u8) {
        self.pending = Some(Command {
            channel: if value & 3 == 3 {
                255
            } else {
                (value & 3) + ((value & 4) >> 2) * 3
            },
            mask: value >> 4,
        });
    }

    pub(super) fn counter_at(step: u32) -> Option<u16> {
        if step < 26 || (step - 26) % 72 >= 24 {
            return None;
        }
        let beat = (step - 26) / 72;
        Some(if beat == 0 {
            0
        } else {
            ((beat - 1) % (constants::ENVELOPE_COUNTER_MAX as u32) + 1) as u16
        })
    }

    pub(super) fn clock(&mut self) {
        self.step += 1;
        if self.step == CLOCK_END {
            self.step = CLOCK_PREFIX;
        }
    }

    pub(super) fn advance_clock(&mut self, steps: u64) {
        let prefix = u64::from(CLOCK_PREFIX.saturating_sub(self.step));
        if steps < prefix {
            self.step += steps as u32;
            return;
        }
        let step = self.step.max(CLOCK_PREFIX) - CLOCK_PREFIX;
        self.step = CLOCK_PREFIX
            + ((u64::from(step) + (steps - prefix) % u64::from(CLOCK_PERIOD))
                % u64::from(CLOCK_PERIOD)) as u32;
    }
}

impl Opn2 {
    #[cfg(test)]
    pub(super) fn accumulate_slot(&mut self, slot: usize) {
        self.accumulate_operator(slot % CHANNELS, slot / CHANNELS);
    }

    fn accumulate_operator(&mut self, channel: usize, physical: usize) {
        let (raw, valid) = self.channels[channel].raw_operator(OPERATOR_ORDER[physical]);
        let algorithm = self.channels[channel].algorithm();
        if physical == 0 {
            let bit = 1 << channel;
            if self.scheduler.carriers[channel] == ALL_OPERATORS {
                self.scheduler.output[channel] =
                    self.scheduler.accumulator[channel] * dac::OUTPUT_SCALE;
                self.scheduler.ready |= bit;
            } else {
                self.scheduler.ready &= !bit;
            }
            self.scheduler.accumulator[channel] = 0;
            self.scheduler.carriers[channel] = 0;
        }
        if valid || !dac::carrier(physical, algorithm) {
            self.scheduler.carriers[channel] |= 1 << physical;
        }
        self.scheduler.accumulator[channel] = dac::carrier_sum(
            self.scheduler.accumulator[channel],
            raw,
            physical,
            algorithm,
        );
    }

    pub(super) fn slot(&self, slot: usize) -> &operator::Operator {
        self.channels[slot % CHANNELS].operator(OPERATOR_ORDER[slot / CHANNELS])
    }

    pub(super) fn slot_mut(&mut self, slot: usize) -> &mut operator::Operator {
        self.channels[slot % CHANNELS].operator_mut(OPERATOR_ORDER[slot / CHANNELS])
    }

    pub(super) fn slot_frequency(&self, slot: usize) -> (u16, u8) {
        self.operator_frequency(slot % CHANNELS, OPERATOR_ORDER[slot / CHANNELS])
    }

    fn operator_frequency(&self, channel: usize, logical: usize) -> (u16, u8) {
        self.channels[channel]
            .frequencies((channel == 2 && self.mode & 0xc0 != 0).then_some(self.special))[logical]
    }

    pub(super) fn clock_slot(&mut self) {
        let cycle = self.scheduler.step as usize % SLOTS;
        let channel = cycle % CHANNELS;
        let group = cycle / CHANNELS;
        let logical = OPERATOR_ORDER[group];
        self.timer_a.clock(cycle, self.mode, &mut self.flags);
        self.timer_b.clock(cycle, self.mode, &mut self.flags);
        self.lfo.clock_slot(cycle);
        let csm = channel == 2 && self.timer_a.csm();
        self.channels[channel].operator_mut(logical).sample_key(csm);
        let command = self.scheduler.command;
        if cycle == usize::from(command.channel) {
            for (group, logical) in OPERATOR_ORDER.into_iter().enumerate() {
                self.slot_mut(cycle + group * CHANNELS).pipeline.desired =
                    command.mask & (1 << logical) != 0;
            }
        }
        self.publish_slot(cycle);
        let consumed = (group + constants::OPERATORS - 1) % constants::OPERATORS;
        self.accumulate_operator(channel, consumed);
        let prepared = (group + 1) % constants::OPERATORS;
        self.channels[channel].prepare_modulation(OPERATOR_ORDER[prepared]);
        self.channels[channel].consume_history(OPERATOR_ORDER[consumed]);
        let shifted = SHIFTED_OPERATORS[cycle];
        let next_channel = if channel == CHANNELS - 1 {
            0
        } else {
            channel + 1
        };
        let rendered = usize::from(shifted & 3);
        self.channels[next_channel]
            .operator_mut(rendered)
            .capture_sample();
        self.channels[next_channel].render_operator(rendered);
        let mask_channel = if channel >= 4 {
            channel - 4
        } else {
            channel + 2
        };
        self.channels[mask_channel]
            .operator_mut(usize::from((shifted >> 2) & 3))
            .mask_increment();
        self.channels[next_channel]
            .operator_mut(rendered)
            .advance_phase();
        let frequency = self.scheduler.frequency;
        let modulated = lfo::frequency(frequency.0, self.lfo.sampled, self.channels[channel].pms());
        let valid = self.scheduler.frequency_valid;
        self.channels[channel]
            .operator_mut(logical)
            .prepare_phase(frequency, modulated, valid);
        let envelope_channel = if channel < 2 {
            channel + 4
        } else {
            channel - 2
        };
        self.channels[envelope_channel]
            .operator_mut(usize::from((shifted >> 4) & 3))
            .clock_pipeline_envelope();
        let generated_channel = if channel == 0 {
            CHANNELS - 1
        } else {
            channel - 1
        };
        let generated = usize::from((shifted >> 6) & 3);
        let omit_total_level = generated_channel == 2 && self.mode & 0xc0 == 0x80;
        self.channels[generated_channel]
            .operator_mut(generated)
            .generate_level(omit_total_level);
        let counter = Scheduler::counter_at(self.scheduler.step + 1);
        self.channels[generated_channel]
            .operator_mut(generated)
            .prepare_envelope(counter);
        let am = lfo::amplitude(self.lfo.sampled, self.channels[channel].ams());
        self.channels[channel]
            .operator_mut(logical)
            .select_rate(frequency, am, valid);
        self.scheduler.frequency =
            self.operator_frequency(next_channel, usize::from((shifted >> 8) & 3));
        self.scheduler.frequency_valid = true;
        if let Some(command) = self.scheduler.pending.take() {
            self.scheduler.command = command;
        }
        self.scheduler.clock();
        if cycle == SLOTS - 1 {
            self.envelope_clock.clock();
        }
    }
}

#[cfg(test)]
mod csm_tests;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod geometry_tests;

#[cfg(test)]
mod clock_tests;
