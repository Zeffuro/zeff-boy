use crate::hardware::cartridge::ChrFetchKind;

use super::Mmc5;

impl Mmc5 {
    pub(super) fn prg_rom_bank_count_8k(&self) -> usize {
        (self.prg_rom.len() / 0x2000).max(1)
    }

    pub(super) fn prg_ram_bank_count_8k(&self) -> usize {
        (self.prg_ram.len() / 0x2000).max(1)
    }

    pub(super) fn chr_bank_count_1k(&self) -> usize {
        (self.chr.len() / 0x0400).max(1)
    }

    pub(super) fn prg_ram_writable(&self) -> bool {
        self.prg_ram_write_protect_1 == 0x02 && self.prg_ram_write_protect_2 == 0x01
    }

    fn decode_prg_bank_register(&self, value: u8) -> (bool, usize) {
        let is_rom = value & 0x80 != 0;
        let index = (value & 0x7F) as usize;
        (is_rom, index)
    }

    pub(super) fn read_prg_8k_bank(&self, addr: u16, bank_reg: u8) -> u8 {
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

    pub(super) fn write_prg_8k_bank(&mut self, addr: u16, bank_reg: u8, val: u8) {
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

    pub(super) fn map_prg_read(&self, addr: u16) -> u8 {
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

    pub(super) fn map_prg_write(&mut self, addr: u16, val: u8) {
        match self.prg_mode & 0x03 {
            0 => {}
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
                0xE000..=0xFFFF => { /* ROM:ignore */ }
                _ => {}
            },
            _ => match addr {
                0x8000..=0x9FFF => self.write_prg_8k_bank(addr, self.prg_banks[0], val),
                0xA000..=0xBFFF => self.write_prg_8k_bank(addr, self.prg_banks[1], val),
                0xC000..=0xDFFF => self.write_prg_8k_bank(addr, self.prg_banks[2], val),
                0xE000..=0xFFFF => { /* ROM:ignore */ }
                _ => {}
            },
        }
    }

    pub(super) fn chr_bank_index(&self, addr: u16, kind: ChrFetchKind) -> usize {
        if self.exram_mode == 1 && matches!(kind, ChrFetchKind::Background) {
            let exram_byte = self.exram_tile_byte;
            let bank_4k = ((self.upper_chr_bank_bits as usize) << 6) | (exram_byte as usize & 0x3F);
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
                let base = if matches!(kind, ChrFetchKind::Sprite) {
                    0
                } else {
                    8
                };
                self.chr_banks[base + slot] as usize
            }
        }
    }
}
