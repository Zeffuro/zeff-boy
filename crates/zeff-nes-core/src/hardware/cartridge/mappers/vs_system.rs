use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct VsSystem {
    prg_rom: Vec<u8>,
    prg_ram: [u8; 0x0800],
    chr: Vec<u8>,
    bank_select: u8,
}

impl VsSystem {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>) -> Self {
        Self {
            prg_rom,
            prg_ram: [0; 0x0800],
            chr,
            bank_select: 0,
        }
    }

    fn prg_8k_bank_count(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn prg_index(&self, bank: usize, offset: usize) -> Option<usize> {
        if bank >= self.prg_8k_bank_count() {
            return None;
        }
        Some((bank * 0x2000 + offset) % self.prg_rom.len())
    }

    fn low_prg_bank(&self) -> Option<usize> {
        if self.bank_select != 0 && self.prg_8k_bank_count() > 4 {
            Some(4)
        } else {
            Some(0)
        }
    }

    fn chr_bank_count(&self) -> usize {
        self.chr.len() / 0x2000
    }

    fn chr_index(&self, addr: u16) -> Option<usize> {
        let bank = usize::from(self.bank_select);
        if bank >= self.chr_bank_count() {
            return None;
        }
        Some(bank * 0x2000 + addr as usize)
    }
}

impl Mapper for VsSystem {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr as usize - 0x6000) & 0x07FF],
            0x8000..=0x9FFF => self
                .low_prg_bank()
                .and_then(|bank| self.prg_index(bank, (addr as usize - 0x8000) & 0x1FFF))
                .map(|idx| self.prg_rom[idx])
                .unwrap_or(0),
            0xA000..=0xFFFF => {
                let slot = usize::from((addr - 0xA000) / 0x2000);
                let offset = usize::from(addr & 0x1FFF);
                self.prg_index(1 + slot, offset)
                    .map(|idx| self.prg_rom[idx])
                    .unwrap_or(0)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x4016 => self.bank_select = (val >> 2) & 0x01,
            0x6000..=0x7FFF => {
                self.prg_ram[(addr as usize - 0x6000) & 0x07FF] = val;
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.chr_index(addr).map(|idx| self.chr[idx]).unwrap_or(0)
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if let Some(idx) = self.chr_index(addr) {
            self.chr[idx] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        Mirroring::FourScreen
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.bank_select);
        w.write_bytes(&self.prg_ram);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.bank_select = r.read_u8()? & 0x01;
        r.read_exact(&mut self.prg_ram)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Vs. System")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zeff_emu_common::save_state::{StateReader, StateWriter};

    fn prg_8k_banks(values: &[u8]) -> Vec<u8> {
        let mut prg = Vec::new();
        for &value in values {
            prg.extend(vec![value; 0x2000]);
        }
        prg
    }

    fn chr_banks(values: &[u8]) -> Vec<u8> {
        let mut chr = Vec::new();
        for &value in values {
            chr.extend(vec![value; 0x2000]);
        }
        chr
    }

    #[test]
    fn switches_chr_from_controller_port_bit_two() {
        let mut mapper = VsSystem::new(
            prg_8k_banks(&[0x00, 0x11, 0x22, 0x33]),
            chr_banks(&[0x44, 0x55]),
        );

        assert_eq!(mapper.cpu_peek(0x8000), 0x00);
        assert_eq!(mapper.cpu_peek(0xA000), 0x11);
        assert_eq!(mapper.cpu_peek(0xE000), 0x33);
        assert_eq!(mapper.chr_read(0x0123), 0x44);

        mapper.cpu_write(0x4016, 0x04);

        assert_eq!(mapper.cpu_peek(0x8000), 0x00);
        assert_eq!(mapper.chr_read(0x0123), 0x55);
        assert_eq!(mapper.mirroring(), Mirroring::FourScreen);
    }

    #[test]
    fn supports_gumshoe_style_alternate_low_prg_bank_when_present() {
        let mut mapper = VsSystem::new(
            prg_8k_banks(&[0x00, 0x11, 0x22, 0x33, 0xAA]),
            chr_banks(&[0x44, 0x55]),
        );

        assert_eq!(mapper.cpu_peek(0x8000), 0x00);
        mapper.cpu_write(0x4016, 0x04);
        assert_eq!(mapper.cpu_peek(0x8000), 0xAA);
        assert_eq!(mapper.cpu_peek(0xA000), 0x11);
    }

    #[test]
    fn prg_ram_and_state_roundtrip() {
        let mut mapper = VsSystem::new(
            prg_8k_banks(&[0x00, 0x11, 0x22, 0x33]),
            chr_banks(&[0x44, 0x55]),
        );
        mapper.cpu_write(0x6002, 0x5A);
        mapper.cpu_write(0x4016, 0x04);
        mapper.chr_write(0x0100, 0xA5);

        let mut writer = StateWriter::new();
        mapper.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut restored = VsSystem::new(
            prg_8k_banks(&[0x00, 0x11, 0x22, 0x33]),
            chr_banks(&[0x44, 0x55]),
        );
        let mut reader = StateReader::new(&bytes);
        restored.read_state(&mut reader).unwrap();

        assert_eq!(restored.cpu_peek(0x6002), 0x5A);
        assert_eq!(restored.chr_read(0x0100), 0xA5);
    }
}
