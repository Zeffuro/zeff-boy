use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl TimerA {
    pub(crate) fn write_state(&self, writer: &mut Writer) {
        writer.write_u16(self.reload);
        {
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

    pub(crate) fn read_state(reader: &mut Reader<'_>) -> Result<Self> {
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
