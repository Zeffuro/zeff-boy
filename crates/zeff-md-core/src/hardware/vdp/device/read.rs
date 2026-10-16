use super::*;
use crate::hardware::vdp::{VdpMemory, VdpReadError};

impl State {
    fn validate_read(&self, region: Region) -> Result<(), VdpPortError> {
        self.validate_transfer_mode(region)?;
        if self
            .registers
            .pending()
            .is_some_and(|write| write.index == AUTO_INCREMENT_REGISTER)
        {
            return Err(VdpPortError::Read(VdpReadError::PendingIncrement));
        }
        let command = self.command;
        let increment = self
            .registers
            .auto_increment()
            .ok_or(VdpPortError::Read(VdpReadError::UnknownIncrement))?;
        if command.address > COMMAND_ADDRESS_MASK
            || !matches!(
                command.code,
                VRAM_READ_CODE | CRAM_READ_CODE | VSRAM_READ_CODE
            )
            || (command.code == VRAM_READ_CODE
                && command.address & u32::from(WORD_ODD_ADDRESS_BIT) != 0)
            || (command.code == VSRAM_READ_CODE
                && (command.address as usize & usize::from(COLOR_WORD_ADDRESS_MASK)) >> 1
                    >= VSRAM_WORDS)
        {
            return Err(VdpPortError::Read(VdpReadError::UnsupportedCommand(
                command,
            )));
        }
        if command.code == VRAM_READ_CODE && increment & WORD_ODD_ADDRESS_BIT as u8 != 0 {
            return Err(VdpPortError::Read(VdpReadError::UnsupportedIncrement(
                increment,
            )));
        }
        Ok(())
    }

    pub(super) fn prefetch_end(
        &self,
        region: Region,
        memory: &VdpMemory,
    ) -> Result<MasterTicks, VdpPortError> {
        self.validate_read(region)?;
        let mut fifo = self.fifo.clone();
        let mut slots = if self.command.code == VRAM_READ_CODE && self.read_phase != Some(true) {
            VRAM_READ_SLOTS
        } else {
            COLOR_READ_SLOTS
        };
        let mut end = self.now();
        while !fifo.is_empty() || slots != 0 {
            end = self
                .interrupts
                .next_write_slot(end, region, &self.registers)
                .map_err(VdpPortError::Schedule)?;
            if !fifo.is_empty() {
                fifo.take_slot();
            } else {
                slots -= 1;
            }
        }
        self.prepare_to(end, region, false, memory)
            .map_err(VdpPortError::Schedule)?;
        Ok(end)
    }

    pub(super) fn service_read(
        &mut self,
        memory: &VdpMemory,
        writes: &data::WriteBatch,
    ) -> Result<(), VdpReadError> {
        if self.command.code == VRAM_READ_CODE && self.read_phase == Some(false) {
            self.read_phase = Some(true);
            return Ok(());
        }
        let word = memory.read_after_writes(
            self.command,
            self.fifo.next_slot_data(),
            writes.iter().flatten().copied(),
        )?;
        self.cache.complete_word(
            word,
            &mut self.command,
            self.registers
                .auto_increment()
                .ok_or(VdpReadError::UnknownIncrement)?,
        );
        self.read_phase = None;
        Ok(())
    }

    pub(super) fn data_read_wait(
        &self,
        region: Region,
        memory: &VdpMemory,
    ) -> Result<Option<MasterTicks>, VdpPortError> {
        self.reject_dma_port()?;
        self.validate_transfer_mode(region)?;
        if !matches!(
            self.command.code,
            VRAM_READ_CODE | CRAM_READ_CODE | VSRAM_READ_CODE
        ) {
            return Err(VdpPortError::Read(VdpReadError::UnsupportedCommand(
                self.command,
            )));
        }
        if self.cache.is_ready() {
            return Ok(None);
        }
        if self.read_phase.is_none() {
            return Err(VdpPortError::Read(VdpReadError::Empty));
        }
        self.prefetch_end(region, memory).map(Some)
    }

    pub(super) fn read_data(
        &mut self,
        region: Region,
        memory: &VdpMemory,
    ) -> Result<u16, VdpPortError> {
        self.reject_dma_port()?;
        self.validate_transfer_mode(region)?;
        let mut candidate = self.clone();
        let word = candidate.cache.read().map_err(VdpPortError::Read)?;
        candidate.read_phase = Some(false);
        candidate.prefetch_end(region, memory)?;
        candidate.control.cancel_pending();
        *self = candidate;
        Ok(word)
    }
}
