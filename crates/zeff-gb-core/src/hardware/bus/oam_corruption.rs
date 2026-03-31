use super::Bus;
use crate::hardware::types::constants::*;
use crate::hardware::types::hardware_mode::HardwareMode;

impl Bus {
    #[inline]
    pub fn maybe_trigger_oam_corruption(&mut self, addr: u16, kind: OamCorruptionType) {
        if self.hardware_mode != HardwareMode::DMG
            || !(OAM_START..=0xFEFF).contains(&addr)
            || !self.io.ppu.lcd_enabled()
            || self.io.ppu.ly >= 144
        {
            return;
        }

        let dots = self.io.ppu.cycles;
        if !(1..80).contains(&dots) {
            return;
        }

        let row = (dots as usize) * 2;
        if !(8..0xA0).contains(&row) {
            return;
        }

        self.apply_oam_corruption(row, kind);
    }

    #[inline]
    fn read_word(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.oam[offset], self.oam[offset + 1]])
    }

    #[inline]
    fn write_word(&mut self, offset: usize, value: u16) {
        [self.oam[offset], self.oam[offset + 1]] = value.to_le_bytes();
    }

    #[inline]
    fn mix_and(base: u16, mask: u16, extra: u16) -> u16 {
        (base & mask) | extra
    }

    #[inline]
    fn copy_block(&mut self, src: usize, dst: usize) {
        self.oam.copy_within(src..src + 8, dst);
    }

    fn apply_oam_corruption(&mut self, row: usize, kind: OamCorruptionType) {
        match kind {
            OamCorruptionType::Write => {
                let a = self.read_word(row);
                let b = self.read_word(row - 8);
                let c = self.read_word(row - 4);

                let mixed = ((a ^ c) & (b ^ c)) ^ c;

                self.write_word(row, mixed);
                self.oam.copy_within(row - 6..row, row + 2);
            }

            OamCorruptionType::Read => {
                let align = row & 0x18;

                if align == 0x10 && row < 0x98 {
                    let a = self.read_word(row - 16);
                    let b = self.read_word(row - 8);
                    let c = self.read_word(row);
                    let d = self.read_word(row - 4);

                    let mask = a | c | d;
                    let extra = a & c & d;

                    let result = Self::mix_and(b, mask, extra);

                    self.write_word(row - 8, result);
                    self.copy_block(row - 8, row - 16);
                } else if align == 0x00 && row < 0x98 {
                    if row == 0x40 {
                        let b = self.read_word(row);
                        let c = self.read_word(row - 4);
                        let d = self.read_word(row - 6);
                        let e = self.read_word(row - 8);
                        let f = self.read_word(row - 14);
                        let g = self.read_word(row - 16);
                        let h = self.read_word(row - 32);

                        let mask = h | g | ((!d) & f) | c | b;
                        let extra = c & g & h;

                        let result = Self::mix_and(e, mask, extra);

                        self.write_word(row - 8, result);
                        self.copy_block(row - 8, row - 16);
                        self.copy_block(row - 8, row - 32);
                    } else {
                        let a = self.read_word(row);
                        let b = self.read_word(row - 4);
                        let c = self.read_word(row - 8);
                        let d = self.read_word(row - 16);
                        let e = self.read_word(row - 32);

                        let mask = a | b | d | e;

                        let result = match row {
                            0x20 => Self::mix_and(c, mask, a & b & d & e),
                            0x60 => Self::mix_and(c, mask, b & d & e),
                            _ => c | (a & b & d & e),
                        };

                        self.write_word(row - 8, result);
                        self.copy_block(row - 8, row - 16);
                        self.copy_block(row - 8, row - 32);
                    }
                } else {
                    let a = self.read_word(row);
                    let b = self.read_word(row - 8);
                    let c = self.read_word(row - 4);

                    let result = b | (a & c);

                    self.write_word(row, result);
                    self.write_word(row - 8, result);
                }

                self.copy_block(row - 8, row);

                if row == 0x80 {
                    self.copy_block(0x80, 0);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OamCorruptionType {
    Write,
    Read,
}
