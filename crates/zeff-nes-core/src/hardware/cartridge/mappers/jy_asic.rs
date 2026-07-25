use crate::hardware::cartridge::{Mapper, Mirroring};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JyAsicVariant {
    Mapper90,
    Mapper209,
    Mapper211,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NametableSource {
    Ciram(usize),
    Rom(usize),
}

pub struct JyAsic {
    prg_rom: Vec<u8>,
    prg_ram: [u8; 0x2000],
    chr: Vec<u8>,
    variant: JyAsicVariant,
    mirroring: Mirroring,
    mirroring_select: u8,
    prg_regs: [u8; 4],
    chr_regs: [u16; 8],
    nt_regs: [u16; 4],
    mode: u8,
    ppu_config: u8,
    outer_bank: u8,
    multiplier: [u8; 2],
    accumulator: u8,
    test_reg: u8,
    irq_prescaler: u8,
    irq_counter: u8,
    irq_xor: u8,
    irq_prescaler_config: u8,
    irq_enabled: bool,
    irq_mode: u8,
    irq_pending: bool,
}

impl JyAsic {
    pub fn new(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self::with_variant(prg_rom, chr, mirroring, JyAsicVariant::Mapper90)
    }

    pub fn new_mapper209(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self::with_variant(prg_rom, chr, mirroring, JyAsicVariant::Mapper209)
    }

    pub fn new_mapper211(prg_rom: Vec<u8>, chr: Vec<u8>, mirroring: Mirroring) -> Self {
        Self::with_variant(prg_rom, chr, mirroring, JyAsicVariant::Mapper211)
    }

    fn with_variant(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        variant: JyAsicVariant,
    ) -> Self {
        Self {
            prg_rom,
            prg_ram: [0; 0x2000],
            chr,
            variant,
            mirroring,
            mirroring_select: 0,
            prg_regs: [0, 1, 2, 3],
            chr_regs: [0, 1, 2, 3, 4, 5, 6, 7],
            nt_regs: [0; 4],
            mode: 0,
            ppu_config: 0,
            outer_bank: 0,
            multiplier: [0; 2],
            accumulator: 0,
            test_reg: 0,
            irq_prescaler: 0,
            irq_counter: 0,
            irq_xor: 0,
            irq_prescaler_config: 0,
            irq_enabled: false,
            irq_mode: 0,
            irq_pending: false,
        }
    }

    fn prg_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn prg_mode(&self) -> u8 {
        self.mode & 0x03
    }

    fn switchable_last(&self) -> bool {
        self.mode & 0x04 != 0
    }

    fn map_prg_6000(&self) -> bool {
        self.mode & 0x80 != 0
    }

    fn reverse_7_bits(value: u8) -> u8 {
        let mut out = 0;
        for bit in 0..7 {
            out |= ((value >> bit) & 1) << (6 - bit);
        }
        out
    }

    fn prg_reg(&self, index: usize) -> usize {
        let value = self.prg_regs[index] & 0x7F;
        if self.prg_mode() == 3 {
            usize::from(Self::reverse_7_bits(value))
        } else {
            usize::from(value)
        }
    }

    fn prg_outer_window_8k(&self) -> usize {
        self.prg_bank_count_8k().min(64).max(1)
    }

    fn prg_outer_base(&self) -> usize {
        let outer_512k = usize::from((self.outer_bank >> 1) & 0x03) * 64;
        if self.prg_bank_count_8k() > 64 {
            outer_512k
        } else {
            0
        }
    }

    fn prg_reg_units(&self, index: usize, bank_size_8k: usize) -> usize {
        let banks = (self.prg_outer_window_8k() / bank_size_8k).max(1);
        self.prg_reg(index) & (banks - 1)
    }

    fn prg_bank(&self, addr: u16) -> usize {
        let count = self.prg_bank_count_8k();
        let outer = self.prg_outer_base();
        let slot = usize::from((addr - 0x8000) / 0x2000);
        let bank = match self.prg_mode() {
            0 => {
                let base = if self.switchable_last() {
                    self.prg_reg_units(3, 4) * 4
                } else {
                    self.prg_outer_window_8k().saturating_sub(4)
                };
                base + slot
            }
            1 => match slot {
                0 | 1 => self.prg_reg_units(1, 2) * 2 + slot,
                2 | 3 if self.switchable_last() => self.prg_reg_units(3, 2) * 2 + (slot - 2),
                _ => self.prg_outer_window_8k().saturating_sub(2) + (slot - 2),
            },
            _ => match slot {
                0 => self.prg_reg(0),
                1 => self.prg_reg(1),
                2 => self.prg_reg(2),
                _ if self.switchable_last() => self.prg_reg(3),
                _ => count.saturating_sub(1),
            },
        };
        (outer + bank) % count
    }

    fn prg_bank_6000(&self) -> usize {
        let count = self.prg_bank_count_8k();
        let bank = match self.prg_mode() {
            0 => (self.prg_reg(3) << 2) | 0x03,
            1 => (self.prg_reg(3) << 1) | 0x01,
            _ => self.prg_reg(3),
        };
        (self.prg_outer_base() + bank) % count
    }

    fn chr_mode(&self) -> u8 {
        (self.mode >> 3) & 0x03
    }

    fn chr_outer_base_1k(&self) -> usize {
        let count = self.chr_bank_count_1k();
        let base = if self.outer_bank & 0x20 != 0 {
            usize::from((self.outer_bank >> 3) & 0x03) * 512
        } else {
            usize::from((self.outer_bank & 0x01) | ((self.outer_bank & 0x18) >> 2)) * 256
        };
        if base < count { base } else { 0 }
    }

    fn chr_outer_window_1k(&self) -> usize {
        let window = if self.outer_bank & 0x20 != 0 {
            512
        } else {
            256
        };
        self.chr_bank_count_1k().min(window).max(1)
    }

    fn chr_reg(&self, index: usize) -> usize {
        usize::from(self.chr_regs[index])
    }

    fn chr_reg_units(&self, index: usize, bank_size_1k: usize) -> usize {
        let banks = (self.chr_outer_window_1k() / bank_size_1k).max(1);
        self.chr_reg(index) & (banks - 1)
    }

    fn chr_bank(&self, addr: u16) -> usize {
        let slot = usize::from(addr / 0x0400);
        let bank = match self.chr_mode() {
            0 => self.chr_reg_units(0, 8) * 8 + slot,
            1 => {
                if addr < 0x1000 {
                    self.chr_reg_units(0, 4) * 4 + slot
                } else {
                    self.chr_reg_units(4, 4) * 4 + (slot - 4)
                }
            }
            2 => {
                let reg = (slot / 2) * 2;
                self.chr_reg_units(reg, 2) * 2 + (slot & 1)
            }
            _ => self.chr_reg(slot),
        };
        (self.chr_outer_base_1k() + bank) % self.chr_bank_count_1k()
    }

    fn rom_nametable_bank(&self, table: usize) -> usize {
        usize::from(self.nt_regs[table]) % self.chr_bank_count_1k()
    }

    fn special_nametable_source(&self, addr: u16) -> Option<NametableSource> {
        if self.variant == JyAsicVariant::Mapper90 {
            return None;
        }

        let table = usize::from(((addr - 0x2000) & 0x0FFF) / 0x0400);
        let reg = self.nt_regs[table];
        let rom_nametables_enabled =
            self.mode & 0x20 != 0 || self.variant == JyAsicVariant::Mapper211;

        if rom_nametables_enabled {
            if self.mode & 0x40 != 0 {
                return Some(NametableSource::Rom(self.rom_nametable_bank(table)));
            }

            let ciram_selected = ((reg as u8 ^ self.ppu_config) & 0x80) == 0;
            return Some(if ciram_selected {
                NametableSource::Ciram(usize::from(reg & 0x01))
            } else {
                NametableSource::Rom(self.rom_nametable_bank(table))
            });
        }

        if self.mirroring_select & 0x08 != 0 {
            Some(NametableSource::Ciram(usize::from(reg & 0x01)))
        } else {
            None
        }
    }

    fn irq_prescaler_mask(&self) -> u8 {
        if self.irq_mode & 0x04 != 0 {
            0x07
        } else {
            0xFF
        }
    }

    fn disable_irq(&mut self) {
        self.irq_enabled = false;
        self.irq_pending = false;
        self.irq_prescaler = 0;
    }

    fn enable_irq(&mut self) {
        self.irq_enabled = true;
    }

    fn clock_irq_counter(&mut self) {
        let direction = self.irq_mode >> 6;
        match direction {
            1 => {
                self.irq_counter = self.irq_counter.wrapping_add(1);
                if self.irq_counter == 0 {
                    self.irq_pending = true;
                }
            }
            2 => {
                self.irq_counter = self.irq_counter.wrapping_sub(1);
                if self.irq_counter == 0xFF {
                    self.irq_pending = true;
                }
            }
            _ => {}
        }
    }

    fn clock_irq(&mut self) {
        if !self.irq_enabled {
            return;
        }
        let mask = self.irq_prescaler_mask();
        let direction = self.irq_mode >> 6;
        match direction {
            1 => {
                self.irq_prescaler = self.irq_prescaler.wrapping_add(1);
                if self.irq_prescaler & mask == 0 {
                    self.clock_irq_counter();
                }
            }
            2 => {
                self.irq_prescaler = self.irq_prescaler.wrapping_sub(1);
                if self.irq_prescaler & mask == mask {
                    self.clock_irq_counter();
                }
            }
            _ => {}
        }
    }

    fn clock_irq_times(&mut self, clocks: usize) {
        for _ in 0..clocks {
            self.clock_irq();
        }
    }
}

impl Mapper for JyAsic {
    fn cpu_peek(&self, addr: u16) -> u8 {
        match addr {
            0x5000 | 0x5400 | 0x5C00 => 0,
            0x5800 if (addr & 0xF803) == 0x5800 => {
                u16::from(self.multiplier[0]).wrapping_mul(u16::from(self.multiplier[1])) as u8
            }
            0x5801 if (addr & 0xF803) == 0x5801 => {
                (u16::from(self.multiplier[0]).wrapping_mul(u16::from(self.multiplier[1])) >> 8)
                    as u8
            }
            0x5802 if (addr & 0xF803) == 0x5802 => self.accumulator,
            0x5803 if (addr & 0xF803) == 0x5803 => self.test_reg,
            0x6000..=0x7FFF if self.map_prg_6000() => {
                let bank = self.prg_bank_6000();
                let offset = addr as usize & 0x1FFF;
                self.prg_rom[(bank * 0x2000 + offset) % self.prg_rom.len()]
            }
            0x6000..=0x7FFF => self.prg_ram[addr as usize & 0x1FFF],
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
            _ if (addr & 0xF803) == 0x5800 => self.multiplier[0] = val,
            _ if (addr & 0xF803) == 0x5801 => self.multiplier[1] = val,
            _ if (addr & 0xF803) == 0x5802 => {
                self.accumulator = self.accumulator.wrapping_add(val);
            }
            _ if (addr & 0xF803) == 0x5803 => {
                self.accumulator = 0;
                self.test_reg = val;
            }
            0x6000..=0x7FFF if !self.map_prg_6000() => {
                self.prg_ram[addr as usize & 0x1FFF] = val;
            }
            _ if (addr & 0xF803) == 0x8000 => self.prg_regs[0] = val & 0x7F,
            _ if (addr & 0xF803) == 0x8001 => self.prg_regs[1] = val & 0x7F,
            _ if (addr & 0xF803) == 0x8002 => self.prg_regs[2] = val & 0x7F,
            _ if (addr & 0xF803) == 0x8003 => self.prg_regs[3] = val & 0x7F,
            _ if (addr & 0xF800) == 0x9000 => {
                let reg = usize::from(addr & 0x0007);
                self.chr_regs[reg] = (self.chr_regs[reg] & 0xFF00) | u16::from(val);
            }
            _ if (addr & 0xF800) == 0xA000 => {
                let reg = usize::from(addr & 0x0007);
                self.chr_regs[reg] = (self.chr_regs[reg] & 0x00FF) | (u16::from(val) << 8);
            }
            _ if (addr & 0xF800) == 0xB000 => {
                let reg = usize::from(addr & 0x0003);
                if addr & 0x0004 == 0 {
                    self.nt_regs[reg] = (self.nt_regs[reg] & 0xFF00) | u16::from(val);
                } else {
                    self.nt_regs[reg] = (self.nt_regs[reg] & 0x00FF) | (u16::from(val) << 8);
                }
            }
            _ if (addr & 0xF007) == 0xC000 => {
                if val & 0x01 != 0 {
                    self.enable_irq();
                } else {
                    self.disable_irq();
                }
            }
            _ if (addr & 0xF007) == 0xC001 => self.irq_mode = val,
            _ if (addr & 0xF007) == 0xC002 => self.disable_irq(),
            _ if (addr & 0xF007) == 0xC003 => self.enable_irq(),
            _ if (addr & 0xF007) == 0xC004 => self.irq_prescaler = val ^ self.irq_xor,
            _ if (addr & 0xF007) == 0xC005 => self.irq_counter = val ^ self.irq_xor,
            _ if (addr & 0xF007) == 0xC006 => self.irq_xor = val,
            _ if (addr & 0xF007) == 0xC007 => self.irq_prescaler_config = val,
            _ if (addr & 0xF803) == 0xD000 => self.mode = val,
            _ if (addr & 0xF803) == 0xD001 => {
                self.mirroring_select = val;
                self.mirroring = match val & 0x03 {
                    0 => Mirroring::Vertical,
                    1 => Mirroring::Horizontal,
                    2 => Mirroring::SingleScreenLower,
                    _ => Mirroring::SingleScreenUpper,
                };
            }
            _ if (addr & 0xF803) == 0xD002 => self.ppu_config = val,
            _ if (addr & 0xF803) == 0xD003 => self.outer_bank = val,
            _ => {}
        }

        if self.irq_enabled && (self.irq_mode & 0x03) == 3 {
            self.clock_irq();
        }
    }

    fn chr_read(&mut self, addr: u16) -> u8 {
        if self.chr.is_empty() {
            return 0;
        }
        let bank = self.chr_bank(addr);
        let offset = addr as usize & 0x03FF;
        self.chr[(bank * 0x0400 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() || self.ppu_config & 0x40 == 0 {
            return;
        }
        let bank = self.chr_bank(addr);
        let offset = addr as usize & 0x03FF;
        let idx = (bank * 0x0400 + offset) % self.chr.len();
        self.chr[idx] = val;
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn ppu_nametable_read(&mut self, addr: u16, ciram: &[u8]) -> Option<u8> {
        let offset = usize::from(addr & 0x03FF);
        match self.special_nametable_source(addr)? {
            NametableSource::Ciram(page) => Some(ciram[(page * 0x0400 + offset) % ciram.len()]),
            NametableSource::Rom(bank) => {
                if self.chr.is_empty() {
                    return Some(0);
                }
                Some(self.chr[(bank * 0x0400 + offset) % self.chr.len()])
            }
        }
    }

    fn ppu_nametable_write(&mut self, addr: u16, val: u8, ciram: &mut [u8]) -> bool {
        let Some(source) = self.special_nametable_source(addr) else {
            return false;
        };

        if let NametableSource::Ciram(page) = source {
            let offset = usize::from(addr & 0x03FF);
            let idx = (page * 0x0400 + offset) % ciram.len();
            ciram[idx] = val;
        }

        true
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn notify_scanline(&mut self) {
        if self.irq_enabled && (self.irq_mode & 0x03) == 1 {
            self.clock_irq_times(8);
        } else if self.irq_enabled && (self.irq_mode & 0x03) == 2 {
            self.clock_irq_times(170);
        }
    }

    fn clock_cpu(&mut self) {
        if self.irq_enabled && (self.irq_mode & 0x03) == 0 {
            self.clock_irq();
        }
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_bytes(&self.prg_ram);
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_u8(self.mirroring_select);
        w.write_bytes(&self.prg_regs);
        for bank in self.chr_regs {
            w.write_u16(bank);
        }
        for bank in self.nt_regs {
            w.write_u16(bank);
        }
        w.write_u8(self.mode);
        w.write_u8(self.ppu_config);
        w.write_u8(self.outer_bank);
        w.write_bytes(&self.multiplier);
        w.write_u8(self.accumulator);
        w.write_u8(self.test_reg);
        w.write_u8(self.irq_prescaler);
        w.write_u8(self.irq_counter);
        w.write_u8(self.irq_xor);
        w.write_u8(self.irq_prescaler_config);
        w.write_bool(self.irq_enabled);
        w.write_u8(self.irq_mode);
        w.write_bool(self.irq_pending);
        crate::save_state::write_chr_state(w, &self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        r.read_exact(&mut self.prg_ram)?;
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.mirroring_select = r.read_u8()?;
        r.read_exact(&mut self.prg_regs)?;
        for bank in &mut self.chr_regs {
            *bank = r.read_u16()?;
        }
        for bank in &mut self.nt_regs {
            *bank = r.read_u16()?;
        }
        self.mode = r.read_u8()?;
        self.ppu_config = r.read_u8()?;
        self.outer_bank = r.read_u8()?;
        r.read_exact(&mut self.multiplier)?;
        self.accumulator = r.read_u8()?;
        self.test_reg = r.read_u8()?;
        self.irq_prescaler = r.read_u8()?;
        self.irq_counter = r.read_u8()?;
        self.irq_xor = r.read_u8()?;
        self.irq_prescaler_config = r.read_u8()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_mode = r.read_u8()?;
        self.irq_pending = r.read_bool()?;
        crate::save_state::read_chr_state(r, &mut self.chr, "J.Y. ASIC")?;
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
    fn switches_8k_prg_and_1k_chr() {
        let mut mapper = JyAsic::new(prg_banks(64), chr_banks(512), Mirroring::Vertical);

        mapper.cpu_write(0xD000, 0x18 | 0x02);
        mapper.cpu_write(0x8000, 3);
        mapper.cpu_write(0x8001, 4);
        mapper.cpu_write(0x8002, 5);
        mapper.cpu_write(0x9000, 9);
        mapper.cpu_write(0x9002, 10);

        assert_eq!(mapper.cpu_peek(0x8000), 3);
        assert_eq!(mapper.cpu_peek(0xA000), 4);
        assert_eq!(mapper.cpu_peek(0xC000), 5);
        assert_eq!(mapper.cpu_peek(0xE000), 63);
        assert_eq!(mapper.chr_read(0x0000), 9);
        assert_eq!(mapper.chr_read(0x0800), 10);
    }

    #[test]
    fn wide_prg_and_chr_modes_use_bank_size_units() {
        let mut mapper = JyAsic::new(prg_banks(64), chr_banks(512), Mirroring::Vertical);

        mapper.cpu_write(0xD000, 0x04);
        mapper.cpu_write(0x8003, 3);
        assert_eq!(mapper.cpu_peek(0x8000), 12);

        mapper.cpu_write(0xD000, 0x01);
        mapper.cpu_write(0x8001, 3);
        assert_eq!(mapper.cpu_peek(0x8000), 6);
        assert_eq!(mapper.cpu_peek(0xA000), 7);

        mapper.cpu_write(0xD000, 0x00);
        mapper.cpu_write(0x9000, 3);
        assert_eq!(mapper.chr_read(0x0000), 24);

        mapper.cpu_write(0xD000, 0x08);
        mapper.cpu_write(0x9004, 3);
        assert_eq!(mapper.chr_read(0x1000), 12);

        mapper.cpu_write(0xD000, 0x10);
        mapper.cpu_write(0x9002, 3);
        assert_eq!(mapper.chr_read(0x0800), 6);
    }

    #[test]
    fn multiplier_accumulator_and_mirroring() {
        let mut mapper = JyAsic::new(prg_banks(4), chr_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0x5800, 7);
        mapper.cpu_write(0x5801, 9);
        assert_eq!(mapper.cpu_peek(0x5800), 63);
        mapper.cpu_write(0x5802, 5);
        mapper.cpu_write(0x5802, 6);
        assert_eq!(mapper.cpu_peek(0x5802), 11);
        mapper.cpu_write(0xD001, 1);
        assert_eq!(mapper.mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn irq_counts_cpu_cycles_in_selected_direction() {
        let mut mapper = JyAsic::new(prg_banks(4), chr_banks(8), Mirroring::Vertical);

        mapper.irq_counter = 0xFF;
        mapper.cpu_write(0xC001, 0x40);
        mapper.cpu_write(0xC000, 1);
        for _ in 0..256 {
            mapper.clock_cpu();
        }
        assert!(mapper.irq_pending());
    }

    #[test]
    fn irq_uses_prescaler_and_f007_register_aliases() {
        let mut mapper = JyAsic::new(prg_banks(4), chr_banks(8), Mirroring::Vertical);

        mapper.cpu_write(0xC806, 0x55);
        mapper.cpu_write(0xC804, 0x55);
        mapper.cpu_write(0xC805, 0xFE);
        mapper.cpu_write(0xC801, 0x44);
        mapper.cpu_write(0xC803, 0);

        assert_eq!(mapper.irq_prescaler, 0);
        assert_eq!(mapper.irq_counter, 0xAB);
        for _ in 0..7 {
            mapper.clock_cpu();
        }
        assert_eq!(mapper.irq_counter, 0xAB);
        mapper.clock_cpu();
        assert_eq!(mapper.irq_counter, 0xAC);
        assert!(!mapper.irq_pending());
    }

    #[test]
    fn irq_disable_acknowledges_and_resets_prescaler() {
        let mut mapper = JyAsic::new(prg_banks(4), chr_banks(8), Mirroring::Vertical);

        mapper.irq_prescaler = 0x7E;
        mapper.irq_counter = 0xFF;
        mapper.cpu_write(0xC001, 0x40);
        mapper.cpu_write(0xC003, 0);
        for _ in 0..256 {
            mapper.clock_cpu();
        }
        assert!(mapper.irq_pending());

        mapper.cpu_write(0xC002, 0);
        assert!(!mapper.irq_enabled);
        assert!(!mapper.irq_pending());
        assert_eq!(mapper.irq_prescaler, 0);
    }

    #[test]
    fn mapper209_maps_rom_nametables_from_b_registers() {
        let mut mapper = JyAsic::new_mapper209(prg_banks(4), chr_banks(512), Mirroring::Vertical);

        mapper.cpu_write(0xD000, 0x20);
        mapper.cpu_write(0xD002, 0x00);
        mapper.cpu_write(0xB000, 0x8D);
        mapper.cpu_write(0xB004, 0x01);

        assert_eq!(mapper.ppu_nametable_read(0x2000, &[0; 0x1000]), Some(0x8D));
    }

    #[test]
    fn mapper90_suppresses_rom_nametable_registers() {
        let mut mapper = JyAsic::new(prg_banks(4), chr_banks(512), Mirroring::Vertical);

        mapper.cpu_write(0xD000, 0x20);
        mapper.cpu_write(0xB000, 0x8D);
        mapper.cpu_write(0xB004, 0x01);

        assert_eq!(mapper.ppu_nametable_read(0x2000, &[0; 0x1000]), None);
    }
}
