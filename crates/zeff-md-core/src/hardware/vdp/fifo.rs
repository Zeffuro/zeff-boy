use super::constants::*;
use super::{ControlCommand, VdpMemory, VdpWrite, VdpWriteError};
mod state;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VdpWriteFifo {
    slots: [Option<VdpWrite>; FIFO_CAPACITY],
    head: usize,
    count: usize,
    second_byte: bool,
    legacy_order_mask: u8,
}

impl VdpWriteFifo {
    pub(super) fn take_slot(&mut self) -> Option<(VdpWrite, Option<bool>)> {
        let entry = self.front()?;
        if entry.command.has_two_write_slots() {
            let phase = self.memory_byte_phase(entry);
            if self.second_byte {
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

    pub(super) fn known_zero_history(&self) -> bool {
        !self.second_byte && self.legacy_order_mask == 0 && self.zero_words()
    }

    pub(super) fn zero_fill_write_pending(&self) -> bool {
        self.count == 1
            && self.legacy_order_mask == 0
            && self.zero_words()
            && self
                .front()
                .is_some_and(|entry| entry.command.code == VRAM_WRITE_CODE)
    }

    fn zero_words(&self) -> bool {
        self.slots
            .iter()
            .all(|slot| slot.is_some_and(|entry| entry.data == 0))
    }

    pub fn push(&mut self, entry: VdpWrite) -> Result<(), VdpWriteError> {
        Self::validate_command(entry.command)?;
        if self.is_full() {
            return Err(VdpWriteError::Full);
        }
        let tail = self.tail();
        self.slots[tail] = Some(entry);
        self.legacy_order_mask &= !(1 << tail);
        self.count += 1;
        Ok(())
    }

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
        ) && !command.is_no_effect_write_target()
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

    pub fn complete_write(
        &mut self,
        memory: &mut VdpMemory,
    ) -> Result<Option<VdpWrite>, VdpWriteError> {
        let Some(entry) = self.front() else {
            return Ok(None);
        };
        if self.second_byte && entry.command.code & !COMMAND_CD4 == VRAM_WRITE_CODE {
            memory.write_vram_byte(entry.command, entry.data, self.memory_byte_phase(entry))?;
        } else {
            memory.write_word(entry.command, entry.data)?;
        }
        self.retire();
        Ok(Some(entry))
    }

    pub fn service_write_slot(
        &mut self,
        memory: &mut VdpMemory,
    ) -> Result<Option<VdpWrite>, VdpWriteError> {
        let Some(entry) = self.front() else {
            return Ok(None);
        };
        if entry.command.has_two_write_slots() {
            if entry.command.code & !COMMAND_CD4 == VRAM_WRITE_CODE {
                memory.write_vram_byte(entry.command, entry.data, self.memory_byte_phase(entry))?;
            }
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
        self.legacy_order_mask &= !(1 << self.head);
        self.head = (self.head + 1) % FIFO_CAPACITY;
        self.count -= 1;
        self.second_byte = false;
    }

    fn tail(&self) -> usize {
        (self.head + self.count) % FIFO_CAPACITY
    }

    fn memory_byte_phase(&self, entry: VdpWrite) -> bool {
        let fresh_odd =
            Self::odd_vram_write(entry) && self.legacy_order_mask & (1 << self.head) == 0;
        self.second_byte ^ fresh_odd
    }

    fn odd_vram_write(entry: VdpWrite) -> bool {
        entry.command.code & !COMMAND_CD4 == VRAM_WRITE_CODE
            && entry.command.address & u32::from(WORD_ODD_ADDRESS_BIT) != 0
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
