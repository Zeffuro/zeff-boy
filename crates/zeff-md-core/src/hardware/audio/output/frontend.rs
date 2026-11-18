use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const MAX_PENDING: i128 = 1_i128 << 48;

impl Output {
    pub(in crate::hardware::audio) fn level(&self) -> [i32; OUTPUT_CHANNELS] {
        self.level
    }

    pub(in crate::hardware::audio) fn write_frontend_state(&self, writer: &mut StateWriter) {
        for frame in self.pending {
            for value in frame {
                writer.write_u64(value as u64);
            }
        }
        for value in self.integrator {
            writer.write_u64(value as u64);
        }
        for value in self.level {
            writer.write_u32(value as u32);
        }
        writer.write_u8(self.cursor as u8);
    }

    pub(in crate::hardware::audio) fn read_frontend_state(
        reader: &mut StateReader<'_>,
    ) -> Result<Self> {
        let mut output = Self::at_level([0; OUTPUT_CHANNELS]);
        for frame in &mut output.pending {
            for value in frame {
                *value = reader.read_u64()? as i64;
            }
        }
        for value in &mut output.integrator {
            *value = reader.read_u64()? as i64;
        }
        for value in &mut output.level {
            *value = reader.read_u32()? as i32;
        }
        output.cursor = usize::from(reader.read_u8()?);
        output.validate_frontend_state()?;
        Ok(output)
    }

    fn validate_frontend_state(&self) -> Result<()> {
        ensure!(self.cursor < HISTORY, "invalid MD output cursor");
        for side in 0..OUTPUT_CHANNELS {
            ensure!(
                (i32::from(i16::MIN)..=i32::from(i16::MAX)).contains(&self.level[side]),
                "invalid MD output level"
            );
            let mut total = i128::from(self.integrator[side]);
            for frame in &self.pending {
                let value = i128::from(frame[side]);
                // Conservation alone permits large cancelling histories that overflow later DSP.
                ensure!(
                    (-MAX_PENDING..=MAX_PENDING).contains(&value),
                    "MD output history exceeds numeric bound"
                );
                total += value;
            }
            ensure!(
                total == i128::from(self.level[side]) * i128::from(UNIT),
                "invalid MD output conservation"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
