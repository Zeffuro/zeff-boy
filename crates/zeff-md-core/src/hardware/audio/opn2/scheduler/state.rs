use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl Command {
    fn write_state(self, w: &mut Writer) {
        w.write_u8(self.channel);
        w.write_u8(self.mask);
    }

    fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        let command = Self {
            channel: r.read_u8()?,
            mask: r.read_u8()?,
        };
        ensure!(
            (command.channel < CHANNELS as u8 || command.channel == 255) && command.mask <= 15,
            "invalid FM key command"
        );
        Ok(command)
    }
}

impl Scheduler {
    pub(crate) fn migrated(clock: EnvelopeClock, phase: u64, output: [i32; CHANNELS]) -> Self {
        Self {
            step: 72 * u32::from(clock.counter)
                + 24 * u32::from(clock.phase)
                + (phase / SLOT_CLOCKS) as u32,
            command: Command {
                channel: 255,
                mask: 0,
            },
            pending: None,
            frequency: (0, 0),
            frequency_valid: false,
            output,
            ready: 0,
            accumulator: [0; CHANNELS],
            carriers: [0; CHANNELS],
        }
    }

    #[cfg(test)]
    pub(crate) fn write_state(&self, w: &mut Writer) {
        self.write_state_version(w, crate::save_state::STATE_VERSION);
    }

    pub(crate) fn write_state_version(&self, w: &mut Writer, version: u32) {
        w.write_u32(self.step);
        self.command.write_state(w);
        w.write_bool(self.pending.is_some());
        if let Some(command) = self.pending {
            command.write_state(w);
        }
        w.write_u16(self.frequency.0);
        w.write_u8(self.frequency.1);
        w.write_bool(self.frequency_valid);
        for value in self.output {
            w.write_u32(value as u32);
        }
        w.write_u8(
            if version >= crate::save_state::STATE_VERSION_WITH_FM_PUBLICATION {
                self.ready
            } else {
                0
            },
        );
        if version >= crate::save_state::STATE_VERSION_WITH_FM_PUBLICATION {
            for value in self.accumulator {
                w.write_u32(value as u32);
            }
            w.write_bytes(&self.carriers);
        }
    }

    #[cfg(test)]
    pub(crate) fn read_state(r: &mut Reader<'_>) -> Result<Self> {
        Self::read_state_version(r, crate::save_state::STATE_VERSION)
    }

    pub(crate) fn read_state_version(r: &mut Reader<'_>, version: u32) -> Result<Self> {
        let step = r.read_u32()?;
        let command = Command::read_state(r)?;
        let pending = if r.read_bool()? {
            Some(Command::read_state(r)?)
        } else {
            None
        };
        let frequency = (r.read_u16()?, r.read_u8()?);
        let frequency_valid = r.read_bool()?;
        let mut output = [0; CHANNELS];
        let peak = 4 * constants::OPERATOR_PEAK;
        for value in &mut output {
            *value = r.read_u32()? as i32;
            ensure!((-peak..=peak).contains(value), "invalid pending FM output");
        }
        let ready = r.read_u8()?;
        ensure!(ready <= 63, "invalid FM output readiness");
        ensure!(step < CLOCK_END, "invalid FM slot clock");
        ensure!(
            frequency.0 <= 0x7ff && frequency.1 <= 7,
            "invalid FM slot frequency"
        );
        let mut result = Self {
            step,
            command,
            pending,
            frequency,
            frequency_valid,
            output,
            ready,
            accumulator: [0; CHANNELS],
            carriers: [0; CHANNELS],
        };
        if version >= crate::save_state::STATE_VERSION_WITH_FM_PUBLICATION {
            for value in &mut result.accumulator {
                *value = r.read_u32()? as i32;
                ensure!(
                    (dac::OUTPUT_MIN..=dac::OUTPUT_MAX).contains(value),
                    "invalid FM accumulator"
                );
            }
            r.read_exact(&mut result.carriers)?;
            ensure!(
                result.carriers.iter().all(|mask| *mask <= ALL_OPERATORS),
                "invalid FM carrier validity"
            );
            for (channel, value) in result.output.iter().enumerate() {
                ensure!(
                    result.ready & (1 << channel) == 0
                        || ((dac::OUTPUT_MIN * dac::OUTPUT_SCALE
                            ..=dac::OUTPUT_MAX * dac::OUTPUT_SCALE)
                            .contains(value)
                            && value % dac::OUTPUT_SCALE == 0),
                    "invalid completed FM output"
                );
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
