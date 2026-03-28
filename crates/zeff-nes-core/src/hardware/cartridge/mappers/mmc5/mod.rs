use crate::hardware::cartridge::{ChrFetchKind, Mapper, Mirroring};
use std::cell::Cell;

pub struct Mmc5 {
    prg_rom: Vec<u8>,
    chr: Vec<u8>,
    prg_ram: Vec<u8>,
    ex_ram: [u8; 0x400],
    has_battery: bool,

    mirroring: Mirroring,
    fixed_four_screen: bool,

    prg_mode: u8,
    chr_mode: u8,
    exram_mode: u8,
    upper_chr_bank_bits: u8,

    prg_ram_write_protect_1: u8,
    prg_ram_write_protect_2: u8,

    nametable_mode: u8,
    fill_tile: u8,
    fill_attr: u8,

    prg_ram_bank: u8,
    prg_banks: [u8; 4],
    chr_banks: [u16; 12],

    multiplicand: u8,
    multiplier: u8,

    irq_line_compare: u8,
    irq_enabled: bool,
    irq_pending: Cell<bool>,
    in_frame: Cell<bool>,
    current_scanline: Cell<u16>,

    consecutive_nt_reads: Cell<u8>,

    exram_tile_byte: Cell<u8>,
}

impl Mmc5 {
    pub fn new(
        prg_rom: Vec<u8>,
        chr: Vec<u8>,
        mirroring: Mirroring,
        prg_ram_size: usize,
        has_battery: bool,
    ) -> Self {
        let ram_len = if prg_ram_size == 0 {
            0x2000
        } else {
            prg_ram_size
        };

        let mut this = Self {
            prg_rom,
            chr,
            prg_ram: vec![0; ram_len],
            ex_ram: [0; 0x400],
            has_battery,
            mirroring,
            fixed_four_screen: matches!(mirroring, Mirroring::FourScreen),
            prg_mode: 3,
            chr_mode: 3,
            exram_mode: 0,
            upper_chr_bank_bits: 0,
            prg_ram_write_protect_1: 0,
            prg_ram_write_protect_2: 0,
            nametable_mode: 0,
            fill_tile: 0,
            fill_attr: 0,
            prg_ram_bank: 0,
            prg_banks: [0, 0, 0, 0xFF],
            chr_banks: [0; 12],
            multiplicand: 0,
            multiplier: 0,
            irq_line_compare: 0,
            irq_enabled: false,
            irq_pending: Cell::new(false),
            in_frame: Cell::new(true),
            current_scanline: Cell::new(0),

            consecutive_nt_reads: Cell::new(0),
            exram_tile_byte: Cell::new(0),
        };

        this.reset_chr_defaults();
        this
    }

    fn reset_chr_defaults(&mut self) {
        let bank_count = self.chr_bank_count_1k() as u16;
        for (idx, bank) in self.chr_banks.iter_mut().enumerate() {
            *bank = (idx as u16) % bank_count;
        }
    }

    fn prg_rom_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    fn prg_ram_bank_count_8k(&self) -> usize {
        (self.prg_ram.len() / 0x2000).max(1)
    }

    fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    fn prg_ram_writable(&self) -> bool {
        self.prg_ram_write_protect_1 == 0x02 && self.prg_ram_write_protect_2 == 0x01
    }

    fn decode_prg_bank_register(&self, value: u8) -> (bool, usize) {
        let is_rom = value & 0x80 != 0;
        let index = (value & 0x7F) as usize;
        (is_rom, index)
    }

    fn read_prg_8k_bank(&self, addr: u16, bank_reg: u8) -> u8 {
        let offset = (addr as usize) & 0x1FFF;
        let (is_rom, index) = self.decode_prg_bank_register(bank_reg);

        if !is_rom {
            if self.prg_ram.is_empty() {
                return 0;
            }
            let bank = index % self.prg_ram_bank_count_8k();
            return self.prg_ram[bank * 0x2000 + offset];
        }

        let bank = index % self.prg_rom_bank_count_8k();
        self.prg_rom[bank * 0x2000 + offset]
    }

    fn write_prg_8k_bank(&mut self, addr: u16, bank_reg: u8, val: u8) {
        if self.prg_ram.is_empty() || !self.prg_ram_writable() {
            return;
        }

        let (is_rom, index) = self.decode_prg_bank_register(bank_reg);
        if is_rom {
            return;
        }

        let bank = index % self.prg_ram_bank_count_8k();
        let offset = (addr as usize) & 0x1FFF;
        self.prg_ram[bank * 0x2000 + offset] = val;
    }

