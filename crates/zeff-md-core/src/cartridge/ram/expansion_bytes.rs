use super::{OddSaveLayout, SaveMemory};
use crate::cartridge::Cartridge;
use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const START: u32 = 0x400000;
const BYTES: usize = 65536;
const STATE_TAG: u16 = 0x5842;
const HEADER_CHECKSUM: [u8; 2] = [0x81, 0x04];
const DECLARATION: [u8; 12] = [b' '; 12];
const DIGESTS: [[u8; 32]; 2] = [
    [
        0x08, 0x3c, 0x8c, 0x9d, 0xf4, 0x90, 0x73, 0x30, 0x90, 0x0c, 0x68, 0x01, 0xda, 0x77, 0x7f,
        0x90, 0x64, 0x0b, 0x20, 0xdc, 0x36, 0x06, 0x09, 0x20, 0x29, 0x1e, 0x73, 0xde, 0xbc, 0x56,
        0x2e, 0xd9,
    ],
    [
        0xc7, 0x1e, 0x9d, 0xee, 0xc8, 0x8b, 0x15, 0x41, 0x90, 0x42, 0xf5, 0x7e, 0xfa, 0x03, 0x3d,
        0x45, 0x75, 0x27, 0x92, 0xe0, 0x0b, 0xa2, 0xc3, 0x65, 0x27, 0x48, 0x8f, 0x54, 0xe3, 0xc2,
        0xdf, 0x16,
    ],
];

fn identity(
    len: usize,
    checksum: Option<&[u8]>,
    declaration: Option<&[u8]>,
    digest: impl FnOnce() -> [u8; 32],
) -> bool {
    len == 0x400000
        && checksum == Some(HEADER_CHECKSUM.as_slice())
        && declaration == Some(DECLARATION.as_slice())
        && DIGESTS.contains(&digest())
}

pub(super) fn domain(address: u32) -> bool {
    (START..START + BYTES as u32).contains(&address)
}

pub(super) fn detect(rom: &[u8]) -> bool {
    #[cfg(test)]
    if tests::fixture_matches(rom) {
        return true;
    }
    identity(
        rom.len(),
        rom.get(0x18e..0x190),
        rom.get(0x1b0..0x1bc),
        || Sha256::digest(rom).into(),
    )
}

impl SaveMemory {
    pub(super) fn new_expansion_bytes() -> Self {
        Self {
            layout: OddSaveLayout::ExpansionBytesKiB64,
            start: START,
            switched: false,
            initially_enabled: true,
            time_latch: false,
            enabled: true,
            write_protected: false,
            bytes: vec![0xff; BYTES].into_boxed_slice(),
            serial: None,
        }
    }

    pub(crate) fn expansion_bytes(&self) -> bool {
        self.layout == OddSaveLayout::ExpansionBytesKiB64
    }

    pub(super) fn validate_expansion_bytes(&self) -> Result<()> {
        ensure!(
            self.expansion_bytes()
                && self.start == START
                && !self.switched
                && self.initially_enabled
                && self.enabled
                && !self.write_protected
                && !self.time_latch
                && self.serial.is_none()
                && self.bytes.len() == BYTES,
            "invalid MD expansion byte save mapping"
        );
        Ok(())
    }

    pub(super) fn write_expansion_bytes_tag(&self, writer: &mut StateWriter) {
        if self.expansion_bytes() {
            writer.write_u16(STATE_TAG);
        }
    }

    pub(super) fn read_expansion_bytes_tag(&self, reader: &mut StateReader<'_>) -> Result<()> {
        if self.expansion_bytes() {
            ensure!(
                reader.read_u16()? == STATE_TAG,
                "MD expansion byte save state has no matching owner"
            );
        }
        Ok(())
    }
}

impl Cartridge {
    pub(crate) fn expansion_save_contains(&self, address: u32) -> bool {
        domain(address) && self.save.as_ref().is_some_and(SaveMemory::expansion_bytes)
    }

    pub(crate) fn validate_expansion_save(&self) -> Result<()> {
        let expected = detect(&self.bytes);
        ensure!(
            expected == self.save.as_ref().is_some_and(SaveMemory::expansion_bytes),
            "MD expansion save presence differs from ROM"
        );
        if expected {
            ensure!(
                self.save_latch.is_none()
                    && self.download.is_none()
                    && self.linear_rom.is_none()
                    && self.unprotected_rom.is_none()
                    && self.parallel_adapter.is_none()
                    && self.registers.is_none()
                    && self.protection.is_none()
                    && self.sf001.is_none()
                    && self.sf002.is_none()
                    && self.sf004.is_none()
                    && self.extended_ssf.is_none()
                    && self.realtec.is_none()
                    && self.ssf2.is_none()
                    && self.lock_on.is_none()
                    && self.lock_on_save.is_none(),
                "MD expansion save has another cartridge owner"
            );
            self.save.as_ref().unwrap().validate_expansion_bytes()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
pub(super) use tests::contract;
#[cfg(test)]
pub(crate) use tests::fixture_rom;
