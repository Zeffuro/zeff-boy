use super::*;

// DD/FD prefixes return separately; an ED word load needs at most six reads.
pub(super) const MAX_ROM_READS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RomRead {
    pub ordinal: u32,
    pub local: u16,
    pub offset: u32,
    pub original: u8,
    pub effective: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct RomReads {
    pub records: [Option<RomRead>; MAX_ROM_READS],
}

impl RomReads {
    pub fn push(&mut self, read: RomRead) -> bool {
        let Some(slot) = self.records.iter_mut().find(|slot| slot.is_none()) else {
            return false;
        };
        *slot = Some(read);
        true
    }

    pub fn changed(&self) -> bool {
        self.records
            .iter()
            .flatten()
            .any(|read| read.original != read.effective)
    }

    pub fn values(&self) -> Vec<RomRead> {
        self.records.iter().flatten().copied().collect()
    }
}

impl CandidateBus<'_> {
    pub(super) fn svp_rom_read(&self, local: u16, offset: u32) -> u8 {
        let Some(&original) = self.cartridge.bytes.get(offset as usize) else {
            self.fail(SoundFault::UnsupportedAccess {
                address: local,
                direction: Direction::Read,
            });
            return UNPOPULATED_ROM_BYTE;
        };
        let ordinal = self.accesses.get();
        let effective = if let Some(history) = self.historical_rom {
            let cursor = self.historical_cursor.get();
            let Some(read) = history.get(cursor).filter(|read| {
                read.ordinal == ordinal
                    && read.local == local
                    && read.offset == offset
                    && read.original == original
            }) else {
                self.fail(SoundFault::UnsupportedAccess {
                    address: local,
                    direction: Direction::Read,
                });
                return UNPOPULATED_ROM_BYTE;
            };
            self.historical_cursor.set(cursor + 1);
            read.effective
        } else {
            self.cartridge.source_byte(offset as usize).unwrap()
        };
        if !self.svp_reads.borrow_mut().push(RomRead {
            ordinal,
            local,
            offset,
            original,
            effective,
        }) {
            self.fail(SoundFault::UnsupportedAccess {
                address: local,
                direction: Direction::Read,
            });
            return UNPOPULATED_ROM_BYTE;
        }
        self.bank_read(offset, effective)
    }
}
