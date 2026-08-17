use anyhow::bail;

use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Nina001 {
    prg_rom: Vec<u8>,
    prg_ram: Vec<u8>,
    chr: Vec<u8>,
    prg_bank: u8,
    chr_bank_0: u8,
    chr_bank_1: u8,
    has_battery: bool,
}

impl Nina001 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, prg_ram_size: usize, has_battery: bool) -> Self {
        let ram_len = if prg_ram_size == 0 {
            0x2000
        } else {
            prg_ram_size
        };

        Self {
            prg_rom,
            prg_ram: vec![0; ram_len],
            chr,
            prg_bank: 0,
            chr_bank_0: 0,
            chr_bank_1: 0,
            has_battery,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / 0x8000).max(1)
    }

    fn chr_bank_count_4k(&self) -> usize {
        (self.chr.len() / 0x1000).max(1)
    }

    fn prg_addr(&self, addr: u16) -> usize {
        let bank = self.prg_bank as usize % self.prg_bank_count();
        let offset = (addr - 0x8000) as usize;
        (bank * 0x8000 + offset) % self.prg_rom.len()
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let (bank, offset) = if addr < 0x1000 {
            (self.chr_bank_0, addr as usize)
        } else {
            (self.chr_bank_1, (addr as usize - 0x1000) & 0x0FFF)
        };
        let bank = bank as usize % self.chr_bank_count_4k();
        (bank * 0x1000 + offset) % self.chr.len()
    }
}

impl Mapper for Nina001 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF if !self.prg_ram.is_empty() => {
                self.prg_ram[(addr as usize - 0x6000) % self.prg_ram.len()]
            }
            0x8000..=0xFFFF => self.prg_rom[self.prg_addr(addr)],
            _ => 0,
        }
    }

    fn cpu_rom_offset(&self, addr: u16) -> Option<usize> {
        (0x8000..=0xFFFF)
            .contains(&addr)
            .then(|| self.prg_addr(addr))
    }

    fn rom_mapping_token(&self) -> u64 {
        u64::from(self.prg_bank)
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if let 0x6000..=0x7FFF = addr {
            if !self.prg_ram.is_empty() {
                let idx = (addr as usize - 0x6000) % self.prg_ram.len();
                self.prg_ram[idx] = val;
            }

            match addr {
                0x7FFD => self.prg_bank = val & 0x01,
                0x7FFE => self.chr_bank_0 = val & 0x0F,
                0x7FFF => self.chr_bank_1 = val & 0x0F,
                _ => {}
            }
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        self.chr[self.chr_addr(addr)]
    }

    fn chr_write(&mut self, _addr: u16, _val: u8) {}

    fn mirroring(&self) -> Mirroring {
        Mirroring::Vertical
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        if self.has_battery && !self.prg_ram.is_empty() {
            Some(self.prg_ram.clone())
        } else {
            None
        }
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if self.prg_ram.is_empty() {
            return Ok(());
        }

        let copy_len = self.prg_ram.len().min(bytes.len());
        self.prg_ram[..copy_len].copy_from_slice(&bytes[..copy_len]);
        if copy_len < self.prg_ram.len() {
            self.prg_ram[copy_len..].fill(0);
        }
        Ok(())
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.prg_bank);
        w.write_u8(self.chr_bank_0);
        w.write_u8(self.chr_bank_1);
        w.write_bool(self.has_battery);
        w.write_vec(&self.prg_ram);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.prg_bank = r.read_u8()? & 0x01;
        self.chr_bank_0 = r.read_u8()? & 0x0F;
        self.chr_bank_1 = r.read_u8()? & 0x0F;
        self.has_battery = r.read_bool()?;

        let prg_ram = r.read_vec(64 * 1024)?;
        if prg_ram.len() != self.prg_ram.len() {
            bail!(
                "NINA-001 PRG RAM size mismatch: expected {}, got {}",
                self.prg_ram.len(),
                prg_ram.len()
            );
        }
        self.prg_ram = prg_ram;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_emu_common::save_state::{StateReader, StateWriter};

    fn prg_banks(values: &[u8]) -> Vec<u8> {
        let mut prg = Vec::new();
        for &value in values {
            prg.extend(vec![value; 0x8000]);
        }
        prg
    }

    fn chr_banks(values: &[u8]) -> Vec<u8> {
        let mut chr = Vec::new();
        for &value in values {
            chr.extend(vec![value; 0x1000]);
        }
        chr
    }

    #[test]
    fn switches_32k_prg_bank_at_7ffd() {
        let mut mapper = Nina001::new(prg_banks(&[0x10, 0x20]), chr_banks(&[0; 16]), 0, false);

        assert_eq!(mapper.cpu_peek(0x8000), 0x10);
        mapper.cpu_write(0x7FFD, 0x01);
        assert_eq!(mapper.cpu_peek(0x8000), 0x20);
    }

    #[test]
    fn switches_two_4k_chr_banks_at_7ffe_7fff() {
        let mut mapper = Nina001::new(
            prg_banks(&[0x10, 0x20]),
            chr_banks(&[0x00, 0x11, 0x22, 0x33]),
            0,
            false,
        );

        mapper.cpu_write(0x7FFE, 0x02);
        mapper.cpu_write(0x7FFF, 0x03);

        assert_eq!(mapper.chr_read(0x0000), 0x22);
        assert_eq!(mapper.chr_read(0x1000), 0x33);
    }

    #[test]
    fn register_writes_also_hit_prg_ram() {
        let mut mapper = Nina001::new(prg_banks(&[0x10, 0x20]), chr_banks(&[0; 16]), 0, false);

        mapper.cpu_write(0x7FFE, 0x05);
        assert_eq!(mapper.cpu_peek(0x7FFE), 0x05);
    }

    #[test]
    fn state_roundtrips_registers_and_prg_ram() {
        let mut mapper = Nina001::new(prg_banks(&[0x10, 0x20]), chr_banks(&[0; 16]), 0, true);
        mapper.cpu_write(0x7FFD, 0x01);
        mapper.cpu_write(0x7FFE, 0x03);
        mapper.cpu_write(0x6001, 0xA5);

        let mut writer = StateWriter::new();
        mapper.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut restored = Nina001::new(prg_banks(&[0x10, 0x20]), chr_banks(&[0; 16]), 0, false);
        let mut reader = StateReader::new(&bytes);
        restored.read_state(&mut reader).unwrap();

        assert_eq!(restored.cpu_peek(0x8000), 0x20);
        assert_eq!(restored.chr_bank_0, 0x03);
        assert_eq!(restored.cpu_peek(0x6001), 0xA5);
        assert!(restored.has_battery);
    }
}
