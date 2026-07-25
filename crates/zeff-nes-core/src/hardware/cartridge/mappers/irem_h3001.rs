use crate::hardware::cartridge::{Mapper, Mirroring};

pub struct IremH3001 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: [u8; 0x2000],
    mirroring: Mirroring,

    prg_bank_0: u8,
    prg_bank_1: u8,
    prg_mode: bool,
    chr_banks: [u8; 8],

    irq_reload: u16,
    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,
}

impl IremH3001 {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self {
            prg_rom,
            chr,
            prg_ram: [0; 0x2000],
            mirroring,
            prg_bank_0: 0,
            prg_bank_1: 1,
            prg_mode: false,
            chr_banks: [0, 1, 2, 3, 4, 5, 6, 7],
            irq_reload: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn prg_read_bank(&self, bank: usize, addr: u16) -> u8 {
        let bank = bank % self.prg_bank_count_8k();
        let offset = (addr as usize) & 0x1FFF;
        self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
    }

    fn chr_addr(&self, addr: u16) -> usize {
        let slot = ((addr as usize) >> 10) & 0x07;
        let bank = self.chr_banks[slot] as usize % self.chr_bank_count_1k();
        let offset = addr as usize & 0x03FF;
        (bank * 0x0400 + offset) % self.chr.len()
    }
}

impl Mapper for IremH3001 {
    fn cpu_peek(&self, addr: u16) -> u8 {
        let fixed_second_last = self.prg_bank_count_8k().saturating_sub(2);
        let fixed_last = self.prg_bank_count_8k().saturating_sub(1);

        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0x9FFF if self.prg_mode => self.prg_read_bank(fixed_second_last, addr),
            0x8000..=0x9FFF => self.prg_read_bank(self.prg_bank_0 as usize, addr),
            0xA000..=0xBFFF => self.prg_read_bank(self.prg_bank_1 as usize, addr),
            0xC000..=0xDFFF if self.prg_mode => self.prg_read_bank(self.prg_bank_0 as usize, addr),
            0xC000..=0xDFFF => self.prg_read_bank(fixed_second_last, addr),
            0xE000..=0xFFFF => self.prg_read_bank(fixed_last, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = val,
            0x8000 => self.prg_bank_0 = val & 0x3F,
            0x9000 => self.prg_mode = val & 0x80 != 0,
            0x9001 => {
                self.mirroring = match (val >> 6) & 0x03 {
                    0 => Mirroring::Vertical,
                    2 => Mirroring::Horizontal,
                    _ => Mirroring::SingleScreenLower,
                };
            }
            0x9003 => {
                self.irq_enabled = val & 0x80 != 0;
                self.irq_pending = false;
            }
            0x9004 => {
                self.irq_counter = self.irq_reload;
                self.irq_pending = false;
            }
            0x9005 => self.irq_reload = (self.irq_reload & 0x00FF) | ((val as u16) << 8),
            0x9006 => self.irq_reload = (self.irq_reload & 0xFF00) | val as u16,
            0xA000 => self.prg_bank_1 = val & 0x3F,
            0xB000..=0xB007 => self.chr_banks[(addr & 0x0007) as usize] = val,
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

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn clock_cpu(&mut self) {
        if !self.irq_enabled || self.irq_counter == 0 {
            return;
        }

        self.irq_counter -= 1;
        if self.irq_counter == 0 {
            self.irq_pending = true;
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));

        w.write_u8(self.prg_bank_0);
        w.write_u8(self.prg_bank_1);
        w.write_bool(self.prg_mode);
        w.write_bytes(&self.chr_banks);

        w.write_u16(self.irq_reload);
        w.write_u16(self.irq_counter);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending);

        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;

        self.prg_bank_0 = r.read_u8()? & 0x3F;
        self.prg_bank_1 = r.read_u8()? & 0x3F;
        self.prg_mode = r.read_bool()?;
        r.read_exact(&mut self.chr_banks)?;

        self.irq_reload = r.read_u16()?;
        self.irq_counter = r.read_u16()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending = r.read_bool()?;

        crate::save_state::read_chr_state(r, &mut self.chr, "Irem H-3001")?;
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
            chr.extend(vec![bank as u8; 0x0400]);
        }
        chr
    }

    #[test]
    fn starts_with_documented_prg_banks_and_switches_mode() {
        let mut mapper = IremH3001::new(prg_banks(64), chr_banks(16), Mirroring::Horizontal);

        assert_eq!(mapper.cpu_peek(0x8000), 0);
        assert_eq!(mapper.cpu_peek(0xA000), 1);
        assert_eq!(mapper.cpu_peek(0xC000), 0x3E);
        assert_eq!(mapper.cpu_peek(0xE000), 0x3F);

        mapper.cpu_write(0x8000, 0x05);
        mapper.cpu_write(0xA000, 0x06);
        mapper.cpu_write(0x9000, 0x80);

        assert_eq!(mapper.cpu_peek(0x8000), 0x3E);
        assert_eq!(mapper.cpu_peek(0xA000), 0x06);
        assert_eq!(mapper.cpu_peek(0xC000), 0x05);
    }

    #[test]
    fn switches_chr_and_mirroring() {
        let mut mapper = IremH3001::new(prg_banks(8), chr_banks(32), Mirroring::Horizontal);

        mapper.cpu_write(0xB004, 0x0A);
        mapper.cpu_write(0x9001, 0x80);

        assert_eq!(mapper.chr_read(0x1000), 0x0A);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);

        mapper.cpu_write(0x9001, 0x40);
        assert_eq!(mapper.mirroring(), Mirroring::SingleScreenLower);
    }

    #[test]
    fn irq_counts_down_once_loaded() {
        let mut mapper = IremH3001::new(prg_banks(8), chr_banks(8), Mirroring::Horizontal);

        mapper.cpu_write(0x9005, 0x00);
        mapper.cpu_write(0x9006, 0x03);
        mapper.cpu_write(0x9004, 0x00);
        mapper.cpu_write(0x9003, 0x80);

        mapper.clock_cpu();
        mapper.clock_cpu();
        assert!(!mapper.irq_pending());
        mapper.clock_cpu();
        assert!(mapper.irq_pending());

        mapper.cpu_write(0x9003, 0x00);
        assert!(!mapper.irq_pending());
    }
}
