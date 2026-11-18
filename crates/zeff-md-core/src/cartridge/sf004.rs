use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

pub(crate) const SAVE_BYTES: usize = 0x8000;
const VALID_BYTES: usize = SAVE_BYTES / 8;
const PRODUCT: &[u8; 14] = b"SF-004        ";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Sf004 {
    bank: u8,
    expanded: bool,
    save_enabled: bool,
    locked: bool,
    ram: Box<[u8]>,
    valid: Box<[u8]>,
}

impl Sf004 {
    pub fn detect(rom: &[u8]) -> bool {
        rom.len() == 0x200000
            && rom.get(0x180..0x18e) == Some(PRODUCT.as_slice())
            && rom.get(0x1a0..0x1a8) == Some(&[0, 0, 0, 0, 0, 3, 0xff, 0xff])
            && rom.get(0x1b0..0x1bc) == Some(&[b' '; 12])
    }

    pub fn new() -> Self {
        Self {
            bank: 0,
            expanded: false,
            save_enabled: false,
            locked: false,
            ram: vec![0xff; SAVE_BYTES].into_boxed_slice(),
            valid: vec![0xff; VALID_BYTES].into_boxed_slice(),
        }
    }

    pub fn domain(address: u32) -> bool {
        address <= 0x7fffff || (0xa13000..=0xa130ff).contains(&address)
    }

    pub fn source_offset(&self, address: u32) -> Option<usize> {
        let base = usize::from(self.bank) << 18;
        if self.expanded {
            (address < 0x140000).then_some((base + address as usize) & 0x1fffff)
        } else {
            (address < 0x200000).then_some(base | (address as usize & 0x3ffff))
        }
    }

    pub fn time_read(&self) -> u8 {
        self.bank << 4
    }

    fn cell(&self, address: u32) -> Option<usize> {
        (self.save_enabled && (0x200001..=0x2fffff).contains(&address) && address & 1 != 0)
            .then_some((address as usize >> 1) & (SAVE_BYTES - 1))
    }

    pub fn read(&self, address: u32, rom: &[u8]) -> Option<u8> {
        if let Some(offset) = self.source_offset(address) {
            return rom.get(offset).copied();
        }
        let cell = self.cell(address)?;
        (self.valid[cell / 8] & (1 << (cell % 8)) != 0).then_some(self.ram[cell])
    }

    pub fn accepts_write(&self, address: u32, value: u8) -> bool {
        match address {
            0x0d01 | 0x0f01 => true,
            0x0e01 => self.locked || value & 0x20 == 0,
            _ => {
                self.cell(address).is_some()
                    || address & 1 != 0
                        && self.source_offset(address).is_some()
                        && (address >= 0x10000 || !matches!((address >> 8) & 0xf, 0xd..=0xf))
            }
        }
    }

    pub fn write(&mut self, address: u32, value: u8) -> bool {
        if !self.accepts_write(address, value) {
            return false;
        }
        match address {
            0x0d01 if !self.locked => self.save_enabled = value & 0x80 != 0,
            0x0e01 if !self.locked => {
                self.expanded = value & 0x40 != 0;
                self.locked = value & 0x80 == 0;
            }
            0x0f01 if !self.locked => self.bank = (value >> 4) & 7,
            0x0d01 | 0x0e01 | 0x0f01 => {}
            _ if self.cell(address).is_some() => {
                let cell = self.cell(address).unwrap();
                self.ram[cell] = value;
                self.valid[cell / 8] |= 1 << (cell % 8);
            }
            _ => {}
        }
        true
    }

    pub fn power_on(&mut self) {
        self.bank = 0;
        self.expanded = false;
        self.save_enabled = false;
        self.locked = false;
    }

    pub fn save_bytes(&self) -> Option<&[u8]> {
        self.valid.iter().all(|v| *v == 0xff).then_some(&self.ram)
    }

    pub fn load_save(&mut self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() == SAVE_BYTES,
            "MD SF-004 save must contain 32768 cells"
        );
        self.ram.copy_from_slice(bytes);
        self.valid.fill(0xff);
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.bank < 8 && self.ram.len() == SAVE_BYTES && self.valid.len() == VALID_BYTES,
            "invalid MD SF-004 state geometry"
        );
        ensure!(
            self.ram
                .iter()
                .enumerate()
                .all(|(cell, byte)| self.valid[cell / 8] & (1 << (cell % 8)) != 0 || *byte == 0),
            "undefined MD SF-004 cell has data"
        );
        Ok(())
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_bytes(b"SF04");
        writer.write_u8(self.bank);
        writer.write_bool(self.expanded);
        writer.write_bool(self.save_enabled);
        writer.write_bool(self.locked);
        writer.write_u32(SAVE_BYTES as u32);
        writer.write_bytes(&self.ram);
        writer.write_bytes(&self.valid);
    }

    pub fn read_state(&mut self, reader: &mut StateReader<'_>) -> Result<()> {
        let mut tag = [0; 4];
        reader.read_exact(&mut tag)?;
        ensure!(&tag == b"SF04", "MD SF-004 owner tag mismatch");
        let mut next = Self::new();
        next.bank = reader.read_u8()?;
        next.expanded = reader.read_bool()?;
        next.save_enabled = reader.read_bool()?;
        next.locked = reader.read_bool()?;
        ensure!(
            reader.read_u32()? == SAVE_BYTES as u32,
            "invalid MD SF-004 cell count"
        );
        reader.read_exact(&mut next.ram)?;
        reader.read_exact(&mut next.valid)?;
        next.validate()?;
        *self = next;
        Ok(())
    }
}