    fn map_prg_read(&self, addr: u16) -> u8 {
        match self.prg_mode & 0x03 {
            0 => {
                let bank32 = (self.prg_banks[3] as usize & 0x7C) >> 2;
                let slot = ((addr as usize - 0x8000) >> 13) & 0x03;
                let bank = (bank32 * 4 + slot) as u8;
                self.read_prg_8k_bank(addr, 0x80 | bank)
            }
            1 => {
                if addr < 0xC000 {
                    let reg = self.prg_banks[1];
                    let rom_bit = reg & 0x80;
                    let bank16 = ((reg & 0x7E) >> 1) as usize;
                    let slot = ((addr as usize - 0x8000) >> 13) & 0x01;
                    let bank = ((bank16 * 2 + slot) as u8) & 0x7F;
                    self.read_prg_8k_bank(addr, rom_bit | bank)
                } else {
                    let bank16 = ((self.prg_banks[3] & 0x7E) >> 1) as usize;
                    let slot = ((addr as usize - 0xC000) >> 13) & 0x01;
                    let bank = (bank16 * 2 + slot) as u8;
                    self.read_prg_8k_bank(addr, 0x80 | bank)
                }
            }
            2 => match addr {
                0x8000..=0xBFFF => {
                    let reg = self.prg_banks[1];
                    let rom_bit = reg & 0x80;
                    let bank16 = ((reg & 0x7E) >> 1) as usize;
                    let slot = ((addr as usize - 0x8000) >> 13) & 0x01;
                    let bank = ((bank16 * 2 + slot) as u8) & 0x7F;
                    self.read_prg_8k_bank(addr, rom_bit | bank)
                }
                0xC000..=0xDFFF => self.read_prg_8k_bank(addr, self.prg_banks[2]),
                0xE000..=0xFFFF => self.read_prg_8k_bank(addr, self.prg_banks[3] | 0x80),
                _ => 0,
            },
            _ => match addr {
                0x8000..=0x9FFF => self.read_prg_8k_bank(addr, self.prg_banks[0]),
                0xA000..=0xBFFF => self.read_prg_8k_bank(addr, self.prg_banks[1]),
                0xC000..=0xDFFF => self.read_prg_8k_bank(addr, self.prg_banks[2]),
                0xE000..=0xFFFF => self.read_prg_8k_bank(addr, self.prg_banks[3] | 0x80),
                _ => 0,
            },
        }
    }

    fn map_prg_write(&mut self, addr: u16, val: u8) {
        match self.prg_mode & 0x03 {
            0 => {
            }
            1 => {
                if addr < 0xC000 {
                    let reg = self.prg_banks[1];
                    let rom_bit = reg & 0x80;
                    let bank16 = ((reg & 0x7E) >> 1) as usize;
                    let slot = ((addr as usize - 0x8000) >> 13) & 0x01;
                    let bank = ((bank16 * 2 + slot) as u8) & 0x7F;
                    self.write_prg_8k_bank(addr, rom_bit | bank, val);
                }
            }
            2 => match addr {
                0x8000..=0xBFFF => {
                    let reg = self.prg_banks[1];
                    let rom_bit = reg & 0x80;
                    let bank16 = ((reg & 0x7E) >> 1) as usize;
                    let slot = ((addr as usize - 0x8000) >> 13) & 0x01;
                    let bank = ((bank16 * 2 + slot) as u8) & 0x7F;
                    self.write_prg_8k_bank(addr, rom_bit | bank, val);
                }
                0xC000..=0xDFFF => self.write_prg_8k_bank(addr, self.prg_banks[2], val),
                0xE000..=0xFFFF => { /* ROM — ignore */ }
                _ => {}
            },
            _ => match addr {
                0x8000..=0x9FFF => self.write_prg_8k_bank(addr, self.prg_banks[0], val),
                0xA000..=0xBFFF => self.write_prg_8k_bank(addr, self.prg_banks[1], val),
                0xC000..=0xDFFF => self.write_prg_8k_bank(addr, self.prg_banks[2], val),
                0xE000..=0xFFFF => { /* ROM — ignore */ }
                _ => {}
            },
        }
    }

