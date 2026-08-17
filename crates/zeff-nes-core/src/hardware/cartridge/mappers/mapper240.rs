use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Mapper240 {
    prg_rom: Vec<u8>,
    prg_ram: [u8; 0x2000],
    chr: Vec<u8>,
    mirroring: Mirroring,
    bank_select: u8,
}

impl Mapper240 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            prg_ram: [0; 0x2000],
            chr,
            mirroring,
            bank_select: 0,
        }
    }

    fn is_register(addr: u16) -> bool {
        (addr & 0xE800) == 0x4800 || (addr & 0xE100) == 0x4100
    }

    fn prg_bank_count_32k(&self) -> usize {
        (self.prg_rom.len() / 0x8000).max(1)
    }

    fn chr_bank_count_8k(&self) -> usize {
        (self.chr.len() / 0x2000).max(1)
    }

    fn prg_addr(&self, addr: u16) -> usize {
        let bank = usize::from(self.bank_select >> 4) % self.prg_bank_count_32k();
        let offset = addr as usize - 0x8000;
        (bank * 0x8000 + offset) % self.prg_rom.len()
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let bank = usize::from(self.bank_select & 0x0F) % self.chr_bank_count_8k();
        (bank * 0x2000 + addr as usize) % self.chr.len()
    }
}

impl Mapper for Mapper240 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[addr as usize & 0x1FFF],
            0x8000..=0xFFFF => self.prg_rom[self.prg_addr(addr)],
            _ => 0,
        }
    }

    fn cpu_rom_offset(&self, addr: u16) -> Option<usize> {
        (addr >= 0x8000).then(|| self.prg_addr(addr))
    }

    fn rom_mapping_token(&self) -> u64 {
        u64::from(self.bank_select >> 4)
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x4020..=0x5FFF if Self::is_register(addr) => self.bank_select = val,
            0x6000..=0x7FFF => self.prg_ram[addr as usize & 0x1FFF] = val,
            _ => {}
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
        w.write_bytes(&self.prg_ram);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.bank_select = r.read_u8()?;
        r.read_exact(&mut self.prg_ram)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Mapper 240")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn switches_prg_chr_from_either_decode() {
        let mut mapper = Mapper240::new(
            prg_banks(&[0, 1, 2, 3]),
            chr_banks(&[4, 5, 6, 7]),
            Mirroring::Horizontal,
        );

        mapper.cpu_write(0x4800, 0x21);
        assert_eq!(mapper.cpu_peek(0x8000), 2);
        assert_eq!(mapper.chr_read(0x0100), 5);

        mapper.cpu_write(0x4100, 0x32);
        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.chr_read(0x0100), 6);
    }

    #[test]
    fn reports_active_prg_rom_offsets() {
        let mut mapper = Mapper240::new(
            prg_banks(&[0, 1, 2, 3]),
            chr_banks(&[0]),
            Mirroring::Horizontal,
        );
        mapper.cpu_write(0x4800, 0x20);
        assert_eq!(mapper.cpu_rom_offset(0x8123), Some(2 * 0x8000 + 0x123));
    }
}
