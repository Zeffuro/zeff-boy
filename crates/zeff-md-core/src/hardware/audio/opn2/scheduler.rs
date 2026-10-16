use super::*;
mod state;

pub(super) const SLOT_CLOCKS: u64 = 6;
pub(super) const SLOTS: usize = CHANNELS * constants::OPERATORS;
pub(super) const CLOCK_PREFIX: u32 = 98;
pub(super) const CLOCK_PERIOD: u32 = 72 * (constants::ENVELOPE_COUNTER_MAX as u32);
pub(super) const CLOCK_END: u32 = CLOCK_PREFIX + CLOCK_PERIOD;
pub(super) const OPERATOR_ORDER: [usize; 4] = [0, 2, 1, 3];
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
    pub(super) fn accumulate_slot(&mut self, slot: usize) {
        let channel = slot % CHANNELS;
        let physical = slot / CHANNELS;
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
        let channel = slot % CHANNELS;
        self.channels[channel]
            .frequencies((channel == 2 && self.mode & 0xc0 != 0).then_some(self.special))
            [OPERATOR_ORDER[slot / CHANNELS]]
    }

    pub(super) fn clock_slot(&mut self) {
        let cycle = self.scheduler.step as usize % SLOTS;
        self.timer_a.clock(cycle, self.mode, &mut self.flags);
        self.timer_b.clock(cycle, self.mode, &mut self.flags);
        self.lfo.clock_slot(cycle);
        let csm = cycle % CHANNELS == 2 && self.timer_a.csm();
        self.slot_mut(cycle).sample_key(csm);
        let command = self.scheduler.command;
        if cycle == usize::from(command.channel) {
            for (group, logical) in OPERATOR_ORDER.into_iter().enumerate() {
                self.slot_mut(cycle + group * CHANNELS).pipeline.desired =
                    command.mask & (1 << logical) != 0;
            }
        }
        self.publish_slot(cycle);
        let consumed = (cycle + SLOTS - CHANNELS) % SLOTS;
        self.accumulate_slot(consumed);
        let prepared = (cycle + CHANNELS) % SLOTS;
        self.channels[prepared % CHANNELS].prepare_modulation(OPERATOR_ORDER[prepared / CHANNELS]);
        self.channels[consumed % CHANNELS].consume_history(OPERATOR_ORDER[consumed / CHANNELS]);
        let rendered = (cycle + SLOTS - 5) % SLOTS;
        self.slot_mut(rendered).capture_sample();
        self.channels[rendered % CHANNELS].render_operator(OPERATOR_ORDER[rendered / CHANNELS]);
        self.slot_mut((cycle + SLOTS - 4) % SLOTS).mask_increment();
        self.slot_mut(rendered).advance_phase();
        let frequency = self.scheduler.frequency;
        let modulated = lfo::frequency(
            frequency.0,
            self.lfo.sampled,
            self.channels[cycle % CHANNELS].pms(),
        );
        let valid = self.scheduler.frequency_valid;
        self.slot_mut(cycle)
            .prepare_phase(frequency, modulated, valid);
        self.slot_mut((cycle + SLOTS - 2) % SLOTS)
            .clock_pipeline_envelope();
        let generated = (cycle + SLOTS - 1) % SLOTS;
        let omit_total_level = generated % CHANNELS == 2 && self.mode & 0xc0 == 0x80;
        self.slot_mut(generated).generate_level(omit_total_level);
        let counter = Scheduler::counter_at(self.scheduler.step + 1);
        self.slot_mut((cycle + SLOTS - 1) % SLOTS)
            .prepare_envelope(counter);
        let am = lfo::amplitude(self.lfo.sampled, self.channels[cycle % CHANNELS].ams());
        self.slot_mut(cycle).select_rate(frequency, am, valid);
        self.scheduler.frequency = self.slot_frequency((cycle + 1) % SLOTS);
        self.scheduler.frequency_valid = true;
        if let Some(command) = self.scheduler.pending.take() {
            self.scheduler.command = command;
        }
        self.scheduler.advance_clock(1);
        if cycle == SLOTS - 1 {
            self.envelope_clock.clock();
        }
    }
}

#[cfg(test)]
mod csm_tests;
#[cfg(test)]
mod tests;
