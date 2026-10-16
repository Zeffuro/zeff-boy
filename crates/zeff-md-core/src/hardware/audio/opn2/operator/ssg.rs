use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Ssg {
    pub(super) enabled: bool,
    direction: bool,
    pub(super) inverted: bool,
    pub(super) repeat: bool,
    pub(super) hold_up: bool,
    pub(super) reset: bool,
}

impl Ssg {
    pub(super) fn clock(&mut self, mode: u8, level: u16, keyed: bool, sampled: bool) {
        let enabled = mode & 8 != 0;
        let crossed = enabled && level & 512 != 0;
        self.reset = crossed && mode & 3 == 0;
        self.repeat = crossed && mode & 1 == 0;
        self.hold_up = enabled && sampled && matches!(mode & 7, 3 | 5);
        self.direction = enabled
            && keyed
            && match (crossed, mode & 3) {
                (true, 2) => !self.direction,
                (true, 3) => true,
                _ => self.direction,
            };
        self.enabled = enabled;
        self.inverted = enabled && keyed && (self.direction ^ (mode & 4 != 0));
    }

    pub(super) fn level(self, level: u16) -> u16 {
        if self.inverted {
            512_u16.wrapping_sub(level) & 1023
        } else {
            level
        }
    }

    pub(super) fn write_state(self, w: &mut StateWriter) {
        let bits = [
            self.enabled,
            self.direction,
            self.inverted,
            self.repeat,
            self.hold_up,
            self.reset,
        ];
        w.write_u8(
            bits.into_iter()
                .enumerate()
                .fold(0, |v, (i, b)| v | u8::from(b) << i),
        );
    }

    pub(super) fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let bits = r.read_u8()?;
        ensure!(bits < 64, "invalid FM SSG latches");
        Ok(Self {
            enabled: bits & 1 != 0,
            direction: bits & 2 != 0,
            inverted: bits & 4 != 0,
            repeat: bits & 8 != 0,
            hold_up: bits & 16 != 0,
            reset: bits & 32 != 0,
        })
    }
}

#[cfg(test)]
mod tests;