    fn chr_bank_index(&self, addr: u16, kind: ChrFetchKind) -> usize {
        if self.exram_mode == 1 && matches!(kind, ChrFetchKind::Background) {
            let exram_byte = self.exram_tile_byte.get();
            let bank_4k = ((self.upper_chr_bank_bits as usize) << 6)
                | (exram_byte as usize & 0x3F);
            let sub = ((addr as usize) >> 10) & 0x03;
            return bank_4k * 4 + sub;
        }

        let addr = addr as usize;
        let (r1, r3, r5, r7, slot_mask) = match kind {
            ChrFetchKind::Sprite => (1usize, 3usize, 5usize, 7usize, 0x07usize),
            ChrFetchKind::Background => (8usize, 9usize, 10usize, 11usize, 0x03usize),
        };

        match self.chr_mode & 0x03 {
            0 => {
                let base = (self.chr_banks[r7] as usize) & !0x07;
                base + (addr >> 10)
            }
            1 => {
                let slot = (addr >> 12) & 0x01;
                let base = if slot == 0 {
                    (self.chr_banks[r3] as usize) & !0x03
                } else {
                    (self.chr_banks[r7] as usize) & !0x03
                };
                base + ((addr >> 10) & 0x03)
            }
            2 => {
                let slot = (addr >> 11) & 0x03;
                let base = match slot {
                    0 => (self.chr_banks[r1] as usize) & !0x01,
                    1 => (self.chr_banks[r3] as usize) & !0x01,
                    2 => (self.chr_banks[r5] as usize) & !0x01,
                    _ => (self.chr_banks[r7] as usize) & !0x01,
                };
                base + ((addr >> 10) & 0x01)
            }
            _ => {
                let slot = (addr >> 10) & slot_mask;
                let base = if matches!(kind, ChrFetchKind::Sprite) { 0 } else { 8 };
                self.chr_banks[base + slot] as usize
            }
        }
    }

    fn apply_nametable_mode(&mut self, val: u8) {
        self.nametable_mode = val;
        if self.fixed_four_screen {
            self.mirroring = Mirroring::FourScreen;
            return;
        }

        let a = val & 0x03;
        let b = (val >> 2) & 0x03;
        let c = (val >> 4) & 0x03;
        let d = (val >> 6) & 0x03;

        self.mirroring = if a == b && c == d && a != c {
            Mirroring::Horizontal
        } else if a == c && b == d && a != b {
            Mirroring::Vertical
        } else if a == b && b == c && c == d {
            if a & 0x01 == 0 {
                Mirroring::SingleScreenLower
            } else {
                Mirroring::SingleScreenUpper
            }
        } else {
            self.mirroring
        };
    }

    fn fill_attr_byte(&self) -> u8 {
        let attr = self.fill_attr & 0x03;
        attr | (attr << 2) | (attr << 4) | (attr << 6)
    }

    fn decode_nametable_addr(addr: u16) -> (usize, usize) {
        let vaddr = ((addr - 0x2000) & 0x0FFF) as usize;
        let table = vaddr >> 10;
        let offset = vaddr & 0x03FF;
        (table, offset)
    }

    fn nametable_source(&self, table: usize) -> u8 {
        (self.nametable_mode >> (table * 2)) & 0x03
    }

    fn write_register(&mut self, addr: u16, val: u8) {
        match addr {
            0x5100 => self.prg_mode = val & 0x03,
            0x5101 => self.chr_mode = val & 0x03,
            0x5102 => self.prg_ram_write_protect_1 = val & 0x03,
            0x5103 => self.prg_ram_write_protect_2 = val & 0x03,
            0x5104 => self.exram_mode = val & 0x03,
            0x5105 => self.apply_nametable_mode(val),
            0x5106 => self.fill_tile = val,
            0x5107 => self.fill_attr = val & 0x03,
            0x5113 => self.prg_ram_bank = val & 0x07,
            0x5114 => self.prg_banks[0] = val,
            0x5115 => self.prg_banks[1] = val,
            0x5116 => self.prg_banks[2] = val,
            0x5117 => self.prg_banks[3] = val | 0x80,
            0x5120..=0x512B => {
                let index = (addr - 0x5120) as usize;
                let bank = (((self.upper_chr_bank_bits as u16) << 8) | val as u16)
                    % self.chr_bank_count_1k() as u16;
                self.chr_banks[index] = bank;
            }
            0x5130 => self.upper_chr_bank_bits = val & 0x03,
            0x5203 => self.irq_line_compare = val,
            0x5204 => {
                self.irq_enabled = val & 0x80 != 0;
            }
            0x5205 => self.multiplicand = val,
            0x5206 => self.multiplier = val,
            _ => {}
        }
    }
}

