use super::constants::*;
use super::{ControlCommand, VdpMemory, VdpWrite, VdpWriteError};
mod state;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VdpWriteFifo {
    slots: [Option<VdpWrite>; FIFO_CAPACITY],
    head: usize,
    count: usize,
    second_byte: bool,
}

impl VdpWriteFifo {
    pub(super) fn take_slot(&mut self) -> Option<(VdpWrite, Option<bool>)> {
        let entry = self.front()?;
        if entry.command.code & !COMMAND_CD4 == VRAM_WRITE_CODE {
            let phase = self.second_byte;
            if phase {
                self.retire();
            } else {
                self.second_byte = true;
            }
            Some((entry, Some(phase)))
        } else {
            self.retire();
            Some((entry, None))
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn is_full(&self) -> bool {
        self.count == FIFO_CAPACITY
    }

    /// Only FIFO flags; the device supplies the remaining status sources.
    pub fn status_bits(&self) -> u16 {
        if self.is_empty() {
            FIFO_EMPTY_STATUS
        } else if self.is_full() {
            FIFO_FULL_STATUS
        } else {
            0
        }
    }

    pub fn front(&self) -> Option<VdpWrite> {
        if self.is_empty() {
            None
        } else {
            self.slots[self.head]
        }
    }

    pub fn next_slot_data(&self) -> Option<u16> {
        self.slots[self.tail()].map(|entry| entry.data)
    }

    pub fn push(&mut self, entry: VdpWrite) -> Result<(), VdpWriteError> {
        Self::validate_command(entry.command)?;
        if self.is_full() {
            return Err(VdpWriteError::Full);
        }
        let tail = self.tail();
        self.slots[tail] = Some(entry);
        self.count += 1;
        Ok(())
    }

    /// Captures a mode-5 write and increments its logical address on acceptance.
    pub fn push_data(
        &mut self,
        command: &mut ControlCommand,
        increment: Option<u8>,
        data: u16,
    ) -> Result<VdpWrite, VdpWriteError> {
        Self::validate_command(*command)?;
        if !matches!(
            command.code & !COMMAND_CD4,
            VRAM_WRITE_CODE | CRAM_WRITE_CODE | VSRAM_WRITE_CODE
        ) && !command.is_unused_write_target()
        {
            return Err(VdpWriteError::UnsupportedCommand(*command));
        }
        let increment = increment.ok_or(VdpWriteError::UnknownIncrement)?;
        let next_address = command.next_address(increment);
        let entry = VdpWrite {
            command: *command,
            data,
        };
        self.push(entry)?;
        command.address = next_address;
        Ok(entry)
    }

    /// Completes the remaining word transfer without replaying an already served byte.
    pub fn complete_write(
        &mut self,
        memory: &mut VdpMemory,
    ) -> Result<Option<VdpWrite>, VdpWriteError> {
        let Some(entry) = self.front() else {
            return Ok(None);
        };
        if self.second_byte {
            memory.write_vram_byte(entry.command, entry.data, true)?;
        } else {
            memory.write_word(entry.command, entry.data)?;
        }
        self.retire();
        Ok(Some(entry))
    }

    /// Serves one eligible opportunity in stable mode 5; returns only a retired entry.
    pub fn service_write_slot(
        &mut self,
        memory: &mut VdpMemory,
    ) -> Result<Option<VdpWrite>, VdpWriteError> {
        let Some(entry) = self.front() else {
            return Ok(None);
        };
        if entry.command.code & !COMMAND_CD4 == VRAM_WRITE_CODE {
            memory.write_vram_byte(entry.command, entry.data, self.second_byte)?;
            if !self.second_byte {
                self.second_byte = true;
                return Ok(None);
            }
        } else {
            memory.write_word(entry.command, entry.data)?;
        }
        self.retire();
        Ok(Some(entry))
    }

    fn retire(&mut self) {
        self.head = (self.head + 1) % FIFO_CAPACITY;
        self.count -= 1;
        self.second_byte = false;
    }

    fn tail(&self) -> usize {
        (self.head + self.count) % FIFO_CAPACITY
    }

    fn validate_command(command: ControlCommand) -> Result<(), VdpWriteError> {
        if command.code & !COMMAND_CODE_MASK != 0 {
            return Err(VdpWriteError::InvalidCode(command.code));
        }
        if command.address & !COMMAND_ADDRESS_MASK != 0 {
            return Err(VdpWriteError::InvalidAddress(command.address));
        }
        Ok(())
    }
}
