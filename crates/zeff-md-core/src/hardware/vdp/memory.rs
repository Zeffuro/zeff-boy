use super::constants::*;
use super::{ControlCommand, VdpReadError, VdpWriteError};
mod state;

/// Canonical mode-5 backing; inspection is separate from buffered port reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VdpMemory {
    vram: Box<[u8]>,
    cram: [u16; CRAM_WORDS],
    vsram: [u16; VSRAM_WORDS],
}

pub(crate) struct MemorySnapshot {
    vram: [u8; VRAM_SIZE],
    cram: [u16; CRAM_WORDS],
    vsram: [u16; VSRAM_WORDS],
}

#[derive(Clone, Copy)]
pub(super) enum WriteUndo {
    Noop,
    Vram(usize, u8),
    Cram(usize, u16),
    Vsram(usize, u16),
}

impl Default for VdpMemory {
    fn default() -> Self {
        Self {
            vram: vec![0; VRAM_SIZE].into_boxed_slice(),
            cram: [0; CRAM_WORDS],
            vsram: [0; VSRAM_WORDS],
        }
    }
}

impl VdpMemory {
    pub(super) fn publish_transfer(
        &mut self,
        entry: super::VdpWrite,
        phase: Option<bool>,
    ) -> WriteUndo {
        let address = entry.command.address as u16;
        if let Some(second) = phase {
            let index = usize::from(address & !WORD_ODD_ADDRESS_BIT) + usize::from(!second);
            let undo = WriteUndo::Vram(index, self.vram[index]);
            self.write_vram_byte(entry.command, entry.data, second)
                .expect("validated FIFO write target");
            undo
        } else {
            let index = usize::from((address & COLOR_WORD_ADDRESS_MASK) >> 1);
            let undo = match entry.command.code & !COMMAND_CD4 {
                CRAM_WRITE_CODE => WriteUndo::Cram(index, self.cram[index]),
                VSRAM_WRITE_CODE => self
                    .vsram
                    .get(index)
                    .map_or(WriteUndo::Noop, |word| WriteUndo::Vsram(index, *word)),
                _ if entry.command.is_unused_write_target() => WriteUndo::Noop,
                _ => unreachable!("validated color transfer"),
            };
            self.write_word(entry.command, entry.data)
                .expect("validated FIFO write target");
            undo
        }
    }

    pub(super) fn undo_transfer(&mut self, undo: WriteUndo) {
        match undo {
            WriteUndo::Noop => {}
            WriteUndo::Vram(index, byte) => self.vram[index] = byte,
            WriteUndo::Cram(index, word) => self.cram[index] = word,
            WriteUndo::Vsram(index, word) => self.vsram[index] = word,
        }
    }

    pub(crate) fn snapshot(&self) -> MemorySnapshot {
        let mut vram = [0; VRAM_SIZE];
        vram.copy_from_slice(&self.vram);
        MemorySnapshot {
            vram,
            cram: self.cram,
            vsram: self.vsram,
        }
    }

    pub(crate) fn restore(&mut self, snapshot: MemorySnapshot) {
        self.vram.copy_from_slice(&snapshot.vram);
        self.cram = snapshot.cram;
        self.vsram = snapshot.vsram;
    }

    pub(super) fn clear(&mut self) {
        self.vram.fill(0);
        self.cram.fill(0);
        self.vsram.fill(0);
    }

    pub fn vram(&self) -> &[u8] {
        &self.vram
    }

    pub(super) fn write_dma_byte(&mut self, address: u16, data: u8) {
        self.vram[usize::from(address)] = data;
    }

    pub fn cram(&self) -> &[u16] {
        &self.cram
    }

    pub fn vsram(&self) -> &[u16] {
        &self.vsram
    }