impl Mapper for Mmc5 {
    fn cpu_read(&self, addr: u16) -> u8 {
        match addr {
            0x5C00..=0x5FFF => {
                if self.exram_mode >= 2 {
                    self.ex_ram[(addr - 0x5C00) as usize]
                } else {
                    0
                }
            }
            0x5204 => {
                let mut status = 0u8;
                if self.irq_pending.get() {
                    status |= 0x80;
                }
                if self.in_frame.get() {
                    status |= 0x40;
                }

                self.irq_pending.set(false);
                status
            }
            0x5205 => {
                let product = self.multiplicand as u16 * self.multiplier as u16;
                product as u8
            }
            0x5206 => {
                let product = self.multiplicand as u16 * self.multiplier as u16;
                (product >> 8) as u8
            }
            0x6000..=0x7FFF => {
                if self.prg_ram.is_empty() {
                    0
                } else {
                    let bank = (self.prg_ram_bank as usize) % self.prg_ram_bank_count_8k();
                    let offset = (addr as usize) & 0x1FFF;
                    self.prg_ram[bank * 0x2000 + offset]
                }
            }
            0x8000..=0xFFFF => self.map_prg_read(addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, val: u8) {
        match addr {
            0x5100..=0x5130 | 0x5203..=0x5206 => self.write_register(addr, val),
            0x5C00..=0x5FFF => {
                if self.exram_mode != 3 {
                    self.ex_ram[(addr - 0x5C00) as usize] = val;
                }
            }
            0x6000..=0x7FFF => {
                if !self.prg_ram.is_empty() && self.prg_ram_writable() {
                    let bank = (self.prg_ram_bank as usize) % self.prg_ram_bank_count_8k();
                    let offset = (addr as usize) & 0x1FFF;
                    self.prg_ram[bank * 0x2000 + offset] = val;
                }
            }
            0x8000..=0xFFFF => self.map_prg_write(addr, val),
            _ => {}
        }
    }

    fn chr_read(&self, addr: u16) -> u8 {
        self.chr_read_kind(addr, ChrFetchKind::Background)
    }

    fn chr_read_kind(&self, addr: u16, kind: ChrFetchKind) -> u8 {
        self.consecutive_nt_reads.set(0);

        if self.chr.is_empty() {
            return 0;
        }

        let bank = self.chr_bank_index(addr, kind) % self.chr_bank_count_1k();
        let offset = (addr as usize) & 0x03FF;
        self.chr[(bank * 0x0400 + offset) % self.chr.len()]
    }

    fn chr_write(&mut self, addr: u16, val: u8) {
        if self.chr.is_empty() {
            return;
        }

        let bank = self.chr_bank_index(addr, ChrFetchKind::Background) % self.chr_bank_count_1k();
        let offset = (addr as usize) & 0x03FF;
        let idx = (bank * 0x0400 + offset) % self.chr.len();
        self.chr[idx] = val;
    }

    fn ppu_nametable_read(&self, addr: u16, ciram: &[u8; 0x800]) -> Option<u8> {
        if !(0x2000..=0x3EFF).contains(&addr) {
            return None;
        }

        let count = self.consecutive_nt_reads.get().saturating_add(1);
        self.consecutive_nt_reads.set(count);
        if count == 3 {
            let sl = self.current_scanline.get();
            if sl >= 240 {
                self.current_scanline.set(0);
                self.in_frame.set(true);
            } else {
                let new_sl = sl + 1;
                self.current_scanline.set(new_sl);
                self.in_frame.set(new_sl < 240);
                if new_sl < 240
                    && self.irq_enabled
                    && new_sl as u8 == self.irq_line_compare
                {
                    self.irq_pending.set(true);
                }
            }
        }

        let (table, offset) = Self::decode_nametable_addr(addr);

        if self.exram_mode == 1 {
            if offset < 0x3C0 {
                self.exram_tile_byte.set(self.ex_ram[offset]);
            } else {
                let byte = self.exram_tile_byte.get();
                let attr = (byte >> 6) & 0x03;
                return Some(attr | (attr << 2) | (attr << 4) | (attr << 6));
            }
        }

        let source = self.nametable_source(table);
        let value = match source {
            0 => ciram[offset],
            1 => ciram[0x400 + offset],
            2 => {
                if self.exram_mode <= 1 {
                    self.ex_ram[offset]
                } else {
                    0
                }
            }
            _ => {
                if offset < 0x03C0 {
                    self.fill_tile
                } else {
                    self.fill_attr_byte()
                }
            }
        };
        Some(value)
    }

    fn ppu_nametable_write(&mut self, addr: u16, val: u8, ciram: &mut [u8; 0x800]) -> bool {
        if !(0x2000..=0x3EFF).contains(&addr) {
            return false;
        }

        let (table, offset) = Self::decode_nametable_addr(addr);
        match self.nametable_source(table) {
            0 => ciram[offset] = val,
            1 => ciram[0x400 + offset] = val,
            2 => {
                if self.exram_mode <= 1 {
                    self.ex_ram[offset] = val;
                }
            }
            _ => {}
        }
        true
    }

    fn mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending.get()
    }

    fn notify_scanline(&mut self) {
        // This is intentionally a no-op.
    }

    fn dump_battery_data(&self) -> Option<Vec<u8>> {
        if self.has_battery && !self.prg_ram.is_empty() {
            Some(self.prg_ram.clone())
        } else {
            None
        }
    }

    fn load_battery_data(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        if self.prg_ram.is_empty() {
            return Ok(());
        }

        let copy_len = self.prg_ram.len().min(bytes.len());
        self.prg_ram[..copy_len].copy_from_slice(&bytes[..copy_len]);
        if copy_len < self.prg_ram.len() {
            self.prg_ram[copy_len..].fill(0);
        }
        Ok(())
    }

    fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(crate::save_state::encode_mirroring(self.mirroring));
        w.write_bool(self.fixed_four_screen);

        w.write_u8(self.prg_mode);
        w.write_u8(self.chr_mode);
        w.write_u8(self.exram_mode);
        w.write_u8(self.upper_chr_bank_bits);

        w.write_u8(self.prg_ram_write_protect_1);
        w.write_u8(self.prg_ram_write_protect_2);
        w.write_u8(self.nametable_mode);
        w.write_u8(self.fill_tile);
        w.write_u8(self.fill_attr);

        w.write_bytes(&self.prg_banks);
        for bank in &self.chr_banks {
            w.write_u16(*bank);
        }

        w.write_u8(self.prg_ram_bank);
        w.write_u8(self.multiplicand);
        w.write_u8(self.multiplier);
        w.write_u8(self.exram_tile_byte.get());

        w.write_u8(self.irq_line_compare);
        w.write_bool(self.irq_enabled);
        w.write_bool(self.irq_pending.get());
        w.write_bool(self.in_frame.get());
        w.write_u16(self.current_scanline.get());

        w.write_bool(self.has_battery);
        w.write_vec(&self.prg_ram);
        w.write_vec(&self.ex_ram);
        w.write_vec(&self.chr);
    }

    fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.mirroring = crate::save_state::decode_mirroring(r.read_u8()?)?;
        self.fixed_four_screen = r.read_bool()?;

        self.prg_mode = r.read_u8()?;
        self.chr_mode = r.read_u8()?;
        self.exram_mode = r.read_u8()?;
        self.upper_chr_bank_bits = r.read_u8()?;

        self.prg_ram_write_protect_1 = r.read_u8()?;
        self.prg_ram_write_protect_2 = r.read_u8()?;
        self.nametable_mode = r.read_u8()?;
        self.fill_tile = r.read_u8()?;
        self.fill_attr = r.read_u8()?;

        r.read_exact(&mut self.prg_banks)?;
        for bank in &mut self.chr_banks {
            *bank = r.read_u16()?;
        }

        self.prg_ram_bank = r.read_u8()?;
        self.multiplicand = r.read_u8()?;
        self.multiplier = r.read_u8()?;
        self.exram_tile_byte.set(r.read_u8()?);

        self.irq_line_compare = r.read_u8()?;
        self.irq_enabled = r.read_bool()?;
        self.irq_pending.set(r.read_bool()?);
        self.in_frame.set(r.read_bool()?);
        self.current_scanline.set(r.read_u16()?);

        self.has_battery = r.read_bool()?;

        let prg_ram = r.read_vec(512 * 1024)?;
        if prg_ram.len() != self.prg_ram.len() {
            anyhow::bail!(
                "MMC5 PRG RAM size mismatch: expected {}, got {}",
                self.prg_ram.len(),
                prg_ram.len()
            );
        }
        self.prg_ram = prg_ram;

        let ex_ram = r.read_vec(0x400)?;
        if ex_ram.len() != self.ex_ram.len() {
            anyhow::bail!(
                "MMC5 ExRAM size mismatch: expected {}, got {}",
                self.ex_ram.len(),
                ex_ram.len()
            );
        }
        self.ex_ram.copy_from_slice(&ex_ram);

        let chr = r.read_vec(1024 * 1024)?;
        if chr.len() != self.chr.len() {
            anyhow::bail!("MMC5 CHR size mismatch: expected {}, got {}", self.chr.len(), chr.len());
        }
        self.chr = chr;

        Ok(())
    }
}

#[cfg(test)]
mod tests;
