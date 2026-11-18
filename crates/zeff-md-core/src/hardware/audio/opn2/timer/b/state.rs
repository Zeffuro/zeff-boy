use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl TimerB {
    pub(crate) fn write_state(&self, writer: &mut Writer) {
        {
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

    pub(crate) fn read_state(reader: &mut Reader<'_>) -> Result<Self> {
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