    /// Completed conventional VRAM word; buffering and byte service are separate.
    pub fn read_word(&self, command: ControlCommand) -> Result<u16, VdpReadError> {
        if command.code != VRAM_READ_CODE || command.address > COMMAND_ADDRESS_MASK {
            return Err(VdpReadError::UnsupportedCommand(command));
        }
        let index = usize::from(command.address as u16 & !WORD_ODD_ADDRESS_BIT);
        Ok(u16::from_be_bytes([self.vram[index], self.vram[index + 1]]))
    }

    pub(super) fn read_word_with_history(
        &self,
        command: ControlCommand,
        history: Option<u16>,
    ) -> Result<u16, VdpReadError> {
        if command.address > COMMAND_ADDRESS_MASK {
            return Err(VdpReadError::UnsupportedCommand(command));
        }
        let index = usize::from((command.address as u16 & COLOR_WORD_ADDRESS_MASK) >> 1);
        let (word, mask) = match command.code {
            CRAM_READ_CODE => (self.cram[index], CRAM_DATA_MASK),
            VSRAM_READ_CODE => {
                let Some(word) = self.vsram.get(index) else {
                    return Err(VdpReadError::UnsupportedCommand(command));
                };
                (*word, VSRAM_DATA_MASK)
            }
            _ => return self.read_word(command),
        };
        let history = history.ok_or(VdpReadError::UnknownFifoHistory)?;
        Ok((word & mask) | (history & !mask))
    }

    pub(super) fn read_after_writes(
        &self,
        command: ControlCommand,
        history: Option<u16>,
        writes: impl Iterator<Item = (super::VdpWrite, Option<bool>)>,
    ) -> Result<u16, VdpReadError> {
        let mut word = self.read_word_with_history(command, history)?;
        for (entry, phase) in writes {
            let code = entry.command.code & !COMMAND_CD4;
            if command.code == VRAM_READ_CODE
                && code == VRAM_WRITE_CODE
                && command.address as u16 & !WORD_ODD_ADDRESS_BIT
                    == entry.command.address as u16 & !WORD_ODD_ADDRESS_BIT
            {
                let mut bytes = entry.data.to_be_bytes();
                if entry.command.address & u32::from(WORD_ODD_ADDRESS_BIT) != 0 {
                    bytes.swap(0, 1);
                }
                let mut current = word.to_be_bytes();
                let byte = usize::from(!phase.expect("VRAM byte batch"));
                current[byte] = bytes[byte];
                word = u16::from_be_bytes(current);
            } else if command.address & u32::from(COLOR_WORD_ADDRESS_MASK)
                == entry.command.address & u32::from(COLOR_WORD_ADDRESS_MASK)
            {
                let mask = match (command.code, code) {
                    (CRAM_READ_CODE, CRAM_WRITE_CODE) => CRAM_DATA_MASK,
                    (VSRAM_READ_CODE, VSRAM_WRITE_CODE) => VSRAM_DATA_MASK,
                    _ => continue,
                };
                word = (word & !mask) | (entry.data & mask);
            }
        }
        Ok(word)
    }

    pub(super) fn write_vram_byte(
        &mut self,
        command: ControlCommand,
        data: u16,
        second_byte: bool,
    ) -> Result<(), VdpWriteError> {
        if command.code & !COMMAND_CD4 != VRAM_WRITE_CODE || command.address > COMMAND_ADDRESS_MASK
        {
            return Err(VdpWriteError::UnsupportedCommand(command));
        }
        let address = command.address as u16;
        let index = usize::from(address & !WORD_ODD_ADDRESS_BIT);
        let mut bytes = data.to_be_bytes();
        if address & WORD_ODD_ADDRESS_BIT != 0 {
            bytes.swap(0, 1);
        }
        let byte = usize::from(!second_byte);
        self.vram[index + byte] = bytes[byte];
        Ok(())
    }

