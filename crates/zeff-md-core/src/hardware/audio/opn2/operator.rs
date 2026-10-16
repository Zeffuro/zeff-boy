use super::constants::*;
use super::tables;
mod pipeline;
mod ssg;
mod state;
pub(super) use pipeline::Pipeline;
#[cfg(test)]
pub(super) use pipeline::Sample;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Envelope {
    Off,
    Attack,
    Decay,
    Sustain,
    Release,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Operator {
    parameters: [u8; 7],
    phase: u32,
    attenuation: u16,
    envelope: Envelope,
    keyed: bool,
    pub(super) pipeline: Pipeline,
    ssg: ssg::Ssg,
}

impl Default for Operator {
    fn default() -> Self {
        Self {
            parameters: [0; 7],
            phase: 0,
            attenuation: ENVELOPE_MAX,
            envelope: Envelope::Off,
            keyed: false,
            pipeline: Pipeline::default(),
            ssg: ssg::Ssg::default(),
        }
    }
}

impl Operator {
    pub(super) fn write(&mut self, group: u8, value: u8) {
        self.parameters[usize::from(group - 3)] = value;
    }

    pub(super) fn active(&self) -> bool {
        self.envelope != Envelope::Off
    }

    #[cfg(test)]
    pub(super) fn key(&mut self, on: bool, frequency: (u16, u8)) {
        if on && !self.keyed {
            self.phase = 0;
            self.envelope = Envelope::Attack;
            if self.attenuation == 0
                || self.effective_rate(self.parameters[2] & 31, key_code(frequency.0, frequency.1))
                    >= 62
            {
                self.attenuation = 0;
                self.envelope = Envelope::Decay;
            }
        } else if !on && self.keyed && self.active() {
            self.envelope = Envelope::Release;
        }
        self.keyed = on;
    }

    #[cfg(test)]
    pub(super) fn clock(&mut self, frequency: (u16, u8), phase_modulation: i32) -> i32 {
        self.clock_with_lfo(frequency, phase_modulation, frequency.0 * 2, 0)
    }

    #[cfg(test)]
    pub(super) fn clock_with_lfo(
        &mut self,
        frequency: (u16, u8),
        phase_modulation: i32,
        modulated_fnum: u16,
        am: u16,
    ) -> i32 {
        if !self.active() {
            return 0;
        }
        let (fnum, block) = frequency;
        let key_code = key_code(fnum, block);
        let phase = self
            .phase
            .wrapping_add((phase_modulation << (PHASE_BITS - SINE_BITS)) as u32);
        let level = self
            .attenuation
            .saturating_add(u16::from(self.parameters[1] & 127) * 8)
            .saturating_add(if self.parameters[3] & 128 != 0 { am } else { 0 });
        let sample = tables::output(phase, level);
        self.phase =
            self.phase
                .wrapping_add(self.modulated_phase_step(modulated_fnum, block, key_code))
                & PHASE_MASK;
        sample
    }

    #[cfg(test)]
    fn phase_step(&self, fnum: u16, block: u8, key_code: u8) -> u32 {
        self.modulated_phase_step(fnum * 2, block, key_code)
    }

    pub(super) fn modulated_phase_step(&self, fnum: u16, block: u8, key_code: u8) -> u32 {
        let base = (i32::from(fnum) << block) / 4;
        let detune = self.parameters[0] >> 4 & 7;
        let magnitude =
            i32::from(DETUNE_MAGNITUDES[usize::from(key_code)][usize::from(detune & 3)]);
        let offset = if detune & 4 != 0 {
            -magnitude
        } else {
            magnitude
        };
        let step = ((base + offset) & DETUNE_MASK) as u32;
        match self.parameters[0] & 15 {
            0 => step / 2,
            multiple => step * u32::from(multiple),
        }
    }

    #[cfg(test)]
    pub(super) fn clock_envelope(&mut self, frequency: (u16, u8), counter: u16) {
        if self.envelope == Envelope::Decay && self.attenuation >> 4 == self.sustain_target() >> 4 {
            self.envelope = Envelope::Sustain;
        }
        let rate = match self.envelope {
            Envelope::Off => return,
            Envelope::Attack => self.parameters[2] & 31,
            Envelope::Decay => self.parameters[3] & 31,
            Envelope::Sustain => self.parameters[4] & 31,
            Envelope::Release => (self.parameters[5] & 15) * 2 + 1,
        };
        if rate == 0 {
            return;
        }
        let effective = self.effective_rate(rate, key_code(frequency.0, frequency.1));
        if self.envelope == Envelope::Attack && effective >= 62 {
            return;
        }
        let increment = super::envelope_clock::increment(effective, counter);
        if increment == 0 {
            return;
        }
        if self.envelope == Envelope::Attack {
            let change = ((u32::from(self.attenuation) + 1) * u32::from(increment)).div_ceil(16);
            self.attenuation = self.attenuation.saturating_sub(change as u16);
            if self.attenuation == 0 {
                self.envelope = Envelope::Decay;
            }
        } else {
            self.attenuation = self.attenuation.saturating_add(increment).min(ENVELOPE_MAX);
            if self.attenuation == ENVELOPE_MAX {
                self.envelope = Envelope::Off;
            }
        }
    }

    #[cfg(test)]
    fn effective_rate(&self, rate: u8, key_code: u8) -> u8 {
        if rate == 0 {
            return 0;
        }
        let scaling = self.parameters[2] >> 6;
        (rate * 2 + (key_code >> (3 - scaling))).min(63)
    }

    fn sustain_target(&self) -> u16 {
        match self.parameters[5] >> 4 {
            15 => 992,
            level => u16::from(level) * 32,
        }
    }

    #[cfg(test)]
    pub(super) fn current_sample(self, am: u16) -> Sample {
        Sample {
            phase: self.phase,
            level: self
                .attenuation
                .saturating_add(u16::from(self.parameters[1] & 127) * 8)
                .saturating_add(if self.parameters[3] & 128 != 0 { am } else { 0 }),
            valid: true,
        }
    }

    #[cfg(test)]
    pub(super) fn control_snapshot(self) -> (u16, u8, bool, u32) {
        (
            self.attenuation,
            match self.envelope {
                Envelope::Off | Envelope::Release => 3,
                Envelope::Attack => 0,
                Envelope::Decay => 1,
                Envelope::Sustain => 2,
            },
            self.keyed,
            self.phase,
        )
    }
}

pub(super) fn key_code(fnum: u16, block: u8) -> u8 {
    let note = match fnum {
        0..=0x37f => 0,
        0x380..=0x3ff => 1,
        0x400..=0x47f => 2,
        _ => 3,
    };
    block << 2 | note
}

#[cfg(test)]
mod envelope_tests;
#[cfg(test)]
mod tests;
