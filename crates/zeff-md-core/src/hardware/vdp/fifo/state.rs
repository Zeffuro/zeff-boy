use super::*;
use crate::hardware::vdp::state::*;
use anyhow::{Result, ensure};

impl VdpWriteFifo {
    #[cfg(test)]
    pub(in crate::hardware::vdp) fn verify_byte_order_and_legacy_migration() {
        tests::byte_order_and_legacy_migration();
    }

    pub(in crate::hardware::vdp) fn write_state(&self, w: &mut StateWriter) {
        for entry in self.slots {
            write_optional(w, entry, |w, entry| {
                write_command(w, entry.command);
                w.write_u16(entry.data);
            });
        }
        w.write_u8(self.head as u8);
        w.write_u8(self.count as u8);
        w.write_bool(self.second_byte);
        w.write_u8(self.legacy_order_mask);
    }

    pub(in crate::hardware::vdp) fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        let mut slots = [None; FIFO_CAPACITY];
        for entry in &mut slots {
            *entry = read_optional(r, |r| {
                Ok(VdpWrite {
                    command: read_command(r)?,
                    data: r.read_u16()?,
                })
            })?;
        }
        let result = Self {
            slots,
            head: usize::from(r.read_u8()?),
            count: usize::from(r.read_u8()?),
            second_byte: r.read_bool()?,
            legacy_order_mask: { r.read_u8()? },
        };
        ensure!(
            result.head < FIFO_CAPACITY && result.count <= FIFO_CAPACITY,
            "invalid VDP FIFO cursor"
        );
        result.validate_state()?;
        Ok(result)
    }

    pub(in crate::hardware::vdp) fn validate_state(&self) -> Result<()> {
        ensure!(
            self.head < FIFO_CAPACITY && self.count <= FIFO_CAPACITY,
            "invalid VDP FIFO cursor"
        );
        ensure!(
            self.legacy_order_mask & !self.active_odd_vram_mask() == 0,
            "invalid VDP FIFO legacy byte order"
        );
        for entry in self.slots.iter().flatten() {
            validate_command(entry.command)?;
        }
        for offset in 0..self.count {
            let entry = self.slots[(self.head + offset) % FIFO_CAPACITY]
                .ok_or_else(|| anyhow::anyhow!("missing active VDP FIFO slot"))?;
            ensure!(
                matches!(
                    entry.command.code & !COMMAND_CD4,
                    VRAM_WRITE_CODE | CRAM_WRITE_CODE | VSRAM_WRITE_CODE
                ) || entry.command.is_no_effect_write_target(),
                "invalid active VDP FIFO command"
            );
        }
        ensure!(
            !self.second_byte
                || self
                    .front()
                    .is_some_and(|entry| entry.command.has_two_write_slots()),
            "invalid VDP FIFO byte phase"
        );
        Ok(())
    }

    fn active_odd_vram_mask(&self) -> u8 {
        (0..self.count).fold(0, |mask, offset| {
            let index = (self.head + offset) % FIFO_CAPACITY;
            mask | if self.slots[index].is_some_and(Self::odd_vram_write) {
                1 << index
            } else {
                0
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_effect_second_slot_roundtrips_without_memory_mutation() {
        for code in [0, 0x10, 9, 0x19] {
            let mut memory = VdpMemory::default();
            let saved = memory.clone();
            let mut fifo = VdpWriteFifo::default();
            let entry = VdpWrite {
                command: ControlCommand {
                    address: 0x1ffff,
                    code,
                },
                data: 0xdead,
            };
            fifo.push(entry).unwrap();
            for phase in 0..2 {
                let mut writer = StateWriter::new();
                fifo.write_state(&mut writer);
                let bytes = writer.into_bytes();
                let mut reader = StateReader::new(&bytes);
                let mut restored = VdpWriteFifo::read_state(&mut reader).unwrap();
                assert!(reader.is_exhausted());
                assert_eq!(restored, fifo);
                let expected = if phase == 0 { None } else { Some(entry) };
                let mut other_memory = memory.clone();
                assert_eq!(restored.service_write_slot(&mut other_memory), Ok(expected));
                assert_eq!(fifo.service_write_slot(&mut memory), Ok(expected));
                assert_eq!(restored, fifo);
                assert_eq!(memory, saved);
                assert_eq!(other_memory, saved);
                assert_eq!(fifo.second_byte, phase == 0);
            }
        }
    }

    fn state_bytes(fifo: &VdpWriteFifo, legacy: bool) -> Vec<u8> {
        let mut writer = StateWriter::new();
        let mut fifo = fifo.clone();
        if legacy {
            fifo.legacy_order_mask = fifo.active_odd_vram_mask();
        }
        fifo.write_state(&mut writer);
        writer.into_bytes()
    }

    fn restore_fifo(fifo: &VdpWriteFifo, legacy: bool) -> VdpWriteFifo {
        let bytes = state_bytes(fifo, legacy);
        let mut reader = StateReader::new(&bytes);
        let restored = VdpWriteFifo::read_state(&mut reader).unwrap();
        assert!(reader.is_exhausted());
        restored
    }

    fn write(address: u32, code: u8) -> VdpWrite {
        VdpWrite {
            command: ControlCommand { address, code },
            data: 0xabcd,
        }
    }

    pub(super) fn byte_order_and_legacy_migration() {
        use crate::hardware::vdp::memory::MemoryTransfer;
        for address in [0, 1, 0xfffe, 0xffff] {
            for code in [VRAM_WRITE_CODE, VRAM_WRITE_CODE | COMMAND_CD4] {
                for legacy in [false, true] {
                    let entry = write(address, code);
                    let base = usize::from(address as u16 & !WORD_ODD_ADDRESS_BIT);
                    let odd = address & 1 != 0;
                    let first_index = if legacy || !odd { 1 } else { 0 };
                    let first_value = if legacy && odd { 0xab } else { 0xcd };
                    let final_pair = if odd { [0xcd, 0xab] } else { [0xab, 0xcd] };
                    let mut expected_first = [0, 0];
                    expected_first[first_index] = first_value;
                    let mut fifo = VdpWriteFifo::default();
                    fifo.push(entry).unwrap();
                    fifo = restore_fifo(&fifo, legacy);
                    let mut memory = VdpMemory::default();
                    let mut direct = fifo.clone();
                    let mut direct_memory = memory.clone();
                    assert_eq!(direct.service_write_slot(&mut direct_memory), Ok(None));
                    assert_eq!(&direct_memory.vram()[base..base + 2], &expected_first);
                    let (taken, phase) = fifo.take_slot().unwrap();
                    assert_eq!(taken.command, entry.command);
                    assert!(fifo.second_byte);
                    let transfer = MemoryTransfer::Known(taken, phase);
                    let read = ControlCommand {
                        address,
                        code: VRAM_READ_CODE,
                    };
                    assert_eq!(
                        memory.read_after_transfers(read, None, [transfer].into_iter()),
                        Ok(u16::from_be_bytes(expected_first))
                    );
                    let undo = memory.publish_retirement(transfer);
                    assert_eq!(&memory.vram()[base..base + 2], &expected_first);
                    memory.undo_transfer(undo);
                    assert_eq!(&memory.vram()[base..base + 2], &[0, 0]);
                    memory.publish_retirement(transfer);
                    let partial = restore_fifo(&fifo, legacy);
                    assert_eq!(partial, fifo);
                    let mut completed = partial.clone();
                    let mut completed_memory = memory.clone();
                    completed_memory.write_dma_byte((base + first_index) as u16, 0x55);
                    completed.complete_write(&mut completed_memory).unwrap();
                    let mut preserved = final_pair;
                    preserved[first_index] = 0x55;
                    assert_eq!(&completed_memory.vram()[base..base + 2], &preserved);
                    assert_eq!(completed.legacy_order_mask, 0);
                    assert_eq!(
                        direct.service_write_slot(&mut direct_memory),
                        Ok(Some(entry))
                    );
                    let (taken, phase) = fifo.take_slot().unwrap();
                    memory.publish_retirement(MemoryTransfer::Known(taken, phase));
                    assert_eq!(&memory.vram()[base..base + 2], &final_pair);
                    assert_eq!(memory, direct_memory);
                    assert!(fifo.is_empty());
                    assert!(!fifo.second_byte);
                    assert_eq!(fifo.legacy_order_mask, 0);
                }
            }
        }
        let entry = write(1, VRAM_WRITE_CODE);
        let mut fifo = VdpWriteFifo::default();
        for _ in 0..FIFO_CAPACITY {
            fifo.push(entry).unwrap();
        }
        let mut fifo = restore_fifo(&fifo, true);
        assert_eq!(fifo.legacy_order_mask, 15);
        fifo.complete_write(&mut VdpMemory::default()).unwrap();
        fifo.push(entry).unwrap();
        assert_eq!(fifo.legacy_order_mask, 14);
        for remaining in [12, 8, 0] {
            let mut memory = VdpMemory::default();
            fifo.service_write_slot(&mut memory).unwrap();
            assert_eq!(&memory.vram()[0..2], &[0, 0xab]);
            fifo.service_write_slot(&mut memory).unwrap();
            assert_eq!(fifo.legacy_order_mask, remaining);
            assert_eq!(&memory.vram()[0..2], &[0xcd, 0xab]);
        }
        let mut memory = VdpMemory::default();
        fifo.service_write_slot(&mut memory).unwrap();
        assert_eq!(&memory.vram()[0..2], &[0xcd, 0]);
        let bytes = state_bytes(&fifo, false);
        assert_eq!(restore_fifo(&fifo, false), fifo);
        for bad in [1, 2, 8, 16, 255] {
            let mut malformed = bytes.clone();
            *malformed.last_mut().unwrap() = bad;
            let accepted = bad & !fifo.active_odd_vram_mask() == 0;
            assert_eq!(
                VdpWriteFifo::read_state(&mut StateReader::new(&malformed)).is_ok(),
                accepted
            );
        }
        fifo.complete_write(&mut memory).unwrap();
        assert_eq!(&memory.vram()[0..2], &[0xcd, 0xab]);
        assert_eq!(fifo.next_slot_data(), Some(0xabcd));
        assert_eq!(restore_fifo(&fifo, true).next_slot_data(), Some(0xabcd));
        for code in [CRAM_WRITE_CODE, VSRAM_WRITE_CODE, 0] {
            let mut other = VdpWriteFifo::default();
            other.push(write(1, code)).unwrap();
            let mut malformed = state_bytes(&other, false);
            *malformed.last_mut().unwrap() = 1;
            assert!(VdpWriteFifo::read_state(&mut StateReader::new(&malformed)).is_err());
        }
    }
}