    pub fn write_word(&mut self, command: ControlCommand, data: u16) -> Result<(), VdpWriteError> {
        if command.address > COMMAND_ADDRESS_MASK {
            return Err(VdpWriteError::UnsupportedCommand(command));
        }
        let address = command.address as u16;
        match command.code & !COMMAND_CD4 {
            VRAM_WRITE_CODE => {
                let index = usize::from(address & !WORD_ODD_ADDRESS_BIT);
                let mut bytes = data.to_be_bytes();
                if address & WORD_ODD_ADDRESS_BIT != 0 {
                    bytes.swap(0, 1);
                }
                self.vram[index..index + bytes.len()].copy_from_slice(&bytes);
            }
            CRAM_WRITE_CODE => {
                let index = usize::from((address & COLOR_WORD_ADDRESS_MASK) >> 1);
                self.cram[index] = data & CRAM_DATA_MASK;
            }
            VSRAM_WRITE_CODE => {
                let index = usize::from((address & COLOR_WORD_ADDRESS_MASK) >> 1);
                if let Some(word) = self.vsram.get_mut(index) {
                    *word = data & VSRAM_DATA_MASK;
                }
            }
            _ if command.is_unused_write_target() => {}
            _ => return Err(VdpWriteError::UnsupportedCommand(command)),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hardware::vdp::{VdpPixel, VdpPlane, VdpRegisterPipeline, VdpWrite};

    #[test]
    fn sampled_plane_pixels_match_tile_rows() {
        let mut memory = VdpMemory::default();
        for address in 0..VRAM_SIZE {
            memory.write_dma_byte(address as u16, (address as u8).wrapping_mul(29));
        }
        let mut registers = VdpRegisterPipeline::bootstrap_mode5();
        for (index, value) in [(PLANE_A_TABLE_REGISTER, 0x20), (PLANE_B_TABLE_REGISTER, 4)] {
            registers
                .advance_to(zeff_emu_common::time::MasterTicks::new(
                    registers.now().get() + 1,
                ))
                .unwrap();
            let write = registers.queue_register(index, value).unwrap();
            registers.advance_to(write.publish_at).unwrap();
        }
        for tile in [0, 1, 7, 255, 256, 1023, 2046, 2047] {
            for attributes in 0..32 {
                let name = tile | (attributes << 11);
                memory
                    .write_word(
                        ControlCommand {
                            address: 0x8000,
                            code: VRAM_WRITE_CODE,
                        },
                        name,
                    )
                    .unwrap();
                for y in 0..TILE_HEIGHT {
                    let row = memory.tile_row(name, y).unwrap();
                    for x in 0..TILE_WIDTH {
                        let expected = VdpPixel {
                            color: row.colors[x],
                            palette: row.palette,
                            priority: row.priority,
                        };
                        for plane in [VdpPlane::A, VdpPlane::B] {
                            assert_eq!(
                                memory.sample_plane(&registers, plane, x as u16, u16::from(y)),
                                Ok(expected),
                                "name {name:04x}, pixel {x},{y}, plane {plane:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn discarded_write_has_reversible_noop() {
        let mut memory = VdpMemory::default();
        memory
            .write_word(
                ControlCommand {
                    address: 0x4e,
                    code: VSRAM_WRITE_CODE,
                },
                0x1234,
            )
            .unwrap();
        let saved = memory.clone();
        let snapshot = memory.snapshot();
        for code in [
            VSRAM_WRITE_CODE,
            VSRAM_WRITE_CODE | COMMAND_CD4,
            2,
            0x12,
            7,
            0x17,
        ] {
            for address in 0x50..=0x7f {
                let entry = VdpWrite {
                    command: ControlCommand { address, code },
                    data: 0xffff,
                };
                let undo = memory.publish_transfer(entry, None);
                assert!(matches!(undo, WriteUndo::Noop));
                assert_eq!(memory, saved);
                memory.undo_transfer(undo);
                assert_eq!(memory, saved);
            }
        }
        memory
            .write_word(
                ControlCommand {
                    address: 0x80,
                    code: VSRAM_WRITE_CODE,
                },
                0xffff,
            )
            .unwrap();
        assert_eq!(memory.vsram()[0], VSRAM_DATA_MASK);
        memory.restore(snapshot);
        assert_eq!(memory, saved);
    }
}
