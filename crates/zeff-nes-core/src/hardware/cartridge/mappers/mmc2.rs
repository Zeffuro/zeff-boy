use crate::hardware::cartridge::{Mapper, Mirroring};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mmc2Variant {
    Mmc2,
    Mmc4,
}

pub struct Mmc2 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,
    fixed_four_screen: bool,
    variant: Mmc2Variant,
    prg_bank: u8,
    chr_banks: [u8; 4],
    chr_latches: [u8; 2],
}

impl Mmc2 {
    pub fn new_mmc2(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self::new(prg_rom, chr, mirroring, Mmc2Variant::Mmc2)
    }

    pub fn new_mmc4(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self::new(prg_rom, chr, mirroring, Mmc2Variant::Mmc4)
    }

    fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring, variant: Mmc2Variant) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            fixed_four_screen: matches!(mirroring, Mirroring::FourScreen),
            variant,
            prg_bank: 0,
            chr_banks: [0; 4],
            chr_latches: [1, 1],
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn prg_read_8k(&self, bank: usize, addr: u16, base: u16) -> u8 {
        let bank = bank % self.prg_bank_count_8k();
        let offset = (addr - base) as usize;
        self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
    }

    fn prg_read_16k(&self, bank: usize, addr: u16, base: u16) -> u8 {
        let bank = bank % self.prg_bank_count_16k();
        let offset = (addr - base) as usize;
        self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
    }

    fn chr_bank_count_4k(&self) -> usize {
        (self.chr.len() / 0x1000).max(1)
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let slot = ((addr as usize) >> 12) & 0x01;
        let latch = self.chr_latches[slot] as usize & 0x01;
        let bank = self.chr_banks[slot * 2 + latch] as usize % self.chr_bank_count_4k();
        let offset = addr as usize & 0x0FFF;
        (bank * 0x1000 + offset) % self.chr.len()
    }

    fn update_chr_latch_after_read(&mut self, addr: u16) {
        match (self.variant, addr & 0x1FFF) {
            (Mmc2Variant::Mmc2, 0x0FD8) => self.chr_latches[0] = 0,
            (Mmc2Variant::Mmc2, 0x0FE8) => self.chr_latches[0] = 1,
            (Mmc2Variant::Mmc2, 0x1FD8..=0x1FDF) => self.chr_latches[1] = 0,
            (Mmc2Variant::Mmc2, 0x1FE8..=0x1FEF) => self.chr_latches[1] = 1,

            (Mmc2Variant::Mmc4, 0x0FD8..=0x0FDF) => self.chr_latches[0] = 0,
            (Mmc2Variant::Mmc4, 0x0FE8..=0x0FEF) => self.chr_latches[0] = 1,
            (Mmc2Variant::Mmc4, 0x1FD8..=0x1FDF) => self.chr_latches[1] = 0,
            (Mmc2Variant::Mmc4, 0x1FE8..=0x1FEF) => self.chr_latches[1] = 1,
            _ => {}
        }
    }
}

impl Mapper for Mmc2 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match self.variant {
            Mmc2Variant::Mmc2 => match addr {
                0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
                0x8000..=0x9FFF => self.prg_read_8k(self.prg_bank as usize, addr, 0x8000),
                0xA000..=0xBFFF => {
                    self.prg_read_8k(self.prg_bank_count_8k().saturating_sub(3), addr, 0xA000)
                }
                0xC000..=0xDFFF => {
                    self.prg_read_8k(self.prg_bank_count_8k().saturating_sub(2), addr, 0xC000)
                }
                0xE000..=0xFFFF => {
                    self.prg_read_8k(self.prg_bank_count_8k().saturating_sub(1), addr, 0xE000)
                }
                _ => 0,
            },
            Mmc2Variant::Mmc4 => match addr {
                0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
                0x8000..=0xBFFF => self.prg_read_16k(self.prg_bank as usize, addr, 0x8000),
                0xC000..=0xFFFF => {
                    self.prg_read_16k(self.prg_bank_count_16k().saturating_sub(1), addr, 0xC000)
                }
                _ => 0,
            },
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => {
                self.prg_ram[(addr - 0x6000) as usize] = val;
            }
            0xA000..=0xAFFF => self.prg_bank = val & 0x0F,
            0xB000..=0xBFFF => self.chr_banks[0] = val & 0x1F,
            0xC000..=0xCFFF => self.chr_banks[1] = val & 0x1F,
            0xD000..=0xDFFF => self.chr_banks[2] = val & 0x1F,
            0xE000..=0xEFFF => self.chr_banks[3] = val & 0x1F,
            0xF000..=0xFFFF => {
                if !self.fixed_four_screen {
                    self.mirroring = if val & 0x01 == 0 {
                        Mirroring::Vertical
                    } else {
                        Mirroring::Horizontal
                    };
                }
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }

