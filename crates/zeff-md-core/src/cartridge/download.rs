use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const ORIGINAL_BYTES: usize = 0x300000;
pub(crate) const VIEW_BYTES: usize = 0x400000;
const IDENTITY: [u8; 32] = [
    0x9b, 0x5b, 0x7e, 0x0a, 0xe9, 0x38, 0x9a, 0xfd, 0x35, 0x95, 0x19, 0xe6, 0xea, 0x05, 0xf1, 0x0e,
    0xe7, 0x51, 0xc5, 0x05, 0x79, 0x86, 0x06, 0xfd, 0x38, 0x77, 0xa8, 0xe8, 0xd4, 0x64, 0x82, 0x2a,
];

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct DownloadRam {
    digest: [u8; 32],
    writable: bool,
    bytes: Arc<[u8]>,
}

impl fmt::Debug for DownloadRam {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DownloadRam")
            .field("writable", &self.writable)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

impl DownloadRam {
    pub(crate) fn detect(
        rom: &[u8],
        header: Option<&CartridgeHeader>,
        digest: impl FnOnce() -> [u8; 32],
    ) -> Option<Self> {
        if !Self::geometry(rom) {
            return None;
        }
        #[cfg(test)]
        if rom.get(0x80..0x90) == Some(b"DOWNLOAD_RAM_TST") {
            return Self::from_identity(rom, digest());
        }
        let header = header?;
        if header.checksum != 0 || header.calculated_checksum != 0xf9d1 {
            return None;
        }
        Self::from_identity(rom, digest())
    }

    fn geometry(rom: &[u8]) -> bool {
        rom.len() == ORIGINAL_BYTES
            && rom.get(0x1b0..0x1bc) == Some(b"            ")
            && rom.get(0x18e..0x190) == Some(&[0, 0])
    }

    fn accepted_identity(rom: &[u8], digest: [u8; 32]) -> bool {
        if !Self::geometry(rom) {
            return false;
        }
        if digest == IDENTITY {
            return true;
        }
        #[cfg(test)]
        if rom.get(0x80..0x90) == Some(b"DOWNLOAD_RAM_TST") {
            return true;
        }
        false
    }

    fn from_identity(rom: &[u8], digest: [u8; 32]) -> Option<Self> {
        Self::accepted_identity(rom, digest).then(|| {
            let mut bytes = vec![0xff; VIEW_BYTES];
            bytes[..rom.len()].copy_from_slice(rom);
            Self {
                digest,
                writable: false,
                bytes: bytes.into(),
            }
        })
    }

    pub(crate) fn read(&self, address: u32) -> Option<u8> {
        self.bytes.get(address as usize).copied()
    }

    pub(crate) fn write(&mut self, address: u32, value: u8) -> bool {
        if address as usize >= VIEW_BYTES {
            return false;
        }
        if self.writable {
            Arc::make_mut(&mut self.bytes)[address as usize] = value;
        }
        true
    }

    pub(crate) fn control(&mut self, address: u32, value: u8) {
        if address == 0xa130f1 {
            self.writable = value & 1 == 0;
        }
    }

    pub(crate) fn power_on(&mut self, original: &[u8]) {
        self.writable = false;
        let bytes = Arc::make_mut(&mut self.bytes);
        bytes.fill(0xff);
        bytes[..original.len()].copy_from_slice(original);
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.writable);
        writer.write_u32(VIEW_BYTES as u32);
        writer.write_bytes(&self.bytes);
    }

    pub(crate) fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        let writable = reader.read_bool()?;
        ensure!(
            reader.read_u32()? as usize == VIEW_BYTES,
            "MD download RAM size mismatch"
        );
        let mut bytes = vec![0; VIEW_BYTES];
        reader.read_exact(&mut bytes)?;
        self.writable = writable;
        self.bytes = bytes.into();
        Ok(())
    }
}

impl Cartridge {
    pub(crate) fn debug_download_fetch_span(&self, address: u32) -> bool {
        address.checked_add(3).is_some_and(memory)
            && self.download.is_some()
            && self.svp.is_none()
            && Arc::ptr_eq(&self.bytes, &self.digest_memo.seal)
            && self.validate_download().is_ok()
    }

    pub(crate) fn download_compatible(&self) -> bool {
        self.save.is_none()
            && self.save_latch.is_none()
            && self.unprotected_rom.is_none()
            && self.linear_rom.is_none()
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
            && self.lock_on_save.is_none()
    }

    pub(crate) fn validate_download(&self) -> Result<()> {
        let expected = DownloadRam::geometry(&self.bytes)
            && DownloadRam::accepted_identity(&self.bytes, self.rom_digest());
        ensure!(
            self.download.is_some() == expected,
            "MD download RAM identity mismatch"
        );
        if let Some(board) = &self.download {
            ensure!(
                self.download_compatible()
                    && board.digest == self.rom_digest()
                    && board.bytes.len() == VIEW_BYTES,
                "MD download RAM owner mismatch"
            );
        }
        Ok(())
    }
}

mod access;
pub(crate) use access::{memory, time};
#[cfg(test)]
#[path = "download/tests.rs"]
mod tests;
#[cfg(test)]
pub(crate) use tests::contract;
