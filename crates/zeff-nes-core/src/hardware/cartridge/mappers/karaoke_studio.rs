use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct KaraokeStudio {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    mirroring: Mirroring,
    prg_bank: u8,
    internal_selected: bool,
}

impl KaraokeStudio {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            mirroring,
            prg_bank: 0,
            internal_selected: true,
        }
    }

    fn internal_len(&self) -> usize {
        if self.prg_rom.len() >= 0x40000 {
            0x20000
        } else {
            self.prg_rom.len()
        }
    }

    fn external_offset(&self) -> Option<usize> {
        (self.prg_rom.len() > self.internal_len()).then_some(self.internal_len())
    }

    fn read_16k_from_region(
        &self,
        region_start: usize,
        region_len: usize,
        bank: usize,
        addr: u16,
    ) -> u8 {
        let bank_count = (region_len / 0x4000).max(1);
        let bank = bank % bank_count;
        let offset = (addr as usize) & 0x3FFF;
        self.prg_rom[(region_start + bank * 0x4000 + offset) % self.prg_rom.len()]
    }
}

impl Mapper for KaraokeStudio {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => 0x03,
            0x8000..=0xBFFF if self.internal_selected => {
                self.read_16k_from_region(0, self.internal_len(), self.prg_bank as usize, addr)
            }
            0x8000..=0xBFFF => {
                if let Some(external_start) = self.external_offset() {
                    let external_len = self.prg_rom.len() - external_start;
                    self.read_16k_from_region(
                        external_start,
                        external_len,
                        self.prg_bank as usize,
                        addr,
                    )
                } else {
                    let last_internal = (self.internal_len() / 0x4000).saturating_sub(1);
                    self.read_16k_from_region(0, self.internal_len(), last_internal, addr)
                }
            }
            0xC000..=0xFFFF => {
                let last_internal = (self.internal_len() / 0x4000).saturating_sub(1);
                self.read_16k_from_region(0, self.internal_len(), last_internal, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        if addr >= 0x8000 {
            self.prg_bank = val & 0x0F;
            self.internal_selected = val & 0x10 != 0;
            self.mirroring = if val & 0x20 == 0 {
                Mirroring::Vertical
            } else {
                Mirroring::Horizontal
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
        w.write_bool(self.internal_selected);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.prg_bank = r.read_u8()? & 0x0F;
        self.internal_selected = r.read_bool()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "Karaoke Studio")?;
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
    fn switches_internal_and_external_rom_banks() {
        let mut mapper = KaraokeStudio::new(prg_banks(16), vec![0; 0x2000], Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x12);
        assert_eq!(mapper.cpu_peek(0x8000), 0x02);
        assert_eq!(mapper.cpu_peek(0xC000), 0x07);

        mapper.cpu_write(0x8000, 0x23);
        assert_eq!(mapper.cpu_peek(0x8000), 0x0B);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn absent_external_rom_falls_back_to_last_internal_bank() {
        let mut mapper = KaraokeStudio::new(prg_banks(8), vec![0; 0x2000], Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x00);
        assert_eq!(mapper.cpu_peek(0x8000), 0x07);
    }
}
