use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl TimerA {
    pub(crate) fn write_state(&self, writer: &mut Writer, version: u32) {
        writer.write_u16(self.reload);
        if version < crate::save_state::STATE_VERSION_WITH_CSM_TIMER {
            writer.write_u64(u64::from(TIMER_A_STEPS - self.counter) * FM_CLOCK_DIVIDER);
            writer.write_bool(self.load_lock);
        } else {
            writer.write_u16(self.counter);
            writer.write_u8(
                u8::from(self.load_lock)
                    | u8::from(self.reload_pending) << 1
                    | u8::from(self.overflow) << 2
                    | u8::from(self.clear) << 3
                    | u8::from(self.csm) << 4,
            );
        }
    }

    pub(crate) fn read_state(reader: &mut Reader<'_>, version: u32) -> Result<Self> {
        if version < crate::save_state::STATE_VERSION_WITH_CSM_TIMER {
            let old = LegacyTimer::read_state(reader, TIMER_A_STEPS, FM_CLOCK_DIVIDER)?;
            let scans = old.remaining.div_ceil(FM_CLOCK_DIVIDER) as u16;
            return Ok(Self {
                reload: old.reload,
                counter: if scans == 0 { 0 } else { TIMER_A_STEPS - scans },
                load_lock: old.running,
                ..Self::default()
            });
        }
        let reload = reader.read_u16()?;
        let counter = reader.read_u16()?;
        let bits = reader.read_u8()?;
        ensure!(
            reload < TIMER_A_STEPS && counter < TIMER_A_STEPS,
            "invalid FM timer A counter"
        );
        ensure!(
            bits < 32 && (bits & 4 == 0 || counter == 0),
            "invalid FM timer A latches"
        );
        Ok(Self {
            reload,
            counter,
            load_lock: bits & 1 != 0,
            reload_pending: bits & 2 != 0,
            overflow: bits & 4 != 0,
            clear: bits & 8 != 0,
            csm: bits & 16 != 0,
        })
    }
}
