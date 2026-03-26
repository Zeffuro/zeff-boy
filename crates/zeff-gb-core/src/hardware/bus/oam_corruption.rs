use super::Bus;
use crate::hardware::types::constants::*;
use crate::hardware::types::hardware_mode::HardwareMode;

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
        if dots >= 80 {
            return;
        }

        let current_row = (dots / 4) as usize;
        if current_row == 0 || current_row >= 20 {
            return;
        }

        self.corrupt_oam_row(current_row, corruption_type);
    }

    #[inline]
    fn corrupt_oam_row(&mut self, row: usize, corruption_type: OamCorruptionType) {
        let row_offset = row * 8;
        let prev_offset = (row - 1) * 8;

        match corruption_type {
            OamCorruptionType::Single => {
                let mut row_n = [0u16; 4];
                let mut row_n1 = [0u16; 4];
                let mut row_n2 = [0u16; 4];

                for i in 0..4 {
                    let off = i * 2;
                    row_n[i] = u16::from_le_bytes([
                        self.oam[row_offset + off],
                        self.oam[row_offset + off + 1],
                    ]);
                    row_n1[i] = u16::from_le_bytes([
                        self.oam[prev_offset + off],
                        self.oam[prev_offset + off + 1],
                    ]);
                    if row >= 2 {
                        let pp_offset = (row - 2) * 8;
                        row_n2[i] = u16::from_le_bytes([
                            self.oam[pp_offset + off],
                            self.oam[pp_offset + off + 1],
                        ]);
                    }
                }

                for i in 0..4 {
                    let glitched = ((row_n[i] ^ row_n2[i]) & row_n1[i]) ^ row_n2[i];
                    let bytes_n = row_n[i].to_le_bytes();
                    self.oam[prev_offset + i * 2] = bytes_n[0];
                    self.oam[prev_offset + i * 2 + 1] = bytes_n[1];
                    let bytes_g = glitched.to_le_bytes();
                    self.oam[row_offset + i * 2] = bytes_g[0];
                    self.oam[row_offset + i * 2 + 1] = bytes_g[1];
                }
            }
            OamCorruptionType::Double => {
                let mut row_n = [0u16; 4];
                let mut row_n1 = [0u16; 4];
                let mut row_n2 = [0u16; 4];

                for i in 0..4 {
                    let off = i * 2;
                    row_n[i] = u16::from_le_bytes([
                        self.oam[row_offset + off],
                        self.oam[row_offset + off + 1],
                    ]);
                    row_n1[i] = u16::from_le_bytes([
                        self.oam[prev_offset + off],
                        self.oam[prev_offset + off + 1],
                    ]);
                    if row >= 2 {
                        let pp_offset = (row - 2) * 8;
                        row_n2[i] = u16::from_le_bytes([
                            self.oam[pp_offset + off],
                            self.oam[pp_offset + off + 1],
                        ]);
                    }
                }

                for i in 0..4 {
                    let merged = ((row_n[i] ^ row_n2[i]) & row_n1[i]) ^ row_n2[i];
                    let bytes = merged.to_le_bytes();
                    self.oam[row_offset + i * 2] = bytes[0];
                    self.oam[row_offset + i * 2 + 1] = bytes[1];
                    self.oam[prev_offset + i * 2] = bytes[0];
                    self.oam[prev_offset + i * 2 + 1] = bytes[1];
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OamCorruptionType {
    Single,
    Double,
}