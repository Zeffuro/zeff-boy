use super::Bus;
use crate::hardware::cartridge::ChrFetchKind;
use crate::hardware::ppu::{PRE_RENDER_SCANLINE, Ppu};

impl Bus {
    pub(super) fn ppu_render_dot(&mut self) {
        let scanline = self.ppu.scanline;
        let dot = self.ppu.dot;
        let rendering = self.ppu.regs.rendering_enabled();
        let visible_line = scanline < 240;
        let pre_render = scanline == PRE_RENDER_SCANLINE;
        let render_line = visible_line || pre_render;

        if rendering && render_line {
            let bg_hi = self.ppu.regs.bg_pattern_addr() != 0;
            let spr_hi = self.ppu.regs.sprite_pattern_addr() != 0;
            let notify_dot = if bg_hi && !spr_hi { 324 } else { 260 };
            if dot == notify_dot {
                self.cartridge.notify_scanline();
            }
        }

        if rendering && visible_line && dot == 0 {
            self.evaluate_sprites_for_scanline(scanline);
        }
        if rendering && pre_render && dot == 0 {
            self.evaluate_sprites_for_scanline(0);
        }

        if visible_line && (1..=256).contains(&dot) {
            if rendering {
                let pal_idx = self.ppu.compose_pixel() as usize;
                Self::write_pixel(&mut self.ppu, dot, scanline, pal_idx, &self.palette_lut);
            } else {
                let pal_idx = (self.ppu.palette_ram[0] & 0x3F) as usize;
                Self::write_pixel(&mut self.ppu, dot, scanline, pal_idx, &self.palette_lut);
            }
        }

        if rendering && render_line {
            let in_bg_range = (1..=256).contains(&dot) || (321..=336).contains(&dot);

            if in_bg_range {
                self.ppu.update_shifters();

                match (dot - 1) % 8 {
                    0 => {
                        self.ppu.load_bg_shifters();
                        let addr = 0x2000 | (self.ppu.v & 0x0FFF);
                        self.ppu.bg_next_tile_id = self.ppu_bus_read(addr);
                    }
                    2 => {
                        let v = self.ppu.v;
                        let addr = 0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07);
                        let attrib = self.ppu_bus_read(addr);
                        let shift = ((v >> 4) & 0x04) | (v & 0x02);
                        self.ppu.bg_next_tile_attrib = (attrib >> shift) & 0x03;
                    }
                    4 => {
                        let base = self.ppu.regs.bg_pattern_addr();
                        let fine_y = (self.ppu.v >> 12) & 0x07;
                        let addr = base + (self.ppu.bg_next_tile_id as u16) * 16 + fine_y;
                        self.ppu.bg_next_tile_lo = self.ppu_bus_read(addr);
                    }
                    6 => {
                        let base = self.ppu.regs.bg_pattern_addr();
                        let fine_y = (self.ppu.v >> 12) & 0x07;
                        let addr = base + (self.ppu.bg_next_tile_id as u16) * 16 + fine_y + 8;
                        self.ppu.bg_next_tile_hi = self.ppu_bus_read(addr);
                    }
                    7 => {
                        self.ppu.increment_scroll_x();
                    }
                    _ => {}
                }
            }

            if dot == 256 {
                self.ppu.increment_scroll_y();
            }

            if dot == 257 {
                self.ppu.copy_horizontal_bits();
            }

            if pre_render && (280..=304).contains(&dot) {
                self.ppu.copy_vertical_bits();
            }
        }
    }

    #[inline]
    fn write_pixel(
        ppu: &mut Ppu,
        dot: u16,
        scanline: u16,
        pal_idx: usize,
        palette_lut: &[[u8; 4]; 64],
    ) {
        let effective_idx = if ppu.regs.greyscale() {
            pal_idx & 0x30
        } else {
            pal_idx
        };
        let [mut r, mut g, mut b, _] = palette_lut[effective_idx];

        let emph_bits = ppu.regs.mask & 0xE0;
        if emph_bits != 0 {
            const ATTEN_NUM: u16 = 192;
            const ATTEN_DEN: u16 = 235;
            if emph_bits & 0x20 == 0 {
                r = (r as u16 * ATTEN_NUM / ATTEN_DEN) as u8;
            }
            if emph_bits & 0x40 == 0 {
                g = (g as u16 * ATTEN_NUM / ATTEN_DEN) as u8;
            }
            if emph_bits & 0x80 == 0 {
                b = (b as u16 * ATTEN_NUM / ATTEN_DEN) as u8;
            }
        }

        let x = (dot - 1) as usize;
        let y = scanline as usize;
        let offset = (y * 256 + x) * 4;
        ppu.framebuffer[offset..offset + 4].copy_from_slice(&[r, g, b, 0xFF]);
    }

    #[inline]
    fn evaluate_sprites_for_scanline(&mut self, target: u16) {
        let sprite_height: u16 = if self.ppu.regs.tall_sprites() { 16 } else { 8 };
        let pattern_base = self.ppu.regs.sprite_pattern_addr();

        self.ppu.sprite_count = 0;
        self.ppu.sprite_zero_rendering = false;
        self.ppu.sprite_patterns_lo = [0; 8];
        self.ppu.sprite_patterns_hi = [0; 8];
        self.ppu.sprite_attribs = [0; 8];
        self.ppu.sprite_x_counters = [0xFF; 8];
        self.ppu.overflow_bug_m = 0;

        let mut count: u8 = 0;

        for i in 0..64usize {
            let base = i * 4;

            let oam_y = if count >= 8 {
                self.ppu.oam[(base + self.ppu.overflow_bug_m as usize) & 0xFF] as u16
            } else {
                self.ppu.oam[base] as u16
            };

            let effective_y = oam_y.wrapping_add(1);
            let diff = target.wrapping_sub(effective_y);
            if diff >= sprite_height {
                if count >= 8 {
                    self.ppu.overflow_bug_m = self.ppu.overflow_bug_m.wrapping_add(1) & 0x03;
                }
                continue;
            }

            if count >= 8 {
                self.ppu.regs.set_sprite_overflow();
                break;
            }

            if i == 0 {
                self.ppu.sprite_zero_rendering = true;
            }

            let tile_index = self.ppu.oam[base + 1];
            let attributes = self.ppu.oam[base + 2];
            let sprite_x = self.ppu.oam[base + 3];
            let flip_h = attributes & 0x40 != 0;
            let flip_v = attributes & 0x80 != 0;

            let mut row = diff;
            if flip_v {
                row = sprite_height - 1 - row;
            }

            let lo_addr = if sprite_height == 8 {
                pattern_base + (tile_index as u16) * 16 + row
            } else {
                let bank = (tile_index as u16 & 0x01) * 0x1000;
                let tile = tile_index as u16 & 0xFE;
                if row < 8 {
                    bank + tile * 16 + row
                } else {
                    bank + (tile + 1) * 16 + (row - 8)
                }
            };
            let hi_addr = lo_addr + 8;

            let mut lo = self.ppu_bus_read_with_kind(lo_addr, ChrFetchKind::Sprite);
            let mut hi = self.ppu_bus_read_with_kind(hi_addr, ChrFetchKind::Sprite);

            if flip_h {
                lo = lo.reverse_bits();
                hi = hi.reverse_bits();
            }

            let idx = count as usize;
            self.ppu.sprite_patterns_lo[idx] = lo;
            self.ppu.sprite_patterns_hi[idx] = hi;
            self.ppu.sprite_attribs[idx] = attributes;
            self.ppu.sprite_x_counters[idx] = sprite_x;

            count += 1;
        }

        self.ppu.sprite_count = count;
    }
}
