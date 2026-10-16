use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

const ALL: u8 = (1 << OPERATORS) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Render {
    pub(super) modulation: [i32; OPERATORS],
    pub(super) raw: [i32; OPERATORS],
    pub(super) mod_valid: u8,
    pub(super) raw_valid: u8,
}

impl Default for Render {
    fn default() -> Self {
        Self {
            modulation: [0; OPERATORS],
            raw: [0; OPERATORS],
            mod_valid: ALL,
            raw_valid: ALL,
        }
    }
}

impl Render {
    pub(super) fn migrated() -> Self {
        Self {
            mod_valid: 0,
            raw_valid: 0,
            ..Self::default()
        }
    }

    pub(super) fn quiet(&self) -> bool {
        self.modulation == [0; OPERATORS]
            && self.raw == [0; OPERATORS]
            && self.mod_valid == ALL
            && self.raw_valid == ALL
    }

    pub(super) fn write_state(&self, w: &mut Writer) {
        for value in self.modulation.into_iter().chain(self.raw) {
            w.write_u32(value as u32);
        }
        w.write_u8(self.mod_valid);
        w.write_u8(self.raw_valid);
    }

    pub(super) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        let mut result = Self::default();
        let peak = super::super::constants::OPERATOR_PEAK;
        for value in result.modulation.iter_mut().chain(&mut result.raw) {
            *value = r.read_u32()? as i32;
            ensure!((-peak..=peak).contains(value), "invalid FM render value");
        }
        result.mod_valid = r.read_u8()?;
        result.raw_valid = r.read_u8()?;
        ensure!(
            result.mod_valid <= ALL && result.raw_valid <= ALL,
            "invalid FM render validity"
        );
        Ok(result)
    }
}

impl Channel {
    pub(crate) fn prepare_modulation(&mut self, operator: usize) {
        let input = match operator {
            0 => {
                let depth = self.feedback;
                if depth == 0 {
                    0
                } else {
                    (self.history[0] + self.history[1]) >> (10 - depth)
                }
            }
            1 => match self.algorithm {
                0 | 3..=6 => self.render.raw[0] >> OUTPUT_PHASE_SHIFT,
                _ => 0,
            },
            2 => {
                (match self.algorithm {
                    0 | 2 => self.previous_s2,
                    1 => self.history[0] + self.previous_s2,
                    5 => self.history[0],
                    _ => 0,
                }) >> OUTPUT_PHASE_SHIFT
            }
            _ => {
                (match self.algorithm {
                    0 | 1 | 4 => self.render.raw[2],
                    2 => self.render.raw[0] + self.render.raw[2],
                    3 => self.previous_s2 + self.render.raw[2],
                    5 => self.render.raw[0],
                    _ => 0,
                }) >> OUTPUT_PHASE_SHIFT
            }
        };
        self.render.modulation[operator] = input;
        self.render.mod_valid |= 1 << operator;
    }

    pub(crate) fn render_operator(&mut self, operator: usize) {
        let sample = self.operators[operator].pipeline.sample;
        self.render.raw[operator] = Operator::render(sample, self.render.modulation[operator]);
        let bit = 1 << operator;
        let valid = if sample.valid {
            self.render.mod_valid & bit
        } else {
            0
        };
        self.render.raw_valid = (self.render.raw_valid & !bit) | valid;
    }

    pub(crate) fn raw_operator(&self, operator: usize) -> (i32, bool) {
        (
            self.render.raw[operator],
            self.render.raw_valid & (1 << operator) != 0,
        )
    }

    pub(crate) fn consume_history(&mut self, operator: usize) {
        match operator {
            0 => self.history = [self.render.raw[0], self.history[0]],
            1 => self.previous_s2 = self.render.raw[1],
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
