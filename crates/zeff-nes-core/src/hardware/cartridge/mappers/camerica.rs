use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Camerica {
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    mirroring: Mirroring,
    prg_bank: u8,
}

impl Camerica {
    pub fn new(prg_rom: Vec<u8>, chr_ram: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_ram,
            mirroring,
            prg_bank: 0,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }
}

impl Mapper for Camerica {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.prg_bank as usize % self.prg_bank_count_16k();
                let offset = (addr - 0x8000) as usize;
                self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
            }
            0xC000..=0xFFFF => {
                let bank = self.prg_bank_count_16k() - 1;
                let offset = (addr - 0xC000) as usize;
                self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0xC000 {
            self.prg_bank = val;
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
        crate::save_state::read_chr_state(r, &mut self.chr_ram, "Camerica")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks(values: &[u8]) -> Vec<u8> {
        let mut prg = Vec::new();
        for &value in values {
            prg.extend(vec![value; 0x4000]);
        }
        prg
    }

    #[test]
    fn switches_lower_16k_prg_and_keeps_last_bank_fixed() {
        let mut mapper = Camerica::new(
            prg_banks(&[0x00, 0x11, 0x22, 0x33]),
            vec![0; 0x2000],
            Mirroring::Vertical,
        );

        assert_eq!(mapper.cpu_peek(0x8000), 0x00);
        assert_eq!(mapper.cpu_peek(0xC000), 0x33);
        mapper.cpu_write(0xC000, 0x02);
        assert_eq!(mapper.cpu_peek(0x8000), 0x22);
        assert_eq!(mapper.cpu_peek(0xC000), 0x33);
    }

    #[test]
    fn chr_ram_is_read_write() {
        let mut mapper = Camerica::new(prg_banks(&[0x00]), vec![0; 0x2000], Mirroring::Vertical);

        mapper.chr_write(0x0123, 0xA5);
        assert_eq!(mapper.chr_read(0x0123), 0xA5);
    }
}