        let value = self.chr[self.chr_addr(addr)];
        self.update_chr_latch_after_read(addr);
        value
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }

        let idx = self.chr_addr(addr);
        self.chr[idx] = val;
        self.update_chr_latch_after_read(addr);
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u8(self.prg_bank);
        w.write_bytes(&self.chr_banks);
        w.write_bytes(&self.chr_latches);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.prg_bank = r.read_u8()? & 0x0F;
        r.read_exact(&mut self.chr_banks)?;
        r.read_exact(&mut self.chr_latches)?;

        for bank in &mut self.chr_banks {
            *bank &= 0x1F;
        }
        for latch in &mut self.chr_latches {
            *latch &= 0x01;
        }

        crate::save_state::read_chr_state(r, &mut self.chr, "MMC2/MMC4")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prg_8k_banks(count: usize) -> Vec<u8> {
        let mut prg = Vec::new();
        for bank in 0..count {
            prg.extend(vec![bank as u8; 0x2000]);
        }
        prg
    }

    fn prg_16k_banks(count: usize) -> Vec<u8> {
        let mut prg = Vec::new();
        for bank in 0..count {
            prg.extend(vec![bank as u8; 0x4000]);
        }
        prg
    }

    fn chr_4k_banks(count: usize) -> Vec<u8> {
        let mut chr = Vec::new();
        for bank in 0..count {
            chr.extend(vec![bank as u8; 0x1000]);
        }
        chr
    }

    #[test]
    fn mmc2_maps_switchable_8k_and_fixed_tail() {
        let mut mapper = Mmc2::new_mmc2(prg_8k_banks(8), chr_4k_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0xA000, 0x02);

        assert_eq!(mapper.cpu_peek(0x8000), 2);
        assert_eq!(mapper.cpu_peek(0xA000), 5);
        assert_eq!(mapper.cpu_peek(0xC000), 6);
        assert_eq!(mapper.cpu_peek(0xE000), 7);
    }

    #[test]
    fn mmc4_maps_switchable_16k_fixed_tail_and_prg_ram() {
        let mut mapper = Mmc2::new_mmc4(prg_16k_banks(8), chr_4k_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0xA000, 0x03);
        mapper.cpu_write(0x6000, 0xA5);

        assert_eq!(mapper.cpu_peek(0x6000), 0xA5);
        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xC000), 7);
    }

    #[test]
    fn mmc2_updates_chr_latches_after_triggering_read() {
        let mut mapper = Mmc2::new_mmc2(prg_8k_banks(8), chr_4k_banks(8), Mirroring::Vertical);
        mapper.cpu_write(0xB000, 0x01);
        mapper.cpu_write(0xC000, 0x02);
        mapper.cpu_write(0xD000, 0x03);
        mapper.cpu_write(0xE000, 0x04);

        assert_eq!(mapper.chr_read(0x0000), 2);
        assert_eq!(mapper.chr_read(0x0FD8), 2);
        assert_eq!(mapper.chr_read(0x0000), 1);
        assert_eq!(mapper.chr_read(0x0FE8), 1);
        assert_eq!(mapper.chr_read(0x0000), 2);

        assert_eq!(mapper.chr_read(0x1000), 4);
        assert_eq!(mapper.chr_read(0x1FD8), 4);
        assert_eq!(mapper.chr_read(0x1000), 3);
        assert_eq!(mapper.chr_read(0x1FE8), 3);
        assert_eq!(mapper.chr_read(0x1000), 4);
    }

    #[test]
    fn mmc2_low_table_latch_uses_single_address() {
        let mut mapper = Mmc2::new_mmc2(prg_8k_banks(8), chr_4k_banks(8), Mirroring::Vertical);
        mapper.cpu_write(0xB000, 0x01);
        mapper.cpu_write(0xC000, 0x02);

        assert_eq!(mapper.chr_read(0x0FD9), 2);
        assert_eq!(mapper.chr_read(0x0000), 2);
    }

    #[test]
    fn mmc4_low_table_latch_uses_full_tile_range() {
        let mut mapper = Mmc2::new_mmc4(prg_16k_banks(8), chr_4k_banks(8), Mirroring::Vertical);
        mapper.cpu_write(0xB000, 0x01);
        mapper.cpu_write(0xC000, 0x02);

        assert_eq!(mapper.chr_read(0x0FD9), 2);
        assert_eq!(mapper.chr_read(0x0000), 1);
    }

    #[test]
    fn switches_mirroring_unless_four_screen() {
        let mut mapper = Mmc2::new_mmc2(prg_8k_banks(8), chr_4k_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0xF000, 0x01);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
        mapper.cpu_write(0xF000, 0x00);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);

        let mut four_screen =
            Mmc2::new_mmc4(prg_16k_banks(8), chr_4k_banks(8), Mirroring::FourScreen);
        four_screen.cpu_write(0xF000, 0x01);
        assert_eq!(four_screen.mirroring(), Mirroring::FourScreen);
    }
}
