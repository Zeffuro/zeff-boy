use super::LoadError;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(crate) const SAVE_START: u32 = 0x20_0000;
pub(crate) const SAVE_CONTROL: u32 = 0xa1_30f0;
const DECLARATION_PREFIX: [u8; 10] = [b'R', b'A', 0xf8, 0x20, 0, 0x20, 0, 1, 0, 0x20];
const HEADER_START: usize = 0x1b0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OddSaveLayout {
    Bytes512,
    KiB8,
    KiB32,
}

impl OddSaveLayout {
    fn bytes(self) -> usize {
        match self {
            Self::Bytes512 => 512,
            Self::KiB8 => 8192,
            Self::KiB32 => 32768,
        }
    }

    fn initially_enabled(self) -> bool {
        self != Self::Bytes512
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SaveMemory {
    layout: OddSaveLayout,
    pub enabled: bool,
    pub bytes: Box<[u8]>,
}

impl SaveMemory {
    pub fn from_rom(rom: &[u8]) -> Result<Option<Self>, LoadError> {
        if rom.get(HEADER_START..HEADER_START + 2) != Some(b"RA") {
            return Ok(None);
        }
        if rom.get(HEADER_START..HEADER_START + DECLARATION_PREFIX.len())
            != Some(DECLARATION_PREFIX.as_slice())
            || rom.len() > SAVE_START as usize
        {
            return Err(LoadError::UnsupportedSaveHardware);
        }
        let layout = match rom.get(HEADER_START + 10..HEADER_START + 12) {
            Some([3, 0xff]) => OddSaveLayout::Bytes512,
            Some([0x3f, 0xff]) => OddSaveLayout::KiB8,
            Some([0xff, 0xff]) => OddSaveLayout::KiB32,
            _ => return Err(LoadError::UnsupportedSaveHardware),
        };
        // Larger non-overlap layouts use a functional startup mapping policy.
        Ok(Some(Self {
            layout,
            enabled: layout.initially_enabled(),
            bytes: vec![0xff; layout.bytes()].into_boxed_slice(),
        }))
    }

    pub fn contains(&self, address: u32) -> bool {
        (SAVE_START..SAVE_START + self.layout.bytes() as u32 * 2).contains(&address)
    }

    pub fn reset_mapping(&mut self) {
        self.enabled = self.layout.initially_enabled();
    }

    pub fn validate_state(&self) -> Result<()> {
        ensure!(
            self.bytes.len() == self.layout.bytes(),
            "invalid MD save RAM size"
        );
        ensure!(
            self.enabled || !self.layout.initially_enabled(),
            "invalid MD save RAM mapping"
        );
        Ok(())
    }

    pub fn read(&self, address: u32) -> Option<u8> {
        self.enabled.then(|| {
            if address & 1 == 0 {
                0xff
            } else {
                self.bytes[((address - SAVE_START) / 2) as usize]
            }
        })
    }

    pub fn write(&mut self, address: u32, value: u8) {
        if address & 1 != 0 {
            self.bytes[((address - SAVE_START) / 2) as usize] = value;
        }
    }

    pub fn load(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() == self.layout.bytes(),
            "MD save RAM must contain exactly {} bytes",
            self.layout.bytes()
        );
        self.bytes.copy_from_slice(bytes);
        Ok(())
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bool(self.enabled);
        writer.write_bytes(self.bytes.as_ref());
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        self.enabled = reader.read_bool()?;
        reader.read_exact(self.bytes.as_mut())?;
        self.validate_state()
    }
}
