use super::*;
use scheduler::{OPERATOR_ORDER, SLOTS};

impl Opn2 {
    fn quiet_at_scan_boundary(&self) -> bool {
        self.scheduler.step.is_multiple_of(SLOTS as u32)
            && !self.timer_a.can_key(self.mode)
            && self.scheduler.ready == (1 << CHANNELS) - 1
            && self.scheduler.accumulator == [0; CHANNELS]
            && self.scheduler.output == [0; CHANNELS]
            && self.scheduler.carriers == [scheduler::CARRIERS_AT_BOUNDARY; CHANNELS]
            && self.output == [0; CHANNELS]
            && self
                .channels
                .iter()
                .zip(self.output_pan)
                .all(|(channel, pan)| channel.pan() & (PAN_LEFT | PAN_RIGHT) == pan)
            && self.scheduler.pending.is_none()
            && (self.scheduler.command.mask == 0 || self.scheduler.command.channel == 255)
            && self.scheduler.frequency_valid
            && self.scheduler.frequency == self.slot_frequency(0)
            && !self.channels.iter().any(Channel::active)
            && self.channels.iter().all(Channel::quiet_history)
            && (0..SLOTS).all(|slot| self.slot(slot).quiet_pipeline())
    }

    pub(super) fn advance_slots(&mut self, mut steps: u64) {
        while steps != 0 {
            if steps >= 3 * SLOTS as u64 && self.quiet_at_scan_boundary() {
                let frames = steps / SLOTS as u64 - 2;
                self.skip_quiet_frames(frames);
                steps -= frames * SLOTS as u64;
            } else {
                self.clock_slot();
                steps -= 1;
            }
        }
    }

    fn skip_quiet_frames(&mut self, frames: u64) {
        self.timer_a.advance_slots(
            self.scheduler.step as usize % SLOTS,
            frames * SLOTS as u64,
            self.mode,
            &mut self.flags,
        );
        self.timer_b.advance_slots(
            self.scheduler.step as usize % SLOTS,
            frames * SLOTS as u64,
            self.mode,
            &mut self.flags,
        );
        let counts = self.lfo.counter_histogram(frames);
        let mut last = self.lfo;
        last.advance(frames - 1);
        for slot in 0..SLOTS {
            let channel = slot % CHANNELS;
            let operator = self.channels[channel].operator(OPERATOR_ORDER[slot / CHANNELS]);
            let frequency = self.slot_frequency(slot);
            let pms = self.channels[channel].pms();
            let keycode = operator::key_code(frequency.0, frequency.1);
            let step = |counter| {
                operator.modulated_phase_step(
                    lfo::frequency(frequency.0, counter, pms),
                    frequency.1,
                    keycode,
                ) & constants::PHASE_MASK
            };
            let mut sum = 0_u32;
            for (counter, count) in counts.into_iter().enumerate() {
                sum = sum.wrapping_add(
                    ((count & u64::from(constants::PHASE_MASK)) * u64::from(step(counter as u8)))
                        as u32,
                );
            }
            let final_increment = step(last.counter);
            // Five slots consume the increment latched in the preceding scan.
            if slot >= SLOTS - 5 {
                sum = sum
                    .wrapping_add(operator.pipeline.increment)
                    .wrapping_sub(final_increment);
            }
            self.slot_mut(slot).skip_phase(sum, final_increment);
        }
        self.scheduler.advance_clock(frames * SLOTS as u64);
        self.lfo.advance(frames);
        self.envelope_clock.advance(frames);
    }
}

#[cfg(test)]
mod tests;
