use super::constants::*;
use super::{ControlCommand, VdpMemory, VdpRegisterPipeline, VdpRegisterWrite, VdpWrite};
mod state;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VdpDmaError {
    UnknownRegister(u8),
    PendingRegister(VdpRegisterWrite),
    UnsupportedMode(u8),
    MissingSource,
    UnsupportedSource(u32),
    WrongOperation(u8),
    StaleCopy,
    BatchFull,
    Busy,
    PendingInputs,
    UnsupportedCommand(ControlCommand),
    UnsupportedIncrement { address: u32, increment: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpDmaWrite {
    pub address: u16,
    pub data: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpDmaCopy {
    registers: VdpDmaRegisters,
    command: ControlCommand,
    increment: u8,
    data: u8,
}

impl VdpDmaCopy {
    pub fn data(&self) -> u8 {
        self.data
    }
}

/// Settled DMA register image; port triggers, FIFO arbitration and timing are separate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpDmaRegisters {
    length: u16,
    source: u16,
    source_high: u8,
}

impl VdpDmaRegisters {
    pub fn from_registers(registers: &VdpRegisterPipeline) -> Result<Self, VdpDmaError> {
        if let Some(write) = registers.pending()
            && (DMA_LENGTH_LOW_REGISTER..=DMA_SOURCE_REGISTER).contains(&write.index)
        {
            return Err(VdpDmaError::PendingRegister(write));
        }
        let value = |index| {
            registers.registers()[usize::from(index)].ok_or(VdpDmaError::UnknownRegister(index))
        };
        let length = u16::from_le_bytes([
            value(DMA_LENGTH_LOW_REGISTER)?,
            value(DMA_LENGTH_HIGH_REGISTER)?,
        ]);
        let source = u16::from_le_bytes([
            value(DMA_SOURCE_LOW_REGISTER)?,
            value(DMA_SOURCE_HIGH_REGISTER)?,
        ]);
        let source_high = value(DMA_SOURCE_REGISTER)?;
        Ok(Self {
            length,
            source,
            source_high,
        })
    }

    pub fn length(&self) -> u16 {
        self.length
    }

    pub fn source(&self) -> u16 {
        self.source
    }

    pub fn source_high(&self) -> u8 {
        self.source_high
    }

    pub fn external_source_address(&self) -> Result<u32, VdpDmaError> {
        if self.source_high & DMA_SOURCE_LOCAL != 0 {
            return Err(VdpDmaError::WrongOperation(self.source_high));
        }
        Ok(
            (u32::from(self.source_high & DMA_EXTERNAL_SOURCE_MASK) << DMA_EXTERNAL_SOURCE_SHIFT)
                | (u32::from(self.source) << 1),
        )
    }

    pub fn external_word(
        &mut self,
        command: &mut ControlCommand,
        increment: u8,
        data: u16,
    ) -> Result<Option<VdpWrite>, VdpDmaError> {
        self.external_source_address()?;
        if !matches!(
            command.code & !(COMMAND_CD4 | COMMAND_CD5),
            VRAM_WRITE_CODE | CRAM_WRITE_CODE | VSRAM_WRITE_CODE
        ) || command.address > COMMAND_ADDRESS_MASK
        {
            return Err(VdpDmaError::UnsupportedCommand(*command));
        }
        if command.code & COMMAND_CD5 == 0 {
            return Ok(None);
        }
        let write = VdpWrite {
            command: ControlCommand {
                code: command.code & !COMMAND_CD5,
                ..*command
            },
            data,
        };
        let next = command.next_address(increment);
        self.complete_byte(command, next);
        Ok(Some(write))
    }

    /// One fill byte after the triggering normal word has completed.
    pub fn fill_byte(
        &mut self,
        command: &mut ControlCommand,
        increment: u8,
        data: u16,
        memory: &mut VdpMemory,
    ) -> Result<Option<VdpDmaWrite>, VdpDmaError> {
        let write = self.plan_fill(command, increment, data)?;
        if let Some(write) = write {
            memory.write_dma_byte(write.address, write.data);
        }
        Ok(write)
    }

    pub(super) fn plan_fill(
        &mut self,
        command: &mut ControlCommand,
        increment: u8,
        data: u16,
    ) -> Result<Option<VdpDmaWrite>, VdpDmaError> {
        let Some(next) = self.next_address(command, increment, DMA_FILL_MODE)? else {
            return Ok(None);
        };
        let write = VdpDmaWrite {
            address: command.address as u16 ^ WORD_ODD_ADDRESS_BIT,
            data: data.to_be_bytes()[0],
        };
        self.complete_byte(command, next);
        Ok(Some(write))
    }

    /// Completed read/write pair; callers qualify its two access opportunities.
    pub fn copy_byte(
        &mut self,
        command: &mut ControlCommand,
        increment: u8,
        memory: &mut VdpMemory,
    ) -> Result<Option<VdpDmaWrite>, VdpDmaError> {
        let Some(copy) = self.read_copy_byte(command, increment, memory)? else {
            return Ok(None);
        };
        self.write_copy_byte(command, increment, copy, memory)
            .map(Some)
    }

    pub fn read_copy_byte(
        &self,
        command: &ControlCommand,
        increment: u8,
        memory: &VdpMemory,
    ) -> Result<Option<VdpDmaCopy>, VdpDmaError> {
        self.read_copy_from_data(
            command,
            increment,
            memory.vram()[usize::from(self.source ^ WORD_ODD_ADDRESS_BIT)],
        )
    }

    pub(super) fn read_copy_from_data(
        &self,
        command: &ControlCommand,
        increment: u8,
        data: u8,
    ) -> Result<Option<VdpDmaCopy>, VdpDmaError> {
        if self
            .next_address(command, increment, DMA_COPY_MODE)?
            .is_none()
        {
            return Ok(None);
        }
        Ok(Some(VdpDmaCopy {
            registers: *self,
            command: *command,
            increment,
            data,
        }))
    }

    /// Requires matching register/address inputs; preserves data latched by the read.
    pub fn write_copy_byte(
        &mut self,
        command: &mut ControlCommand,
        increment: u8,
        copy: VdpDmaCopy,
        memory: &mut VdpMemory,
    ) -> Result<VdpDmaWrite, VdpDmaError> {
        let write = self.plan_copy_write(command, increment, copy)?;
        memory.write_dma_byte(write.address, write.data);
        Ok(write)
    }

    pub(super) fn plan_copy_write(
        &mut self,
        command: &mut ControlCommand,
        increment: u8,
        copy: VdpDmaCopy,
    ) -> Result<VdpDmaWrite, VdpDmaError> {
        if *self != copy.registers || *command != copy.command || increment != copy.increment {
            return Err(VdpDmaError::StaleCopy);
        }
        let next = self
            .next_address(command, increment, DMA_COPY_MODE)?
            .ok_or(VdpDmaError::StaleCopy)?;
        let write = VdpDmaWrite {
            address: command.address as u16 ^ WORD_ODD_ADDRESS_BIT,
            data: copy.data,
        };
        self.complete_byte(command, next);
        Ok(write)
    }

    /// Atomic completed bytes, bounded by eight distinct VRAM destinations.
    pub fn fill_bytes(
        &mut self,
        command: &mut ControlCommand,
        increment: u8,
        data: u16,
        count: u32,
        memory: &mut VdpMemory,
    ) -> Result<u32, VdpDmaError> {
        self.transfer_bytes(
            command,
            increment,
            Some(data.to_be_bytes()[0]),
            count,
            memory,
        )
    }

    pub fn copy_bytes(
        &mut self,
        command: &mut ControlCommand,
        increment: u8,
        count: u32,
        memory: &mut VdpMemory,
    ) -> Result<u32, VdpDmaError> {
        self.transfer_bytes(command, increment, None, count, memory)
    }

    fn transfer_bytes(
        &mut self,
        command: &mut ControlCommand,
        increment: u8,
        fill: Option<u8>,
        count: u32,
        memory: &mut VdpMemory,
    ) -> Result<u32, VdpDmaError> {
        let mut registers = *self;
        let mut candidate = *command;
        let mode = if fill.is_some() {
            DMA_FILL_MODE
        } else {
            DMA_COPY_MODE
        };
        let mut writes: [Option<VdpDmaWrite>; DMA_WRITE_BATCH_CAPACITY] =
            [None; DMA_WRITE_BATCH_CAPACITY];
        let mut completed = 0;
        while completed < count {
            let Some(next) = registers.next_address(&candidate, increment, mode)? else {
                break;
            };
            let source = registers.source ^ WORD_ODD_ADDRESS_BIT;
            let data = fill.unwrap_or_else(|| {
                writes
                    .iter()
                    .flatten()
                    .find(|write| write.address == source)
                    .map_or(memory.vram()[usize::from(source)], |write| write.data)
            });
            let address = candidate.address as u16 ^ WORD_ODD_ADDRESS_BIT;
            let index = writes
                .iter()
                .position(|write| write.is_some_and(|write| write.address == address))
                .or_else(|| writes.iter().position(Option::is_none))
                .ok_or(VdpDmaError::BatchFull)?;
            writes[index] = Some(VdpDmaWrite { address, data });
            registers.complete_byte(&mut candidate, next);
            completed += 1;
        }
        for write in writes.into_iter().flatten() {
            memory.write_dma_byte(write.address, write.data);
        }
        *self = registers;
        *command = candidate;
        Ok(completed)
    }

    pub(super) fn next_address(
        &self,
        command: &ControlCommand,
        increment: u8,
        mode: u8,
    ) -> Result<Option<u32>, VdpDmaError> {
        if self.source_high & DMA_MODE_MASK != mode {
            return Err(VdpDmaError::WrongOperation(self.source_high));
        }
        let valid_code = if mode == DMA_FILL_MODE {
            command.code & !(COMMAND_CD4 | COMMAND_CD5) == VRAM_WRITE_CODE
        } else {
            command.code & !COMMAND_CD5 == COMMAND_CD4
        };
        if !valid_code || command.address > COMMAND_ADDRESS_MASK {
            return Err(VdpDmaError::UnsupportedCommand(*command));
        }
        if command.code & COMMAND_CD5 == 0 {
            return Ok(None);
        }
        Ok(Some(command.next_address(increment)))
    }

    fn complete_byte(&mut self, command: &mut ControlCommand, next: u32) {
        self.source = self.source.wrapping_add(1);
        self.length = self.length.wrapping_sub(1);
        command.address = next;
        if self.length == 0 {
            command.code &= !COMMAND_CD5;
        }
    }
}
