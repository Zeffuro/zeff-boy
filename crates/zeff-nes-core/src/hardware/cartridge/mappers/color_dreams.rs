use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct ColorDreams {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    bank_select: u8,
}

impl ColorDreams {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            bank_select: 0,
        }
    }

    fn prg_bank(&self) -> usize {
        usize::from(self.bank_select & 0x03)
    }

    fn chr_bank(&self) -> usize {
        usize::from(self.bank_select >> 4)
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / 0x8000).max(1)
    }

    fn chr_bank_count(&self) -> usize {
        (self.chr.len() / 0x2000).max(1)
    }

    fn prg_addr(&self, addr: u16) -> usize {
        let bank = self.prg_bank() % self.prg_bank_count();
        let offset = (addr - 0x8000) as usize;
        (bank * 0x8000 + offset) % self.prg_rom.len()
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let bank = self.chr_bank() % self.chr_bank_count();
        (bank * 0x2000 + addr as usize) % self.chr.len()
    }
}

impl Mapper for ColorDreams {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => self.prg_rom[self.prg_addr(addr)],
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.bank_select = val & self.cpu_peek(addr);
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        self.chr[self.chr_addr(addr)]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let idx = self.chr_addr(addr);
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.bank_select);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.bank_select = r.read_u8()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Color Dreams")?;
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
            chr.extend(vec![value; 0x2000]);
        }
        chr
    }

    #[test]
    fn switches_32k_prg_and_8k_chr_with_four_chr_bits() {
        let mut prg = prg_banks(&[0x00, 0x22, 0x44, 0x66]);
        prg[0] = 0x31;
        let mut chr_values = [0u8; 16];
        chr_values[3] = 0xEE;
        let chr = chr_banks(&chr_values);

        let mut mapper = ColorDreams::new(prg, chr, Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0x31);
        assert_eq!(mapper.cpu_peek(0x8000), 0x22);
        assert_eq!(mapper.chr_read(0x0000), 0xEE);
    }

    #[test]
    fn writes_are_subject_to_bus_conflicts() {
        let mut prg = prg_banks(&[0x00, 0x22, 0x44, 0x66]);
        prg[0] = 0x3F;
        let chr = chr_banks(&[0x00; 16]);

        let mut mapper = ColorDreams::new(prg, chr, Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0x31);
        assert_eq!(mapper.bank_select, 0x31);

        mapper.cpu_write(0x8000, 0x33);
        assert_eq!(mapper.bank_select, 0x33 & 0x22);
    }

    #[test]
    fn chr_ram_is_read_write_when_header_has_no_chr_rom() {
        let mut mapper = ColorDreams::new(prg_banks(&[0x00]), vec![0; 0x2000], Mirroring::Vertical);

        mapper.chr_write(0x0400, 0xC7);
        assert_eq!(mapper.chr_read(0x0400), 0xC7);
    }

    #[test]
    fn state_roundtrips_bank_and_chr() {
        let mut mapper = ColorDreams::new(
            prg_banks(&[0x00, 0x22]),
            chr_banks(&[0x00, 0xBB]),
            Mirroring::Horizontal,
        );
        mapper.bank_select = 0x11;
        mapper.chr_write(0x0100, 0xA5);

        let mut writer = StateWriter::new();
        mapper.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut restored = ColorDreams::new(
            prg_banks(&[0x00, 0x22]),
            chr_banks(&[0x00, 0xBB]),
            Mirroring::Horizontal,
        );
        let mut reader = StateReader::new(&bytes);
        restored.read_state(&mut reader).unwrap();

        assert_eq!(restored.bank_select, 0x11);
        assert_eq!(restored.chr_read(0x0100), 0xA5);
    }
}
