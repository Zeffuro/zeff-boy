use super::super::super::chip_constants::{FM_CLOCK_DIVIDER, TIMER_B_DIVIDER};
use super::super::LegacyTimer;
use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl TimerB {
    pub(crate) fn write_state(&self, writer: &mut Writer, version: u32) {
        if version < crate::save_state::STATE_VERSION_WITH_TIMER_B_PHASE {
            let scans = (TIMER_B_STEPS - u16::from(self.counter))
                * u16::from(TIMER_B_SUBCOUNTER_STEPS)
                - u16::from(self.subcounter);
            writer.write_u16(u16::from(self.reload));
            writer.write_u64(u64::from(scans) * FM_CLOCK_DIVIDER);
            writer.write_bool(self.load_lock);
        } else {
            writer.write_u8(self.reload);
            writer.write_u8(self.counter);
            writer.write_u8(self.subcounter);
            writer.write_u8(
                u8::from(self.load_lock)
                    | u8::from(self.reload_pending) << 1
                    | u8::from(self.overflow) << 2
                    | u8::from(self.clear) << 3,
            );
        }
    }

    pub(crate) fn read_state(reader: &mut Reader<'_>, version: u32) -> Result<Self> {
        if version < crate::save_state::STATE_VERSION_WITH_TIMER_B_PHASE {
            let old = LegacyTimer::read_state(reader, TIMER_B_STEPS, TIMER_B_DIVIDER)?;
            let scans = old.remaining.div_ceil(FM_CLOCK_DIVIDER) as u16;
            let steps = scans.div_ceil(u16::from(TIMER_B_SUBCOUNTER_STEPS));
            return Ok(Self {
                reload: old.reload as u8,
                counter: if scans == 0 {
                    0
                } else {
                    (TIMER_B_STEPS - steps) as u8
                },
                subcounter: (steps * u16::from(TIMER_B_SUBCOUNTER_STEPS) - scans) as u8,
                load_lock: old.running,
                ..Self::default()
            });
        }
        let reload = reader.read_u8()?;
        let counter = reader.read_u8()?;
        let subcounter = reader.read_u8()?;
        let bits = reader.read_u8()?;
        ensure!(
            subcounter < TIMER_B_SUBCOUNTER_STEPS && bits < 16 && (bits & 4 == 0 || counter == 0),
            "invalid FM timer B latches"
        );
        Ok(Self {
            reload,
            counter,
            subcounter,
            load_lock: bits & 1 != 0,
            reload_pending: bits & 2 != 0,
            overflow: bits & 4 != 0,
            clear: bits & 8 != 0,
        })
    }
}
