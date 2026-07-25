use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct Mapper242 {
    prg_rom: Vec<u8>,
    chr_ram: [u8; 0x2000],
    prg_ram: [u8; 0x2000],
    latch: u16,
    mirroring: Mirroring,
}

impl Mapper242 {
    pub fn new(prg_rom: Vec<u8>, _mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr_ram: [0; 0x2000],
            prg_ram: [0; 0x2000],
            latch: 0,
            mirroring: Mirroring::Vertical,
        }
    }

    fn prg_bank_count_16k(&self) -> usize {
        (self.prg_rom.len() / 0x4000).max(1)
    }

    fn map_prg_bank(&self, addr: u16) -> usize {
        let bank_count = self.prg_bank_count_16k();
        let s = self.latch & 0x0001 != 0;
        let p = (((self.latch >> 3) & 0x03) << 1) | ((self.latch >> 2) & 0x01);
        let outer = (self.latch >> 5) & 0x03;
        let o = self.latch & 0x0080 != 0;
        let l = self.latch & 0x0200 != 0;
        let chip = self.latch & 0x8000 == 0;

        let mut base = outer as usize * 8;
        if self.prg_rom.len() > 0x80000 && chip {
            base += 32;
        }

        let lower = addr < 0xC000;
        let inner = if o {
            if s {
                ((p as usize) & 0x06) | usize::from(!lower)
            } else {
                p as usize
            }
        } else if lower {
            if s { (p as usize) & 0x06 } else { p as usize }
        } else if l {
            7
        } else {
            0
        };

        (base + inner) % bank_count
    }

    fn update_mirroring(&mut self) {
        self.mirroring = if self.latch & 0x0002 == 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
    }
}

impl Mapper for Mapper242 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let bank = self.map_prg_bank(addr);
                let offset = (addr as usize) & 0x3FFF;
                self.prg_rom[(bank * 0x4000 + offset) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = val,
            0x8000..=0xFFFF => {
                self.latch = addr;
                self.update_mirroring();
            }
            _ => {}
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        self.chr_ram[(addr as usize) & 0x1FFF]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        self.chr_ram[(addr as usize) & 0x1FFF] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.chr_ram);
        w.write_bytes(&self.prg_ram);
        w.write_u16(self.latch);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.chr_ram)?;
        r.read_exact(&mut self.prg_ram)?;
        self.latch = r.read_u16()?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        Ok(())
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        Some(self.prg_ram.to_vec())
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let len = bytes.len().min(self.prg_ram.len());
        self.prg_ram[..len].copy_from_slice(&bytes[..len]);
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
    fn reset_maps_both_windows_to_bank_zero() {
        let mapper = Mapper242::new(prg_banks(32), Mirroring::Horizontal);

        assert_eq!(mapper.cpu_peek(0x8000), 0x00);
        assert_eq!(mapper.cpu_peek(0xC000), 0x00);
        assert_eq!(mapper.mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn switches_unrom_style_lower_bank_and_fixed_high_bank() {
        let mut mapper = Mapper242::new(prg_banks(32), Mirroring::Horizontal);

        mapper.cpu_write(0x822E, 0x00);

        assert_eq!(mapper.cpu_peek(0x8000), 0x0B);
        assert_eq!(mapper.cpu_peek(0xC000), 0x0F);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn supports_nrom_256_mode() {
        let mut mapper = Mapper242::new(prg_banks(32), Mirroring::Horizontal);

        mapper.cpu_write(0x8091, 0x00);

        assert_eq!(mapper.cpu_peek(0x8000), 0x04);
        assert_eq!(mapper.cpu_peek(0xC000), 0x05);
    }
}
