use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Nina03 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    bank_select: u8,
    multicart: bool,
}

impl Nina03 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            bank_select: 0,
            multicart: false,
        }
    }

    pub fn new_multicart(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            bank_select: 0,
            multicart: true,
        }
    }

    fn prg_bank_count_32k(&self) -> usize {
        (self.prg_rom.len() / 0x8000).max(1)
    }

    fn prg_bank(&self) -> usize {
        let bank = if self.multicart {
            (self.bank_select >> 3) & 0x07
        } else {
            (self.bank_select >> 3) & 0x01
        };
        usize::from(bank) % self.prg_bank_count_32k()
    }

    fn chr_bank_count_8k(&self) -> usize {
        (self.chr.len() / 0x2000).max(1)
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let bank = if self.multicart {
            (self.bank_select & 0x07) | ((self.bank_select >> 3) & 0x08)
        } else {
            self.bank_select & 0x07
        };
        let bank = usize::from(bank) % self.chr_bank_count_8k();
        (bank * 0x2000 + addr as usize) % self.chr.len()
    }

    fn is_register_addr(addr: u16) -> bool {
        (addr & 0xE100) == 0x4100
    }
}

impl Mapper for Nina03 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank = self.prg_bank();
                let offset = (addr - 0x8000) as usize;
                self.prg_rom[(bank * 0x8000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if Self::is_register_addr(addr) {
            self.bank_select = if self.multicart { val } else { val & 0x0F };
            if self.multicart {
                self.mirroring = if val & 0x80 == 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
            }
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
        if self.multicart {
            w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        }
        w.write_u8(self.bank_select);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        if self.multicart {
            self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        }
        self.bank_select = if self.multicart {
            r.read_u8()?
        } else {
            r.read_u8()? & 0x0F
        };
        crate::save_state::read_chr_state(r, &mut self.chr, "NINA-03")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chr_banks(values: &[u8]) -> Vec<u8> {
        let mut chr = Vec::new();
        for &value in values {
            chr.extend(vec![value; 0x2000]);
        }
        chr
    }

    #[test]
    fn switches_32k_prg_and_8k_chr_bank_register() {
        let mut mapper = Nina03::new(
            [vec![0x88; 0x8000], vec![0x99; 0x8000]].concat(),
            chr_banks(&[0x00, 0x11, 0x22, 0x33]),
            Mirroring::Vertical,
        );

        assert_eq!(mapper.cpu_peek(0x8000), 0x88);
        assert_eq!(mapper.chr_read(0x0000), 0x00);
        mapper.cpu_write(0x4100, 0x0A);
        assert_eq!(mapper.cpu_peek(0x8000), 0x99);
        assert_eq!(mapper.chr_read(0x0000), 0x22);
    }

    #[test]
    fn register_addr_uses_documented_mirrors() {
        let mut mapper = Nina03::new(
            vec![0x88; 0x8000],
            chr_banks(&[0x00, 0x11, 0x22, 0x33]),
            Mirroring::Vertical,
        );

        mapper.cpu_write(0x5F00, 0x03);
        assert_eq!(mapper.chr_read(0x0000), 0x33);

        mapper.cpu_write(0x4000, 0x01);
        assert_eq!(mapper.chr_read(0x0000), 0x33);
    }

    #[test]
    fn multicart_uses_extended_prg_chr_and_mirroring_bits() {
        let mut mapper = Nina03::new_multicart(
            [vec![0x00; 0x8000], vec![0x11; 0x8000], vec![0x22; 0x8000]].concat(),
            chr_banks(&[
                0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
                0x0E, 0x0F,
            ]),
            Mirroring::Horizontal,
        );

        mapper.cpu_write(0x4100, 0xD2);

        assert_eq!(mapper.cpu_peek(0x8000), 0x22);
        assert_eq!(mapper.chr_read(0x0000), 0x0A);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
    }
}
