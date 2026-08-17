use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Cprom {
    prg_rom: Vec<u8>,
    chr_ram: Vec<u8>,
    chr_bank: u8,
}

impl Cprom {
    const CHR_RAM_SIZE: usize = 0x4000;
    const CHR_BANK_SIZE: usize = 0x1000;

    pub fn new(prg_rom: Vec<u8>) -> Self {
        Self {
            prg_rom,
            chr_ram: vec![0; Self::CHR_RAM_SIZE],
            chr_bank: 0,
        }
    }

    fn prg_addr(&self, addr: u16) -> usize {
        (addr - 0x8000) as usize % self.prg_rom.len()
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let offset = addr as usize & (Self::CHR_BANK_SIZE - 1);
        if addr < 0x1000 {
            offset
        } else {
            let bank = usize::from(self.chr_bank) % (Self::CHR_RAM_SIZE / Self::CHR_BANK_SIZE);
            bank * Self::CHR_BANK_SIZE + offset
        }
    }
}

impl Mapper for Cprom {
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

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.chr_bank = (val & self.cpu_peek(addr)) & 0x03;
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.chr_ram[self.chr_addr(addr)]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        let idx = self.chr_addr(addr);
        self.chr_ram[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        Mirroring::Vertical
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.chr_bank);
        crate::save_state::write_chr_state(w, &self.chr_ram);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.chr_bank = r.read_u8()? & 0x03;
        crate::save_state::read_chr_state(r, &mut self.chr_ram, "CPROM")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg(fill0: u8) -> Vec<u8> {
        let mut prg = vec![0xFF; 0x8000];
        prg[0] = fill0;
        prg
    }

    #[test]
    fn lower_chr_ram_is_fixed_and_upper_4k_is_banked() {
        let mut mapper = Cprom::new(prg(0xFF));

        mapper.chr_write(0x0200, 0x11);
        mapper.cpu_write(0x8000, 0x02);
        mapper.chr_write(0x1200, 0x44);

        assert_eq!(mapper.chr_read(0x0200), 0x11);
        assert_eq!(mapper.chr_read(0x1200), 0x44);
        mapper.cpu_write(0x8000, 0x00);
        assert_eq!(mapper.chr_read(0x1200), 0x11);
    }

    #[test]
    fn writes_are_subject_to_bus_conflicts() {
        let mut mapper = Cprom::new(prg(0x01));

        mapper.cpu_write(0x8000, 0x03);
        assert_eq!(mapper.chr_bank, 0x01);
    }
}
