use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct SenjouNoOokami {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    bank_select: u8,
}

impl SenjouNoOokami {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            bank_select: 0,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }
}

impl Mapper for SenjouNoOokami {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let bank = self.bank_select as usize % self.prg_bank_count_16k();
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
        if addr >= 0x8000 {
            self.bank_select = (val >> 2) & 0x07;
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
        w.write_u8(self.bank_select);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.bank_select = r.read_u8()? & 0x07;
        crate::save_state::read_chr_state(r, &mut self.chr, "Senjou no Ookami")?;
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
    fn selects_16k_prg_bank_from_bits_2_to_4() {
        let mut mapper = SenjouNoOokami::new(prg_banks(8), vec![0; 0x2000], Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x14);
        assert_eq!(mapper.cpu_peek(0x8000), 0x05);
        assert_eq!(mapper.cpu_peek(0xC000), 0x07);
    }
}
