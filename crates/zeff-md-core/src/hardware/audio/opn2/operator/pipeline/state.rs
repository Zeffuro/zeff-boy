use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl Pipeline {
    pub(crate) fn write_state(&self, w: &mut Writer) {
        w.write_bool(self.desired);
        w.write_bool(self.sampled);
        w.write_bool(self.selected.enabled);
        w.write_u8(self.selected.effective);
        w.write_u16(self.selected.sustain);
        w.write_bool(self.selected.valid);
        w.write_u8(self.prepared.increment);
        w.write_bool(self.prepared.maximum);
        w.write_u16(self.prepared.sustain);
        w.write_bool(self.prepared.valid);
        w.write_u32(self.increment);
        w.write_bool(self.increment_valid);
        w.write_bool(self.reset);
        w.write_u16(self.level_add);
        w.write_u16(self.level);
        w.write_bool(self.level_valid);
        w.write_u32(self.sample.phase);
        w.write_u16(self.sample.level);
        w.write_bool(self.sample.valid);
        {
            w.write_u8(self.total_level);
        }
        {
            w.write_bool(self.csm);
        }
    }

    pub(crate) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        let pipeline = Self {
            desired: r.read_bool()?,
            sampled: r.read_bool()?,
            selected: Rate {
                enabled: r.read_bool()?,
                effective: r.read_u8()?,
                sustain: r.read_u16()?,
                valid: r.read_bool()?,
            },
            prepared: Action {
                increment: r.read_u8()?,
                maximum: r.read_bool()?,
                sustain: r.read_u16()?,
                valid: r.read_bool()?,
            },
            increment: r.read_u32()?,
            increment_valid: r.read_bool()?,
            reset: r.read_bool()?,
            level_add: r.read_u16()?,
            level: r.read_u16()?,
            level_valid: r.read_bool()?,
            sample: Sample {
                phase: r.read_u32()?,
                level: r.read_u16()?,
                valid: r.read_bool()?,
            },
            total_level: { r.read_u8()? },
            csm: r.read_bool()?,
        };
        let sustain_valid = |level: u16| level == 992 || (level <= 448 && level.is_multiple_of(32));
        ensure!(
            pipeline.selected.effective <= 63
                && sustain_valid(pipeline.selected.sustain)
                && sustain_valid(pipeline.prepared.sustain),
            "invalid FM selected rate"
        );
        ensure!(
            matches!(pipeline.prepared.increment, 0 | 1 | 2 | 4 | 8),
            "invalid FM prepared increment"
        );
        ensure!(
            pipeline.increment <= PHASE_MASK && pipeline.sample.phase <= PHASE_MASK,
            "invalid FM pipeline phase"
        );
        ensure!(
            pipeline.level_add <= 127 * 8 + 126
                && pipeline.level <= ENVELOPE_MAX
                && pipeline.sample.level <= ENVELOPE_MAX,
            "invalid FM pipeline level"
        );
        ensure!(
            pipeline.total_level == UNKNOWN_TOTAL_LEVEL
                || (pipeline.total_level < UNKNOWN_TOTAL_LEVEL
                    && pipeline.level_add >= u16::from(pipeline.total_level) * 8
                    && pipeline.level_add - u16::from(pipeline.total_level) * 8 <= 126),
            "invalid FM selected total level"
        );
        ensure!(
            !pipeline.csm || (pipeline.sampled && pipeline.total_level < UNKNOWN_TOTAL_LEVEL),
            "invalid FM CSM sample"
        );
        Ok(pipeline)
    }
}

#[cfg(test)]
mod tests;
