use crate::hardware::cartridge::{Mapper, Mirroring, RomHeader};

pub struct Contra100In1 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,
    bank: u8,
    prg_a13: bool,
    mode: u8,
}

impl Contra100In1 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            bank: 0,
            prg_a13: false,
            mode: 0,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn read_prg_8k(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.prg_bank_count_8k();
        let offset = addr as usize & 0x1FFF;
        self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
    }

    fn cpu_a13(addr: u16) -> usize {
        ((addr >> 13) & 0x01) as usize
    }

    fn cpu_a14(addr: u16) -> usize {
        ((addr >> 14) & 0x01) as usize
    }

    fn selected_prg_8k_bank(&self, addr: u16) -> usize {
        let bank = self.bank as usize;
        match self.mode & 0x03 {
            0 => {
                let bank_16k = (bank & !0x01) | Self::cpu_a14(addr);
                bank_16k * 2 + Self::cpu_a13(addr)
            }
            1 => {
                let bank_16k = if addr & 0x4000 != 0 {
                    (bank & !0x07) | 0x07
                } else {
                    bank
                };
                bank_16k * 2 + Self::cpu_a13(addr)
            }
            2 => bank * 2 + usize::from(self.prg_a13),
            3 => bank * 2 + Self::cpu_a13(addr),
            _ => unreachable!(),
        }
    }
}

impl Mapper for Contra100In1 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => self.read_prg_8k(self.selected_prg_8k_bank(addr), addr),
            _ => 0,
        }
    }

    fn cpu_rom_offset(&self, addr: u16) -> Option<usize> {
        if addr < 0x8000 {
            return None;
        }
        let bank = self.selected_prg_8k_bank(addr) % self.prg_bank_count_8k();
        Some((bank * 0x2000 + (addr as usize & 0x1FFF)) % self.prg_rom.len())
    }

    fn rom_mapping_token(&self) -> u64 {
        u64::from(self.bank) | (u64::from(self.prg_a13) << 8) | (u64::from(self.mode) << 9)
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = val,
            0x8000..=0xFFFF => {
                self.mode = (addr & 0x0003) as u8;
                self.bank = val & 0x3F;
                self.prg_a13 = val & 0x80 != 0;
                self.mirroring = if val & 0x40 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        self.chr[addr as usize % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }
        let idx = addr as usize % self.chr.len();
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn load_trainer(&mut self, bytes: &[u8], _header: &RomHeader) -> anyhow::Result<()> {
        let start = 0x1000;
        let copy_len = bytes.len().min(self.prg_ram.len() - start);
        self.prg_ram[start..start + copy_len].copy_from_slice(&bytes[..copy_len]);
        Ok(())
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u8(self.bank);
        w.write_bool(self.prg_a13);
        w.write_u8(self.mode);
        w.write_bytes(&self.prg_ram);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.bank = r.read_u8()? & 0x3F;
        self.prg_a13 = r.read_bool()?;
        self.mode = r.read_u8()? & 0x03;
        r.read_exact(&mut self.prg_ram)?;
        crate::save_state::read_chr_state(r, &mut self.chr, "100-in-1 Contra Function 16")?;
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

    #[test]
    fn switches_nrom_256_mode() {
        let mut mapper = Contra100In1::new(prg_8k_banks(64), vec![0; 0x2000], Mirroring::Vertical);

        mapper.cpu_write(0x8000, 0x04);
        assert_eq!(mapper.cpu_peek(0x8000), 8);
        assert_eq!(mapper.cpu_peek(0xA000), 9);
        assert_eq!(mapper.cpu_peek(0xC000), 10);
        assert_eq!(mapper.cpu_peek(0xE000), 11);
    }

    #[test]
    fn switches_unrom_mode() {
        let mut mapper = Contra100In1::new(prg_8k_banks(64), vec![0; 0x2000], Mirroring::Vertical);

        mapper.cpu_write(0x8001, 0x04);
        assert_eq!(mapper.cpu_peek(0x8000), 8);
        assert_eq!(mapper.cpu_peek(0xA000), 9);
        assert_eq!(mapper.cpu_peek(0xC000), 14);
        assert_eq!(mapper.cpu_peek(0xE000), 15);
    }

    #[test]
    fn switches_nrom_64_mode() {
        let mut mapper = Contra100In1::new(prg_8k_banks(64), vec![0; 0x2000], Mirroring::Vertical);

        mapper.cpu_write(0x8002, 0x84);
        assert_eq!(mapper.cpu_peek(0x8000), 9);
        assert_eq!(mapper.cpu_peek(0xA000), 9);
        assert_eq!(mapper.cpu_peek(0xC000), 9);
        assert_eq!(mapper.cpu_peek(0xE000), 9);
    }

    #[test]
    fn switches_nrom_128_mode_and_mirroring() {
        let mut mapper = Contra100In1::new(prg_8k_banks(64), vec![0; 0x2000], Mirroring::Vertical);

        mapper.cpu_write(0x8003, 0x45);
        assert_eq!(mapper.cpu_peek(0x8000), 10);
        assert_eq!(mapper.cpu_peek(0xA000), 11);
        assert_eq!(mapper.cpu_peek(0xC000), 10);
        assert_eq!(mapper.cpu_peek(0xE000), 11);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn trainer_loads_at_7000_inside_prg_ram() {
        let mut mapper = Contra100In1::new(prg_8k_banks(64), vec![0; 0x2000], Mirroring::Vertical);
        let mut trainer = vec![0; 512];
        trainer[0] = 0xA9;
        trainer[0x1FF] = 0x60;
        let header = RomHeader::parse(&[
            b'N', b'E', b'S', 0x1A, 16, 0, 0xF4, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ])
        .unwrap();

        mapper.load_trainer(&trainer, &header).unwrap();

        assert_eq!(mapper.cpu_peek(0x7000), 0xA9);
        assert_eq!(mapper.cpu_peek(0x71FF), 0x60);
        assert_eq!(mapper.cpu_peek(0x6FFF), 0x00);
        assert_eq!(mapper.cpu_peek(0x7200), 0x00);
    }

    #[test]
    fn reports_active_prg_rom_offsets() {
        let mut mapper = Contra100In1::new(prg_8k_banks(64), vec![0; 0x2000], Mirroring::Vertical);
        mapper.cpu_write(0x8000, 4);
        assert_eq!(mapper.cpu_rom_offset(0x8123), Some(8 * 0x2000 + 0x123));
        mapper.cpu_write(0x8002, 0x84);
        assert_eq!(mapper.cpu_rom_offset(0xA123), Some(9 * 0x2000 + 0x123));
    }
}
