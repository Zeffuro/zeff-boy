use super::{DOTS_PER_LINE, DRAW_DOTS_BASE, Lcdc, OAM_DOTS, PPU, SCREEN_H, renderer};

impl PPU {
    pub(super) fn window_enable_condition(&self) -> bool {
        self.lcdc.contains(Lcdc::WINDOW_ENABLE)
    }

    pub(super) fn window_visible_on_current_line(&self) -> bool {
        self.ly < SCREEN_H as u8
            && self.window_enable_condition()
            && self.window_y_triggered
            && self.wx <= 166
    }

    pub(super) fn increment_window_line_counter_after_scanline(&mut self) {
        if self.window_visible_on_current_line() {
            self.window_line_counter = self.window_line_counter.saturating_add(1);
            self.window_was_active_this_frame = true;
        }
    }

    fn compute_draw_dots_for_line(&self, oam: &[u8]) -> u64 {
        if self.ly >= 144 {
            return DRAW_DOTS_BASE;
        }

        let scx_penalty = (self.scx & 7) as u64;

        let sprite_penalty = if self.lcdc.contains(Lcdc::OBJ_ENABLE) {
            let tall = self.lcdc.contains(Lcdc::OBJ_SIZE);
            let sprite_h: i32 = if tall { 16 } else { 8 };
            let mut count: u64 = 0;
            for i in 0..40usize {
                let base = i * 4;
                if base + 3 >= oam.len() {
                    break;
                }
                let sy = oam[base] as i32 - 16;
                if (self.ly as i32) >= sy && (self.ly as i32) < sy + sprite_h {
                    count += 1;
                    if count >= 10 {
                        break;
                    }
                }
            }
            count * 6
        } else {
            0
        };

        let window_penalty = if self.window_visible_on_current_line() {
            6
        } else {
            0
        };

        DRAW_DOTS_BASE + scx_penalty + sprite_penalty + window_penalty
    }

    #[inline]
    pub(in crate::hardware) fn step(
        &mut self,
        cycles: u64,
        vram: &[u8],
        oam: &[u8],
        cgb_mode: bool,
    ) -> u8 {
        self.cgb_mode = cgb_mode;

        let lcd_enabled = self.lcdc.contains(Lcdc::LCD_ENABLE);

        if !lcd_enabled {
            self.lcd_was_enabled = false;
            self.blank_first_frame_after_lcd_on = false;

            self.cycles = 0;
            self.ly = 0;
            self.stat &= !0x03;
            self.window_line_counter = 0;
            self.window_was_active_this_frame = false;
            self.window_y_triggered = false;
            self.rendered_current_line = false;
            self.prev_stat_line = false;
            self.draw_dots_for_line = DRAW_DOTS_BASE;
            return 0;
        }

        if !self.lcd_was_enabled {
            self.lcd_was_enabled = true;
            self.blank_first_frame_after_lcd_on = true;

            self.cycles = 4;
            self.ly = 0;
            self.stat = (self.stat & !0x03) | 2;
            self.window_line_counter = 0;
            self.window_was_active_this_frame = false;
            self.window_y_triggered = false;
            self.rendered_current_line = false;
            self.prev_stat_line = false;
            self.draw_dots_for_line = DRAW_DOTS_BASE;
        }

        if self.ly == self.wy {
            self.window_y_triggered = true;
        }

        if self.ly < 144 && !self.rendered_current_line {
            self.draw_dots_for_line = self.compute_draw_dots_for_line(oam);
        }

        let previous_mode = self.stat & 0x03;
        let mut interrupts = 0u8;

        self.cycles += cycles;

        let should_render_output = !self.blank_first_frame_after_lcd_on;
        let draw_dots = self.draw_dots_for_line;

        if !self.rendered_current_line && self.cycles >= OAM_DOTS + draw_dots {
            if self.ly < 144 && should_render_output {
                if cgb_mode {
                    renderer::render_scanline_cgb(self, vram, oam);
                } else {
                    renderer::render_scanline_dmg(self, vram, oam);
                }
            }
            self.rendered_current_line = true;
        }

        while self.cycles >= DOTS_PER_LINE {
            self.cycles -= DOTS_PER_LINE;

            if !self.rendered_current_line && self.ly < 144 && should_render_output {
                if cgb_mode {
                    renderer::render_scanline_cgb(self, vram, oam);
                } else {
                    renderer::render_scanline_dmg(self, vram, oam);
                }
            }
            
            if self.ly < 144 {
                self.increment_window_line_counter_after_scanline();
            }

            self.ly += 1;
            self.rendered_current_line = false;

            if self.ly == 144 {
                interrupts |= 0x01;
            }

            if self.ly >= 154 {
                self.ly = 0;
                self.window_line_counter = 0;
                self.window_was_active_this_frame = false;
                self.window_y_triggered = false;

                if self.blank_first_frame_after_lcd_on {
                    self.blank_first_frame_after_lcd_on = false;
                }
            }

            if self.ly == self.wy {
                self.window_y_triggered = true;
            }

            if self.ly < 144 {
                self.draw_dots_for_line = self.compute_draw_dots_for_line(oam);
            }
        }

        let draw_dots = self.draw_dots_for_line;
        let current_mode = if self.ly >= 144 {
            1 // VBlank
        } else if self.cycles < OAM_DOTS {
            2 // OAM scan
        } else if self.cycles < OAM_DOTS + draw_dots {
            3 // Drawing
        } else {
            0 // HBlank
        };

        if current_mode != previous_mode {
            self.stat = (self.stat & !0x03) | current_mode;
        }

        if self.update_stat_interrupt() {
            interrupts |= 0x02;
        }

        interrupts
    }

    pub(in crate::hardware) fn mode(&self) -> u8 {
        self.stat & 0x03
    }

    pub(in crate::hardware) fn lcd_enabled(&self) -> bool {
        self.lcdc.contains(Lcdc::LCD_ENABLE)
    }

    pub(in crate::hardware) fn cpu_vram_accessible(&self) -> bool {
        !self.lcd_enabled() || self.mode() != 3
    }

    pub(in crate::hardware) fn cpu_oam_accessible(&self) -> bool {
        !self.lcd_enabled() || (self.mode() != 2 && self.mode() != 3)
    }

    pub(in crate::hardware::ppu) fn cpu_palette_accessible(&self) -> bool {
        !self.lcd_enabled() || self.mode() != 3
    }

    pub(super) fn update_stat_interrupt(&mut self) -> bool {
        let ly_match = self.ly == self.lyc;
        if ly_match {
            self.stat |= 0x04;
        } else {
            self.stat &= !0x04;
        }

        let mode = self.stat & 0x03;
        let stat_line = (self.stat & 0x40 != 0 && ly_match)
            || (self.stat & 0x20 != 0 && mode == 2)
            || (self.stat & 0x10 != 0 && mode == 1)
            || (self.stat & 0x08 != 0 && mode == 0);

        let rising_edge = stat_line && !self.prev_stat_line;
        self.prev_stat_line = stat_line;
        rising_edge
    }
}