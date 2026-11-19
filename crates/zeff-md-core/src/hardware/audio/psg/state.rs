use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl Psg {
    pub(crate) fn write_state(&self, w: &mut Writer) {
        let s = self.state;
        for value in s.periods {
            w.write_u16(value);
        }
        w.write_bytes(&s.volumes);
        for value in s.counters {
            w.write_u16(value);
        }
        for value in s.outputs {
            w.write_bool(value);
        }
        w.write_u8(s.latched_register);
        w.write_u8(s.noise_control);
        w.write_u16(s.noise_counter);
        w.write_bool(s.noise_clock);
        w.write_u16(s.noise_lfsr);
    }

    pub(crate) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        let mut chip = Self::default();
        let s = &mut chip.state;
        for value in &mut s.periods {
            *value = r.read_u16()?;
            ensure!(*value <= 0x3ff, "invalid PSG period");
        }
        r.read_exact(&mut s.volumes)?;
        ensure!(
            s.volumes.iter().all(|v| *v <= PSG_DATA_MASK),
            "invalid PSG volume"
        );
        for value in &mut s.counters {
            *value = r.read_u16()?;
            ensure!((1..=0x3ff).contains(value), "invalid PSG counter");
        }
        for value in &mut s.outputs {
            *value = r.read_bool()?;
        }
        s.latched_register = r.read_u8()?;
        s.noise_control = r.read_u8()?;
        s.noise_counter = r.read_u16()?;
        s.noise_clock = r.read_bool()?;
        s.noise_lfsr = r.read_u16()?;
        ensure!(s.latched_register <= PSG_REGISTER_MASK, "invalid PSG latch");
        ensure!(s.noise_control <= PSG_NOISE_MASK, "invalid PSG noise mode");
        ensure!(
            (1..=PSG_NOISE_PERIOD << 2).contains(&s.noise_counter),
            "invalid PSG noise counter"
        );
        ensure!(s.noise_lfsr != 0, "invalid PSG noise shift register");
        Ok(chip)
    }
}

#[cfg(test)]
mod tests;
