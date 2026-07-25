use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct FfeMapper8 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    bank_select: u8,
}

impl FfeMapper8 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            bank_select: 0,
        }
    }

    fn prg_bank_count_32k(&self) -> usize {
        (self.prg_rom.len() / 0x8000).max(1)
    }

    fn chr_bank_count_8k(&self) -> usize {
        (self.chr.len() / 0x2000).max(1)
    }
}

impl Mapper for FfeMapper8 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank = ((self.bank_select >> 4) & 0x03) as usize % self.prg_bank_count_32k();
                let offset = (addr - 0x8000) as usize;
                self.prg_rom[(bank * 0x8000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.bank_select = val;
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let bank = (self.bank_select & 0x03) as usize % self.chr_bank_count_8k();
        self.chr[(bank * 0x2000 + addr as usize) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let bank = (self.bank_select & 0x03) as usize % self.chr_bank_count_8k();
        let idx = (bank * 0x2000 + addr as usize) % self.chr.len();
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
        crate::save_state::read_chr_state(r, &mut self.chr, "FFE mapper 8")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_banks(count: usize) -> Vec<u8> {
        let mut prg = Vec::new();
        for bank in 0..count {
            prg.extend(vec![bank as u8; 0x8000]);
        }
        prg
    }

    fn chr_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x2000]);
        }
        chr
    }

    #[test]
    fn switches_gnrom_style_without_bus_conflicts() {
        let mut mapper = FfeMapper8::new(prg_banks(4), chr_banks(4), Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x21);
        assert_eq!(mapper.cpu_peek(0x8000), 0x02);
        assert_eq!(mapper.chr_read(0x0000), 0x01);
    }
}
