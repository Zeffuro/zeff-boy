use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};

impl Audio {
    pub(crate) fn write_state(&self, w: &mut Writer) -> Result<()> {
        assert!(self.deferred.is_none(), "canonical audio state");
        self.psg.write_state(w);
        self.opn2.write_state(w);
        w.write_u64(self.now);
        w.write_u64(self.chip_phase);
        w.write_u64(self.opn2_phase);
        Ok(())
    }

    pub(crate) fn read_state(&mut self, r: &mut Reader<'_>, region: Region) -> Result<()> {
        let psg = Psg::read_state(r)?;
        let opn2 = Opn2::read_state(r)?;
        let now = r.read_u64()?;
        let chip_phase = r.read_u64()?;
        let opn2_phase = r.read_u64()?;
        ensure!(chip_phase < PSG_CLOCK_TICKS, "invalid PSG clock phase");
        ensure!(opn2_phase < OPN2_CLOCK_TICKS, "invalid OPN2 clock phase");
        ensure!(
            self.master_hz == region.master_hz(),
            "audio region mismatch"
        );
        self.psg = psg;
        self.opn2 = opn2;
        self.now = now;
        self.chip_phase = chip_phase;
        self.opn2_phase = opn2_phase;
        self.restart_output();
        Ok(())
    }

    pub(crate) fn state_now(&self) -> u64 {
        self.now
    }
}

#[cfg(test)]
mod tests;
