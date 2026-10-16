use super::*;
use crate::hardware::vdp::{VdpDmaCopy, VdpDmaError, VdpDmaRegisters, VdpMemory, VdpWrite};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    External,
    FillWaiting,
    Fill(u16),
    Copy(Option<VdpDmaCopy>),
}

impl State {
    pub(super) fn reject_dma_data(&self) -> Result<(), VdpPortError> {
        if self.dma == Some(Phase::FillWaiting) {
            Ok(())
        } else {
            self.reject_dma_port()
        }
    }

    pub(super) fn arm_dma(&mut self, region: Region) -> Result<(), VdpPortError> {
        self.validate_transfer_mode(region)?;
        let mode = VdpDmaRegisters::from_registers(&self.registers)
            .map_err(VdpPortError::Dma)?
            .source_high()
            & DMA_MODE_MASK;
        if mode & DMA_SOURCE_LOCAL == 0 {
            self.validate_external(region)?;
            self.dma = Some(Phase::External);
            return Ok(());
        }
        if mode == DMA_COPY_MODE && !self.fifo.is_empty() {
            return Err(VdpPortError::Dma(VdpDmaError::PendingInputs));
        }
        let mut fifo = self.fifo.clone();
        while let Some((write, _)) = fifo.take_slot() {
            if write.command.code & !COMMAND_CD4 != VRAM_WRITE_CODE {
                return Err(VdpPortError::Dma(VdpDmaError::PendingInputs));
            }
        }
        self.validate_dma_phase(self.command, mode, region)?;
        self.dma = Some(if mode == DMA_FILL_MODE {
            Phase::FillWaiting
        } else {
            Phase::Copy(None)
        });
        Ok(())
    }

    pub(super) fn reject_dma_port(&self) -> Result<(), VdpPortError> {
        if self.dma.is_some() {
            Err(VdpPortError::Dma(VdpDmaError::Busy))
        } else {
            Ok(())
        }
    }

    pub(super) fn resume_dma(
        &mut self,
        command: ControlCommand,
        fill: Option<u16>,
        region: Region,
    ) -> Result<(), VdpPortError> {
        self.reject_dma_port()?;
        self.validate_transfer_mode(region)?;
        if !self.fifo.is_empty() {
            return Err(VdpPortError::Dma(VdpDmaError::PendingInputs));
        }
        let mode = if fill.is_some() {
            DMA_FILL_MODE
        } else {
            DMA_COPY_MODE
        };
        self.validate_dma_phase(command, mode, region)?;
        self.command = command;
        self.dma = Some(fill.map_or(Phase::Copy(None), Phase::Fill));
        Ok(())
    }

    fn validate_dma_phase(
        &self,
        command: ControlCommand,
        mode: u8,
        region: Region,
    ) -> Result<(), VdpPortError> {
        if self.read_phase.is_some()
            || self.cache.is_ready()
            || self.control.pending_word().is_some()
            || self.registers.pending().is_some()
        {
            return Err(VdpPortError::Dma(VdpDmaError::PendingInputs));
        }
        let dma = VdpDmaRegisters::from_registers(&self.registers).map_err(VdpPortError::Dma)?;
        let increment = self.registers.auto_increment().ok_or(VdpPortError::Dma(
            VdpDmaError::UnknownRegister(AUTO_INCREMENT_REGISTER),
        ))?;
        if dma
            .next_address(&command, increment, mode)
            .map_err(VdpPortError::Dma)?
            .is_none()
        {
            return Err(VdpPortError::Dma(VdpDmaError::UnsupportedCommand(command)));
        }
        self.interrupts
            .next_write_slot(self.now(), region, &self.registers)
            .map_err(VdpPortError::Schedule)?;
        Ok(())
    }

    pub(super) fn service_dma(
        &mut self,
        memory: &VdpMemory,
        writes: &data::WriteBatch,
    ) -> Result<Option<(VdpWrite, Option<bool>)>, VdpScheduleError> {
        let mut dma =
            VdpDmaRegisters::from_registers(&self.registers).map_err(VdpScheduleError::Dma)?;
        let increment = self
            .registers
            .auto_increment()
            .ok_or(VdpScheduleError::Dma(VdpDmaError::UnknownRegister(
                AUTO_INCREMENT_REGISTER,
            )))?;
        let cache_address = self.command.address ^ u32::from(WORD_ODD_ADDRESS_BIT);
        let write = match self.dma.unwrap() {
            Phase::External => unreachable!("external DMA feeds the FIFO"),
            Phase::FillWaiting => unreachable!("fill waits for FIFO retirement"),
            Phase::Fill(data) => dma.plan_fill(&mut self.command, increment, data),
            Phase::Copy(Some(copy)) => {
                self.dma = Some(Phase::Copy(None));
                dma.plan_copy_write(&mut self.command, increment, copy)
                    .map(Some)
            }
            Phase::Copy(None) => {
                let source = dma.source() ^ WORD_ODD_ADDRESS_BIT;
                let word = memory
                    .read_after_writes(
                        ControlCommand {
                            address: u32::from(source & !WORD_ODD_ADDRESS_BIT),
                            code: VRAM_READ_CODE,
                        },
                        None,
                        writes.iter().flatten().copied(),
                    )
                    .map_err(VdpScheduleError::Read)?;
                let byte = word.to_be_bytes()[usize::from(source & WORD_ODD_ADDRESS_BIT)];
                let copy = dma
                    .read_copy_from_data(&self.command, increment, byte)
                    .map_err(VdpScheduleError::Dma)?;
                self.dma = Some(Phase::Copy(copy));
                return Ok(None);
            }
        }
        .map_err(VdpScheduleError::Dma)?;
        self.registers.publish_dma_counters(dma);
        if self.command.code & COMMAND_CD5 == 0 {
            self.dma = None;
        }
        Ok(write.map(|write| {
            self.sprites.observe_dma_byte(
                self.registers.registers()[usize::from(SPRITE_TABLE_REGISTER)],
                cache_address,
                self.registers.horizontal_mode().unwrap(),
            );
            (
                VdpWrite {
                    command: ControlCommand {
                        address: u32::from(write.address & !WORD_ODD_ADDRESS_BIT),
                        code: VRAM_WRITE_CODE,
                    },
                    data: u16::from_be_bytes([write.data; 2]),
                },
                Some(write.address & WORD_ODD_ADDRESS_BIT == 0),
            )
        }))
    }
}
