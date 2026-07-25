use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct G0151 {
    prg_rom: Vec<u8>,
    prg_ram: [u8; 0x0800],
    chr: Vec<u8>,
    mirroring: Mirroring,
    prg_banks: [u8; 4],
    chr_banks: [u8; 4],
}

impl G0151 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            prg_ram: [0; 0x0800],
            chr,
            mirroring,
            prg_banks: [0, 1, 2, 0xFF],
            chr_banks: [0, 1, 2, 3],
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_2k(&self) -> usize {
        (self.chr.len() / 0x0800).max(1)
    }

    fn is_vector_forced(addr: u16) -> bool {
        matches!(
            addr,
            0xFFE4..=0xFFE7 | 0xFFEC..=0xFFEF | 0xFFF4..=0xFFF7 | 0xFFFC..=0xFFFF
        )
    }

    fn prg_bank(&self, addr: u16) -> usize {
        let slot = usize::from((addr - 0x8000) / 0x2000);
        let mut bank = self.prg_banks[slot];
        if slot == 3 && Self::is_vector_forced(addr) {
            bank |= 0x10;
        }
        usize::from(bank) % self.prg_bank_count_8k()
    }

    fn chr_bank(&self, addr: u16) -> usize {
        let slot = usize::from(addr / 0x0800);
        usize::from(self.chr_banks[slot]) % self.chr_bank_count_2k()
    }
}

impl Mapper for G0151 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6800..=0x6FFF => self.prg_ram[addr as usize & 0x07FF],
            0x8000..=0xFFFF => {
                let bank = self.prg_bank(addr);
                let offset = addr as usize & 0x1FFF;
                self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x6003 => self.prg_banks[usize::from(addr & 0x0003)] = val & 0x3F,
            0x6004..=0x6007 => self.chr_banks[usize::from(addr & 0x0003)] = val,
            0x6800..=0x6FFF => self.prg_ram[addr as usize & 0x07FF] = val,
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let bank = self.chr_bank(addr);
        let offset = addr as usize & 0x07FF;
        self.chr[(bank * 0x0800 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let bank = self.chr_bank(addr);
        let offset = addr as usize & 0x07FF;
        let idx = (bank * 0x0800 + offset) % self.chr.len();
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_bytes(&self.prg_banks);
        w.write_bytes(&self.chr_banks);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        r.read_exact(&mut self.prg_banks)?;
        r.read_exact(&mut self.chr_banks)?;
        for bank in &mut self.prg_banks {
            *bank &= 0x3F;
        }
        crate::save_state::read_chr_state(r, &mut self.chr, "G0151")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks(count: usize) -> Vec<u8> {
        let mut prg = Vec::new();
        for bank in 0..count {
            prg.extend(vec![bank as u8; 0x2000]);
        }
        prg
    }

    fn chr_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x0800]);
        }
        chr
    }

    #[test]
    fn switches_prg_and_chr_banks() {
        let mut mapper = G0151::new(prg_banks(64), chr_banks(64), Mirroring::Horizontal);

        mapper.cpu_write(0x6000, 3);
        mapper.cpu_write(0x6001, 4);
        mapper.cpu_write(0x6002, 5);
        mapper.cpu_write(0x6003, 6);
        mapper.cpu_write(0x6004, 9);

        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xA000), 4);
        assert_eq!(mapper.cpu_peek(0xC000), 5);
        assert_eq!(mapper.cpu_peek(0xE000), 6);
        assert_eq!(mapper.chr_read(0x0100), 9);
    }

    #[test]
    fn forces_high_prg_bit_for_vector_reads() {
        let mut mapper = G0151::new(prg_banks(64), chr_banks(4), Mirroring::Horizontal);
        mapper.cpu_write(0x6003, 6);

        assert_eq!(mapper.cpu_peek(0xFFF0), 6);
        assert_eq!(mapper.cpu_peek(0xFFFC), 0x16);
    }
}
