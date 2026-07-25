use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct IremTamS1 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    prg_bank: u8,
}

impl IremTamS1 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            prg_bank: 0,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn read_prg_bank(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.prg_bank_count_16k();
        let offset = (addr as usize) & 0x3FFF;
        self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
    }
}

impl Mapper for IremTamS1 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.read_prg_bank(self.prg_bank_count_16k() - 1, addr),
            0xC000..=0xFFFF => self.read_prg_bank(self.prg_bank as usize, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if (0x8000..=0xBFFF).contains(&addr) {
            self.prg_bank = val & 0x1F;
            self.mirroring = if val & 0x80 == 0 {
                Mirroring::Horizontal
            } else {
                Mirroring::Vertical
            };
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        self.chr[addr as usize % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        let len = self.chr.len();
        if len > 0 {
            self.chr[addr as usize % len] = val;
        }
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u8(self.prg_bank);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.prg_bank = r.read_u8()? & 0x1F;
        crate::save_state::read_chr_state(r, &mut self.chr, "Irem TAM-S1")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks(count: usize) -> Vec<u8> {
        let mut prg = Vec::new();
        for bank in 0..count {
            prg.extend(vec![bank as u8; 0x4000]);
        }
        prg
    }

    #[test]
    fn switches_upper_16k_and_mirroring() {
        let mut mapper = IremTamS1::new(prg_banks(8), vec![0; 0x2000], Mirroring::Horizontal);

        mapper.cpu_write(0x8000, 0x84);

        assert_eq!(mapper.cpu_peek(0x8000), 0x07);
        assert_eq!(mapper.cpu_peek(0xC000), 0x04);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
    }
}
