use super::*;

pub(super) const LATCH_STEPS: usize = 4;
const LATCH_CHANNELS: [usize; CHANNELS] = [1, 5, 3, 0, 4, 2];

impl Opn2 {
    pub(super) fn publish_slot(&mut self, cycle: usize) {
        if !cycle.is_multiple_of(LATCH_STEPS) {
            return;
        }
        let channel = LATCH_CHANNELS[cycle / LATCH_STEPS];
        if self.scheduler.ready & (1 << channel) != 0 {
            self.output[channel] = self.scheduler.output[channel];
            self.output_pan[channel] = self.channels[channel].pan() & (PAN_LEFT | PAN_RIGHT);
        }
    }

    pub(crate) fn clocks_to_sample(&self) -> u64 {
        let interval = scheduler::SLOT_CLOCKS * LATCH_STEPS as u64;
        let phase = self.phase % interval;
        if phase < scheduler::SLOT_CLOCKS {
            scheduler::SLOT_CLOCKS - phase
        } else {
            interval + scheduler::SLOT_CLOCKS - phase
        }
    }
}

#[cfg(test)]
mod tests;
