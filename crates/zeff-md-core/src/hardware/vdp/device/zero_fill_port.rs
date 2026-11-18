use super::*;
use crate::hardware::vdp::{VdpDmaError, VdpDmaRegisters, VdpWriteFifo};

pub(crate) struct ZeroFillWrite {
    end: MasterTicks,
    region: Region,
    command: ControlCommand,
    registers: [Option<u8>; REGISTER_COUNT as usize],
    fifo: VdpWriteFifo,
}

impl State {
    fn zero_fill_port_ready(&self) -> bool {
        self.dma == Some(dma::Phase::Fill(0))
            && self.command.code == 0x21
            && self.fifo.is_empty()
            && self.fifo.known_zero_history()
            && self.registers.auto_increment().is_some()
            && VdpDmaRegisters::from_registers(&self.registers)
                .is_ok_and(|registers| registers.source_high() & DMA_MODE_MASK == DMA_FILL_MODE)
            && self.registers.pending().is_none()
            && self.control.pending_word().is_none()
            && self.read_phase.is_none()
            && !self.cache.is_ready()
            && self.interrupts.blank_history().is_none()
    }
}

impl Vdp {
    pub(crate) fn prepare_zero_fill_write(
        &self,
        end: MasterTicks,
        region: Region,
    ) -> Option<ZeroFillWrite> {
        if self.owned.is_some() {
            return None;
        }
        let state = &self.state;
        if end <= self.now()
            || !state.zero_fill_port_ready()
            || state.validate_transfer_mode(region).is_err()
            || state
                .interrupts
                .is_machine_write_slot(self.now(), region, &state.registers)
                .ok()?
            || state
                .interrupts
                .next_write_slot(self.now(), region, &state.registers)
                .ok()?
                <= end
        {
            return None;
        }
        Some(ZeroFillWrite {
            end,
            region,
            command: state.command,
            registers: *state.registers.registers(),
            fifo: state.fifo.clone(),
        })
    }

    pub(crate) fn validate_zero_fill_write(
        &self,
        receipt: &ZeroFillWrite,
        region: Region,
    ) -> Result<(), VdpPortError> {
        self.require_legacy_port()?;
        if self.now() != receipt.end
            || region != receipt.region
            || !self.state.zero_fill_port_ready()
            || self.state.command != receipt.command
            || self.state.registers.registers() != &receipt.registers
            || self.state.fifo != receipt.fifo
        {
            return Err(VdpPortError::Dma(VdpDmaError::Busy));
        }
        self.state.validate_transfer_mode(region)
    }

    pub(crate) fn commit_zero_fill_write(
        &mut self,
        word: u16,
        receipt: ZeroFillWrite,
        region: Region,
    ) -> Result<(), VdpPortError> {
        self.validate_zero_fill_write(&receipt, region)?;
        if word != 0 {
            return Err(VdpPortError::Dma(VdpDmaError::Busy));
        }
        self.state.enqueue_data(0, region, &self.memory)?;
        self.invalidate_interval();
        Ok(())
    }
}
