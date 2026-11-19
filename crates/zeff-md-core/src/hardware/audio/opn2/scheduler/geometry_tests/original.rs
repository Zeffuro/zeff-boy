use super::*;

impl Opn2 {
    pub(super) fn slot_frequency_original(&self, slot: usize) -> (u16, u8) {
        let channel = slot % CHANNELS;
        self.channels[channel]
            .frequencies((channel == 2 && self.mode & 0xc0 != 0).then_some(self.special))
            [OPERATOR_ORDER[slot / CHANNELS]]
    }

    pub(super) fn clock_slot_original(&mut self) {
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
        let rendered = (cycle + SLOTS - 5) % SLOTS;
        self.slot_mut(rendered).capture_sample();
        self.channels[rendered % CHANNELS].render_operator(OPERATOR_ORDER[rendered / CHANNELS]);
        self.slot_mut((cycle + SLOTS - 4) % SLOTS).mask_increment();
        self.slot_mut(rendered).advance_phase();
        let frequency = self.scheduler.frequency;
        let modulated = lfo::frequency(frequency.0, self.lfo.sampled, self.channels[channel].pms());
        let valid = self.scheduler.frequency_valid;
        self.channels[channel]
            .operator_mut(logical)
            .prepare_phase(frequency, modulated, valid);
        self.slot_mut((cycle + SLOTS - 2) % SLOTS)
            .clock_pipeline_envelope();
        let generated = (cycle + SLOTS - 1) % SLOTS;
        let omit_total_level = generated % CHANNELS == 2 && self.mode & 0xc0 == 0x80;
        self.slot_mut(generated).generate_level(omit_total_level);
        let counter = Scheduler::counter_at(self.scheduler.step + 1);
        self.slot_mut((cycle + SLOTS - 1) % SLOTS)
            .prepare_envelope(counter);
        let am = lfo::amplitude(self.lfo.sampled, self.channels[channel].ams());
        self.channels[channel]
            .operator_mut(logical)
            .select_rate(frequency, am, valid);
        self.scheduler.frequency = self.slot_frequency_original((cycle + 1) % SLOTS);
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
