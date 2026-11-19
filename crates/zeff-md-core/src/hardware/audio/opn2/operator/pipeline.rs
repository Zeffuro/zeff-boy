use super::*;
mod state;

pub(crate) const UNKNOWN_TOTAL_LEVEL: u8 = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Sample {
    pub(crate) phase: u32,
    pub(crate) level: u16,
    pub(crate) valid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Rate {
    pub(crate) enabled: bool,
    pub(crate) effective: u8,
    pub(crate) sustain: u16,
    pub(crate) valid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Action {
    pub(crate) increment: u8,
    pub(crate) maximum: bool,
    pub(crate) sustain: u16,
    pub(crate) valid: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Pipeline {
    pub(crate) desired: bool,
    pub(crate) sampled: bool,
    pub(crate) csm: bool,
    pub(crate) selected: Rate,
    pub(crate) prepared: Action,
    pub(crate) increment: u32,
    pub(crate) increment_valid: bool,
    pub(crate) reset: bool,
    pub(crate) level_add: u16,
    pub(crate) total_level: u8,
    pub(crate) level: u16,
    pub(crate) level_valid: bool,
    pub(crate) sample: Sample,
}

impl Default for Pipeline {
    fn default() -> Self {
        Self {
            desired: false,
            sampled: false,
            csm: false,
            selected: Rate {
                enabled: false,
                effective: 0,
                sustain: 0,
                valid: true,
            },
            prepared: Action {
                increment: 0,
                maximum: false,
                sustain: 0,
                valid: true,
            },
            increment: 0,
            increment_valid: true,
            reset: false,
            level_add: 0,
            total_level: 0,
            level: ENVELOPE_MAX,
            level_valid: true,
            sample: Sample::default(),
        }
    }
}

impl Operator {
    pub(crate) fn sample_key(&mut self, csm: bool) {
        self.pipeline.csm = csm;
        self.pipeline.sampled = self.pipeline.desired || csm;
    }

    pub(crate) fn select_rate(&mut self, frequency: (u16, u8), am: u16, valid: bool) {
        self.ssg.clock(
            self.parameters[6],
            self.attenuation,
            self.keyed,
            self.pipeline.sampled,
        );
        let raw = if (!self.keyed && self.pipeline.sampled) || (self.keyed && self.ssg.repeat) {
            self.parameters[2] & 31
        } else {
            match self.envelope {
                Envelope::Attack => self.parameters[2] & 31,
                Envelope::Decay => self.parameters[3] & 31,
                Envelope::Sustain => self.parameters[4] & 31,
                Envelope::Off | Envelope::Release => (self.parameters[5] & 15) * 2 + 1,
            }
        };
        let scaling = self.parameters[2] >> 6;
        self.pipeline.selected = Rate {
            enabled: raw != 0,
            effective: (raw * 2 + (key_code(frequency.0, frequency.1) >> (3 - scaling))).min(63),
            sustain: self.sustain_target(),
            valid,
        };
        self.pipeline.total_level = self.parameters[1] & 127;
        self.pipeline.level_add = u16::from(self.pipeline.total_level) * 8
            + if self.parameters[3] & 128 != 0 { am } else { 0 };
    }

    pub(crate) fn prepare_envelope(&mut self, counter: Option<u16>) {
        let rate = self.pipeline.selected;
        self.pipeline.prepared = Action {
            increment: if rate.enabled {
                counter.map_or(0, |counter| {
                    super::super::envelope_clock::increment(rate.effective, counter) as u8
                })
            } else {
                0
            },
            maximum: rate.effective >= 62,
            sustain: rate.sustain,
            valid: rate.valid,
        };
    }

    pub(crate) fn clock_pipeline_envelope(&mut self) {
        let action = self.pipeline.prepared;
        if !action.valid {
            return;
        }
        let on = self.pipeline.sampled;
        let rising = on && !self.keyed;
        let old = self.envelope;
        let level = if self.keyed && !on {
            self.ssg.level(self.attenuation)
        } else {
            self.attenuation
        };
        let mut next = level;
        let mut increment = 0;
        let event = rising || (self.keyed && self.ssg.repeat);
        let off = level >= if self.ssg.enabled { 512 } else { 1008 };
        self.pipeline.reset = rising || self.ssg.reset;
        if event {
            self.envelope = Envelope::Attack;
            if action.maximum {
                next = 0;
            } else if old == Envelope::Attack && level != 0 && action.increment != 0 && on {
                increment = Self::attack_increment(level, action.increment);
            }
        } else {
            match old {
                Envelope::Attack if level == 0 => self.envelope = Envelope::Decay,
                Envelope::Attack if !action.maximum && on => {
                    increment = Self::attack_increment(level, action.increment);
                }
                Envelope::Decay if level >> 4 == action.sustain >> 4 => {
                    self.envelope = Envelope::Sustain
                }
                Envelope::Attack => {}
                _ if !off => {
                    increment = i32::from(action.increment) << if self.ssg.enabled { 2 } else { 0 };
                }
                _ => {}
            }
            if !on {
                self.envelope = Envelope::Release;
            }
        }
        if self.pipeline.csm && self.pipeline.total_level != UNKNOWN_TOTAL_LEVEL {
            next |= u16::from(self.pipeline.total_level) * 8;
        }
        if !event && !self.ssg.hold_up && old != Envelope::Attack && off {
            next = ENVELOPE_MAX;
            self.envelope = Envelope::Off;
        }
        self.attenuation = (i32::from(next) + increment) as u16 & ENVELOPE_MAX;
        self.keyed = on;
    }

    fn attack_increment(level: u16, increment: u8) -> i32 {
        -(((u32::from(level) + 1) * u32::from(increment)).div_ceil(16) as i32)
    }

    pub(crate) fn generate_level(&mut self, omit_total_level: bool) {
        let add = if omit_total_level && self.pipeline.total_level != UNKNOWN_TOTAL_LEVEL {
            self.pipeline.level_add - u16::from(self.pipeline.total_level) * 8
        } else {
            self.pipeline.level_add
        };
        self.pipeline.level = self
            .ssg
            .level(self.attenuation)
            .saturating_add(add)
            .min(ENVELOPE_MAX);
        self.pipeline.level_valid = self.pipeline.selected.valid;
    }

    pub(crate) fn capture_sample(&mut self) {
        self.pipeline.sample = Sample {
            phase: self.phase,
            level: self.pipeline.level,
            valid: self.pipeline.level_valid,
        };
    }

    pub(crate) fn mask_increment(&mut self) {
        if self.pipeline.reset {
            self.pipeline.increment = 0;
        }
    }

    pub(crate) fn advance_phase(&mut self) {
        if self.pipeline.increment_valid {
            self.phase = (if self.pipeline.reset { 0 } else { self.phase })
                .wrapping_add(self.pipeline.increment)
                & PHASE_MASK;
        }
    }

    pub(crate) fn prepare_phase(&mut self, frequency: (u16, u8), fnum: u16, valid: bool) {
        self.pipeline.increment =
            self.modulated_phase_step(fnum, frequency.1, key_code(frequency.0, frequency.1))
                & PHASE_MASK;
        self.pipeline.increment_valid = valid;
    }

    pub(crate) fn render(sample: Sample, modulation: i32) -> i32 {
        let phase = sample
            .phase
            .wrapping_add((modulation << (PHASE_BITS - SINE_BITS)) as u32);
        tables::output(phase, sample.level)
    }

    pub(crate) fn quiet_pipeline(&self) -> bool {
        let pipeline = self.pipeline;
        matches!(self.envelope, Envelope::Off | Envelope::Release)
            && self.attenuation == ENVELOPE_MAX
            && !self.keyed
            && !pipeline.desired
            && !pipeline.sampled
            && !pipeline.csm
            && !pipeline.reset
            && pipeline.selected.valid
            && pipeline.prepared.valid
            && pipeline.increment_valid
            && pipeline.level_valid
            && pipeline.level == ENVELOPE_MAX
            && pipeline.sample.valid
            && pipeline.sample.level == ENVELOPE_MAX
            && pipeline.total_level != UNKNOWN_TOTAL_LEVEL
            && self.parameters[6] & 8 == 0
            && self.ssg == ssg::Ssg::default()
    }

    pub(crate) fn skip_phase(&mut self, amount: u32, increment: u32) {
        self.phase = self.phase.wrapping_add(amount) & PHASE_MASK;
        self.pipeline.increment = increment;
    }
}

#[cfg(test)]
mod csm_tests;
#[cfg(test)]
mod tests;
