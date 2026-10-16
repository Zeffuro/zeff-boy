use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl Opn2 {
    pub(crate) fn write_state(&self, w: &mut Writer) {
        for channel in self.channels {
            channel.write_state(w);
        }
        self.write_tail(w, crate::save_state::STATE_VERSION);
    }

    #[cfg(test)]
    pub(crate) fn write_state_version(&self, w: &mut Writer, version: u32) {
        for channel in self.channels {
            channel.write_state_version(w, version);
        }
        self.write_tail(w, version);
    }

    fn write_tail(&self, w: &mut Writer, version: u32) {
        w.write_u16(self.address);
        w.write_u8(self.frequency_high);
        w.write_u8(self.special_high);
        for (fnum, block) in self.special {
            w.write_u16(fnum);
            w.write_u8(block);
        }
        self.timer_a.write_state(w, version);
        self.timer_b.write_state(w, version);
        w.write_u8(self.mode);
        w.write_u8(self.flags);
        w.write_u64(self.busy);
        w.write_u64(self.phase);
        w.write_u8(self.dac);
        w.write_bool(self.dac_enabled);
        for value in self.output {
            w.write_u32(value as u32);
        }
        if version >= crate::save_state::STATE_VERSION_WITH_FM_PIPELINE {
            w.write_u8(self.lfo.control);
            w.write_u8(self.lfo.counter);
            w.write_u8(self.lfo.quotient);
        }
        if version >= crate::save_state::STATE_VERSION_WITH_ENVELOPE_CLOCK {
            w.write_u16(self.envelope_clock.counter);
            w.write_u8(self.envelope_clock.phase);
        }
        if version >= crate::save_state::STATE_VERSION_WITH_SLOT_SCHEDULER {
            self.scheduler.write_state_version(w, version);
        }
        if version >= crate::save_state::STATE_VERSION_WITH_LFO_LATCH {
            w.write_u8(self.lfo.sampled);
        }
        if version >= crate::save_state::STATE_VERSION_WITH_FM_PUBLICATION {
            w.write_bytes(&self.output_pan);
        }
    }

    #[cfg(test)]
    pub(crate) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        Self::read_state_version(r, crate::save_state::STATE_VERSION)
    }

    pub(crate) fn read_state_version(r: &mut Reader<'_>, version: u32) -> Result<Self> {
        let mut chip = Self::default();
        for channel in &mut chip.channels {
            *channel = Channel::read_state(r, version)?;
            ensure!(
                version >= crate::save_state::STATE_VERSION_WITH_FM_PIPELINE
                    || channel.pan() & !0xc0 == 0,
                "invalid legacy FM channel parameters"
            );
        }
        ensure!(
            (0..scheduler::SLOTS).all(|slot| slot % CHANNELS == 2 || !chip.slot(slot).pipeline.csm),
            "invalid FM CSM channel"
        );
        chip.address = r.read_u16()?;
        chip.frequency_high = r.read_u8()?;
        chip.special_high = r.read_u8()?;
        for frequency in &mut chip.special {
            *frequency = (r.read_u16()?, r.read_u8()?);
            ensure!(
                frequency.0 <= 0x7ff && frequency.1 <= 7,
                "invalid special FM frequency"
            );
        }
        chip.timer_a = TimerA::read_state(r, version)?;
        chip.timer_b = TimerB::read_state(r, version)?;
        chip.mode = r.read_u8()?;
        chip.flags = r.read_u8()?;
        chip.busy = r.read_u64()?;
        chip.phase = r.read_u64()?;
        chip.dac = r.read_u8()?;
        chip.dac_enabled = r.read_bool()?;
        let peak = 4 * constants::OPERATOR_PEAK;
        for value in &mut chip.output {
            *value = r.read_u32()? as i32;
            ensure!((-peak..=peak).contains(value), "invalid FM output history");
        }
        ensure!(
            chip.address <= 0x1ff && chip.frequency_high <= 0x3f && chip.special_high <= 0x3f,
            "invalid FM register latch"
        );
        ensure!(
            chip.flags <= 3 && chip.mode & 0x30 == 0,
            "invalid FM timer mode"
        );
        ensure!(
            chip.busy <= BUSY_CLOCKS && chip.phase < FM_CLOCK_DIVIDER,
            "invalid FM clock phase"
        );
        if version >= crate::save_state::STATE_VERSION_WITH_FM_PIPELINE {
            chip.lfo.control = r.read_u8()?;
            chip.lfo.counter = r.read_u8()?;
            chip.lfo.quotient = r.read_u8()?;
            ensure!(
                chip.lfo.control <= 15 && chip.lfo.counter <= 127 && chip.lfo.quotient <= 127,
                "invalid FM LFO state"
            );
        }
        if version >= crate::save_state::STATE_VERSION_WITH_ENVELOPE_CLOCK {
            chip.envelope_clock.counter = r.read_u16()?;
            chip.envelope_clock.phase = r.read_u8()?;
            let legacy = version < crate::save_state::STATE_VERSION_WITH_ENVELOPE_CARRY;
            let maximum = if legacy {
                (1 << 14) - 1
            } else {
                constants::ENVELOPE_COUNTER_MAX
            };
            ensure!(
                chip.envelope_clock.counter <= maximum
                    && chip.envelope_clock.phase < constants::ENVELOPE_DIVIDER,
                "invalid FM envelope clock"
            );
            if legacy && chip.envelope_clock.counter != 0 {
                chip.envelope_clock.counter =
                    (chip.envelope_clock.counter - 1) % constants::ENVELOPE_COUNTER_MAX + 1;
            }
        }
        chip.scheduler = if version >= crate::save_state::STATE_VERSION_WITH_SLOT_SCHEDULER {
            Scheduler::read_state_version(r, version)?
        } else {
            Scheduler::migrated(chip.envelope_clock, chip.phase, chip.output)
        };
        if version < crate::save_state::STATE_VERSION_WITH_FM_PUBLICATION {
            chip.scheduler.ready = 0;
            chip.scheduler.output = chip.output;
        }
        chip.lfo.sampled = if version >= crate::save_state::STATE_VERSION_WITH_LFO_LATCH {
            r.read_u8()?
        } else {
            chip.lfo.counter
        };
        ensure!(chip.lfo.sampled <= 127, "invalid FM LFO latch");
        if version >= crate::save_state::STATE_VERSION_WITH_FM_PUBLICATION {
            r.read_exact(&mut chip.output_pan)?;
            ensure!(
                chip.output_pan
                    .iter()
                    .all(|pan| pan & !(PAN_LEFT | PAN_RIGHT) == 0),
                "invalid held FM pan"
            );
        } else {
            chip.output_pan = chip
                .channels
                .map(|channel| channel.pan() & (PAN_LEFT | PAN_RIGHT));
        }
        ensure!(
            chip.phase / scheduler::SLOT_CLOCKS == u64::from(chip.scheduler.step % 24),
            "inconsistent FM slot cursor"
        );
        Ok(chip)
    }
}

#[cfg(test)]
mod tests;
