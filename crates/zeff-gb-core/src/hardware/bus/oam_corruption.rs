// Thank you SameBoy for a lot of reference code on how to solve this
use super::Bus;
use crate::hardware::types::constants::*;
use crate::hardware::types::hardware_mode::HardwareMode;

#[inline]
fn glitch_write(a: u16, b: u16, c: u16) -> u16 {
    ((a ^ c) & (b ^ c)) ^ c
}

#[inline]
fn glitch_read(a: u16, b: u16, c: u16) -> u16 {
    b | (a & c)
}

#[inline]
fn glitch_read_secondary(a: u16, b: u16, c: u16, d: u16) -> u16 {
    (b & (a | c | d)) | (a & c & d)
}

#[inline]
fn glitch_tertiary_1(a: u16, b: u16, c: u16, d: u16, e: u16) -> u16 {
    c | (a & b & d & e)
}

#[inline]
fn glitch_tertiary_2(a: u16, b: u16, c: u16, d: u16, e: u16) -> u16 {
    (c & (a | b | d | e)) | (a & b & d & e)
}

#[inline]
fn glitch_tertiary_3(a: u16, b: u16, c: u16, d: u16, e: u16) -> u16 {
    (c & (a | b | d | e)) | (b & d & e)
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn glitch_quaternary_dmg(
    _a: u16, b: u16, c: u16, d: u16, e: u16, f: u16, g: u16, h: u16,
) -> u16 {
    (e & (h | g | (!d & f) | c | b)) | (c & g & h)
}

impl Bus {
    #[inline]
    pub fn maybe_trigger_oam_corruption(&mut self, addr: u16, corruption_type: OamCorruptionType) {
        if self.hardware_mode != HardwareMode::DMG {
            return;
        }

        if addr < OAM_START || addr > 0xFEFF {
            return;
        }

        if !self.io.ppu.lcd_enabled() {
            return;
        }

        let ly = self.io.ppu.ly;
        if ly >= 144 {
            return;
        }

        let dots = self.io.ppu.cycles;
        if dots == 0 || dots >= 80 {
            return;
        }

        let accessed_oam_row = (dots as usize) * 2;

        if accessed_oam_row < 8 || accessed_oam_row >= 0xA0 {
            return;
        }

        self.apply_oam_corruption(accessed_oam_row, corruption_type);
    }

    #[inline]
    fn oam_read_word(&self, offset: usize) -> u16 {
        u16::from_le_bytes([self.oam[offset], self.oam[offset + 1]])
    }

    #[inline]
    fn oam_write_word(&mut self, offset: usize, val: u16) {
        [self.oam[offset], self.oam[offset + 1]] = val.to_le_bytes();
    }

    fn apply_oam_corruption(&mut self, row: usize, corruption_type: OamCorruptionType) {
        match corruption_type {
            OamCorruptionType::Write => {
                let a = self.oam_read_word(row);
                let b = self.oam_read_word(row - 8);
                let c = self.oam_read_word(row - 4);

                self.oam_write_word(row, glitch_write(a, b, c));
                self.oam.copy_within(row - 6..row, row + 2);
            }

            OamCorruptionType::Read => {
                let row_alignment = row & 0x18;

                if row_alignment == 0x10 {
                    if row < 0x98 {
                        let a = self.oam_read_word(row - 16);
                        let b = self.oam_read_word(row - 8);
                        let c = self.oam_read_word(row);
                        let d = self.oam_read_word(row - 4);

                        self.oam_write_word(row - 8, glitch_read_secondary(a, b, c, d));
                        self.oam.copy_within(row - 8..row, row - 16);
                    }
                } else if row_alignment == 0x00 {
                    if row < 0x98 {
                        if row == 0x40 {
                            let a = self.oam_read_word(0);
                            let b = self.oam_read_word(row);
                            let c = self.oam_read_word(row - 4);
                            let d = self.oam_read_word(row - 6);
                            let e = self.oam_read_word(row - 8);
                            let f = self.oam_read_word(row - 14);
                            let g = self.oam_read_word(row - 16);
                            let h = self.oam_read_word(row - 32);

                            self.oam_write_word(row - 8, glitch_quaternary_dmg(a, b, c, d, e, f, g, h));
                            self.oam.copy_within(row - 8..row, row - 16);
                            self.oam.copy_within(row - 8..row, row - 32);
                        } else {
                            let a = self.oam_read_word(row);
                            let b = self.oam_read_word(row - 4);
                            let c = self.oam_read_word(row - 8);
                            let d = self.oam_read_word(row - 16);
                            let e = self.oam_read_word(row - 32);

                            let glitched = match row {
                                0x20 => glitch_tertiary_2(a, b, c, d, e),
                                0x60 => glitch_tertiary_3(a, b, c, d, e),
                                _    => glitch_tertiary_1(a, b, c, d, e),
                            };

                            self.oam_write_word(row - 8, glitched);
                            self.oam.copy_within(row - 8..row, row - 16);
                            self.oam.copy_within(row - 8..row, row - 32);
                        }
                    }
                } else {
                    let a = self.oam_read_word(row);
                    let b = self.oam_read_word(row - 8);
                    let c = self.oam_read_word(row - 4);
                    let glitched = glitch_read(a, b, c);

                    self.oam_write_word(row, glitched);
                    self.oam_write_word(row - 8, glitched);
                }

                self.oam.copy_within(row - 8..row, row);

                if row == 0x80 {
                    self.oam.copy_within(0x80..0x88, 0);
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