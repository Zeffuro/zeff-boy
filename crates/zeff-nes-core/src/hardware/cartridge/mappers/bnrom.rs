use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Bnrom {
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirroring: Mirroring,
    prg_bank: u8,
}

impl Bnrom {
    pub fn new(prg_rom: Vec<u8>, chr_ram: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_ram,
            mirroring,
            prg_bank: 0,
        }
    }

    fn prg_bank_count(&self) -> usize {
        (self.prg_rom.len() / 0x8000).max(1)
    }

    fn prg_addr(&self, addr: u16) -> usize {
        let bank = self.prg_bank as usize % self.prg_bank_count();
        let offset = (addr - 0x8000) as usize;
        (bank * 0x8000 + offset) % self.prg_rom.len()
    }
}

impl Mapper for Bnrom {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
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
        if addr >= 0x8000 {
            // BNROM/BxROM boards have PRG-ROM bus conflicts: the ROM byte and
            // CPU write both drive the data bus, so the mapper sees their AND.
            self.prg_bank = val & self.cpu_peek(addr);
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr_ram.is_empty() {
            return 0;
        }
        self.chr_ram[addr as usize % self.chr_ram.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        let len = self.chr_ram.len();
        if len > 0 {
            self.chr_ram[addr as usize % len] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.prg_bank);
        crate::save_state::write_chr_state(w, &self.chr_ram);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.prg_bank = r.read_u8()?;
        crate::save_state::read_chr_state(r, &mut self.chr_ram, "BNROM")?;
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

    #[test]
    fn switches_32k_prg_bank_with_bus_conflict_safe_write() {
        let mut prg = prg_banks(&[0x00, 0x11, 0x22]);
        prg[0x1234] = 0x02;

        let mut mapper = Bnrom::new(prg, vec![0; 0x2000], Mirroring::Horizontal);

        assert_eq!(mapper.cpu_peek(0x9234), 0x02);
        mapper.cpu_write(0x9234, 0x02);
        assert_eq!(mapper.cpu_peek(0x8000), 0x22);
    }

    #[test]
    fn bus_conflict_ands_write_with_rom_byte() {
        let mut prg = prg_banks(&[0x00, 0x11, 0x22, 0x33]);
        prg[0] = 0x03;

        let mut mapper = Bnrom::new(prg, vec![0; 0x2000], Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0x02);
        assert_eq!(mapper.prg_bank, 0x02);

        mapper.cpu_write(0x8000, 0x01);
        assert_eq!(mapper.prg_bank, 0x00);
    }

    #[test]
    fn chr_ram_is_read_write() {
        let mut mapper = Bnrom::new(prg_banks(&[0x00]), vec![0; 0x2000], Mirroring::Vertical);

        mapper.chr_write(0x1234, 0xAB);
        assert_eq!(mapper.chr_read(0x1234), 0xAB);
        assert_eq!(mapper.chr_read(0x3234), 0xAB);
    }

    #[test]
    fn state_roundtrips_bank_and_chr_ram() {
        let mut mapper = Bnrom::new(
            prg_banks(&[0x00, 0x01]),
            vec![0; 0x2000],
            Mirroring::Vertical,
        );
        mapper.prg_bank = 1;
        mapper.chr_write(0x0100, 0x5A);

        let mut writer = StateWriter::new();
        mapper.write_state(&mut writer);
        let bytes = writer.into_bytes();

        let mut restored = Bnrom::new(
            prg_banks(&[0x00, 0x01]),
            vec![0; 0x2000],
            Mirroring::Vertical,
        );
        let mut reader = StateReader::new(&bytes);
        restored.read_state(&mut reader).unwrap();

        assert_eq!(restored.prg_bank, 1);
        assert_eq!(restored.chr_read(0x0100), 0x5A);
    }
}
