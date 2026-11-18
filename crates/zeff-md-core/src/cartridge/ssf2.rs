use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};
use zeff_m68k::bus::{BusAccess, ByteLanes};

const PAGE_BYTES: usize = 0x80000;
const ROM_BYTES: usize = PAGE_BYTES * 10;
// Initial checksum reads the linear 4 MiB window before writing bank controls.
const INITIAL_SLOTS: [u8; 7] = [1, 2, 3, 4, 5, 6, 7];
const PROFILES: [(&[u8; 14], &str); 3] = [
    (
        b"GM MK-12056-00",
        "d3700c90ba971d0904d6391129ac169ba9b343d73537cde2f818f044ed0c5631",
    ),
    (
        b"GM T-12056 -00",
        "a409ea5b53bdb64051b98257d8b64af432bfcf98d6fb8a95dc2c66d5ebd71926",
    ),
    (
        b"GM T-12043 -00",
        "f1c72093f19a5360678bbaf664338b1415e814e84afa1dcd3b92518bf04d01b6",
    ),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ssf2 {
    slots: [u8; 7],
    #[cfg(test)]
    fixture_sha: Option<[u8; 32]>,
}

impl Default for Ssf2 {
    fn default() -> Self {
        Self::new()
    }
}

impl Ssf2 {
    pub fn detect(rom: &[u8]) -> bool {
        Self::identity(
            rom.len(),
            rom.get(0x180..0x18e),
            rom.get(0x1b0..0x1bc),
            || {
                Sha256::digest(rom)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect()
            },
        )
    }

    fn identity(
        size: usize,
        product: Option<&[u8]>,
        save: Option<&[u8]>,
        digest: impl FnOnce() -> String,
    ) -> bool {
        if size != ROM_BYTES || save != Some(&[b' '; 12]) {
            return false;
        }
        let Some((_, expected)) = PROFILES
            .iter()
            .find(|(candidate, _)| product == Some(candidate.as_slice()))
        else {
            return false;
        };
        digest() == *expected
    }

    pub fn new() -> Self {
        Self {
            slots: INITIAL_SLOTS,
            #[cfg(test)]
            fixture_sha: None,
        }
    }

    pub fn matches_rom(&self, rom: &[u8]) -> bool {
        #[cfg(test)]
        if let Some(expected) = self.fixture_sha {
            return rom.len() == ROM_BYTES && <[u8; 32]>::from(Sha256::digest(rom)) == expected;
        }
        Self::detect(rom)
    }

    pub fn mode_sensitive(address: u32) -> bool {
        (0x80000..=0x3fffff).contains(&address)
    }

    pub fn source_offset(&self, address: u32) -> Option<usize> {
        if address > 0x3fffff {
            return None;
        }
        let slot = address as usize / PAGE_BYTES;
        let page = if slot == 0 { 0 } else { self.slots[slot - 1] };
        Some(usize::from(page) * PAGE_BYTES + address as usize % PAGE_BYTES)
    }

    pub fn control(&mut self, access: BusAccess, value: u16) -> bool {
        if access.lanes != ByteLanes::Lower
            || access.logical_address & 1 == 0
            || access.bus_address != access.logical_address & !1
        {
            return false;
        }
        if access.logical_address == 0xa130f1 {
            return value as u8 == 2;
        }
        if !(0xa130f3..=0xa130ff).contains(&access.logical_address) || value as u8 >= 10 {
            return false;
        }
        let slot = ((access.logical_address - 0xa130f3) / 2) as usize;
        self.slots[slot] = value as u8;
        true
    }

    pub fn power_on(&mut self) {
        self.slots = INITIAL_SLOTS;
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        for page in self.slots {
            writer.write_u8(page);
        }
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        let mut slots = [0; 7];
        for slot in &mut slots {
            let value = reader.read_u8()?;
            ensure!(value < 10, "invalid MD banked ROM selection");
            *slot = value;
        }
        self.slots = slots;
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests;
