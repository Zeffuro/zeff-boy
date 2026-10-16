use super::*;
use crate::hardware::vdp::{VdpDmaError, VdpDmaRegisters, VdpDmaSource, VdpMemory};

impl State {
    pub(super) fn validate_external(&self, region: Region) -> Result<(), VdpPortError> {
        if self.read_phase.is_some()
            || self.cache.is_ready()
            || self.control.pending_word().is_some()
            || self.registers.pending().is_some()
        {
            return Err(VdpPortError::Dma(VdpDmaError::PendingInputs));
        }
        let mut dma =
            VdpDmaRegisters::from_registers(&self.registers).map_err(VdpPortError::Dma)?;
        let increment = self.registers.auto_increment().ok_or(VdpPortError::Dma(
            VdpDmaError::UnknownRegister(AUTO_INCREMENT_REGISTER),
        ))?;
        let mut command = self.command;
        dma.external_word(&mut command, increment, 0)
            .map_err(VdpPortError::Dma)?;
        self.interrupts
            .next_external_slot(self.now(), region, &self.registers)
            .map_err(VdpPortError::Schedule)?;
        Ok(())
    }

    pub(super) fn fetch_external(
        &mut self,
        source: Option<VdpDmaSource<'_>>,
    ) -> Result<(), VdpScheduleError> {
        let mut dma =
            VdpDmaRegisters::from_registers(&self.registers).map_err(VdpScheduleError::Dma)?;
        let source = source.ok_or(VdpScheduleError::Dma(VdpDmaError::MissingSource))?;
        let word = source
            .read(
                dma.external_source_address()
                    .map_err(VdpScheduleError::Dma)?,
            )
            .map_err(VdpScheduleError::Dma)?;
        let increment = self
            .registers
            .auto_increment()
            .expect("validated DMA increment");
        let entry = dma
            .external_word(&mut self.command, increment, word)
            .map_err(VdpScheduleError::Dma)?
            .expect("active external DMA");
        self.fifo.push(entry).expect("external FIFO vacancy");
        self.registers.publish_dma_counters(dma);
        if self.command.code & COMMAND_CD5 == 0 {
            self.dma = None;
        }
        Ok(())
    }

    pub(super) fn external_release(
        &self,
        region: Region,
        memory: &VdpMemory,
        source: VdpDmaSource<'_>,
    ) -> Result<Option<MasterTicks>, VdpScheduleError> {
        if self.dma != Some(dma::Phase::External) {
            return Ok(None);
        }
        let mut state = self.clone();
        while state.dma == Some(dma::Phase::External) {
            let fetch = if state.fifo.is_full() {
                None
            } else {
                Some(
                    state
                        .interrupts
                        .next_external_slot(state.now(), region, &state.registers)?,
                )
            };
            let consume = if state.fifo.is_empty() {
                None
            } else {
                Some(
                    state
                        .interrupts
                        .next_write_slot(state.now(), region, &state.registers)?,
                )
            };
            let next = fetch
                .into_iter()
                .chain(consume)
                .min()
                .expect("external DMA progress");
            state = state
                .prepare_chunk_to_from(next, region, false, memory, Some(source))?
                .0;
        }
        Ok(Some(state.now()))
    }
}
