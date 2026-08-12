use super::*;

impl Vdp {
    pub fn step_cycles(&mut self, cycles: u32) {
        self.scanline_cycle = self.scanline_cycle.wrapping_add(cycles);
        while self.scanline_cycle >= SMS_SCANLINE_Z80_CYCLES {
            self.scanline_cycle -= SMS_SCANLINE_Z80_CYCLES;
            self.advance_scanline();
        }
        self.h_counter = ((self.scanline_cycle * 256) / SMS_SCANLINE_Z80_CYCLES) as u8;
    }

    fn advance_scanline(&mut self) {
        let completed_scanline = self.scanline;
        self.latch_scanline_display_enabled(completed_scanline);
        self.render_presented_scanline(completed_scanline);
        if self.mode4_enabled() {
            self.evaluate_mode4_sprite_status_for_scanline(completed_scanline);
        } else {
            self.evaluate_tms_sprite_status_for_scanline(completed_scanline);
        }
        self.step_line_counter_for_scanline(completed_scanline);
        self.scanline += 1;
        if self.scanline == self.mode4_display_height().frame_interrupt_scanline() {
            self.status |= VDP_STATUS_VBLANK;
        } else if self.scanline >= self.total_scanlines() {
            self.scanline = 0;
            self.status &= !VDP_STATUS_VBLANK;
        }
        self.update_v_counter();
        self.scanline_start_registers = self.registers;
    }

    fn render_presented_scanline(&mut self, scanline: u16) {
        let visible_lines = if self.mode4_enabled() {
            self.mode4_display_height().lines()
        } else {
            SMS_VISIBLE_SCANLINES
        };
        if scanline >= visible_lines || usize::from(scanline) >= VDP_MAX_VISIBLE_SCANLINES {
            return;
        }
        if scanline == 0 {
            self.clear_presented_frame_history();
        }

        let source_y = usize::from(scanline);
        let area = Mode4RenderArea::new(VDP_PRESENTED_FRAME_WIDTH, 1, 0, source_y);
        let mut scanline_rgba = [0; VDP_PRESENTED_SCANLINE_BYTES];
        let current_registers = self.registers;
        self.registers = self.scanline_start_registers;
        if self.mode4_enabled() {
            render::render_mode4_frame_rgba(self, &mut scanline_rgba, area, self.color_mode);
        } else {
            render::render_tms9918_frame_rgba(
                self,
                &mut scanline_rgba,
                area,
                self.tms_presented_color_mode(),
            );
        }
        self.registers = current_registers;

        let row_start = source_y * VDP_PRESENTED_SCANLINE_BYTES;
        let row_end = row_start + VDP_PRESENTED_SCANLINE_BYTES;
        self.presented_framebuffer[row_start..row_end].copy_from_slice(&scanline_rgba);
        self.presented_scanline_valid[source_y] = true;
    }

    fn latch_scanline_display_enabled(&mut self, scanline: u16) {
        let visible_lines = if self.mode4_enabled() {
            self.mode4_display_height().lines()
        } else {
            SMS_VISIBLE_SCANLINES
        };
        let display_enabled = self.scanline_start_registers[VDP_REGISTER_MODE_CONTROL_2]
            & VDP_REG1_DISPLAY_ENABLE
            != 0;
        if scanline < visible_lines
            && let Some(enabled) = self.scanline_display_enabled.get_mut(usize::from(scanline))
        {
            *enabled = display_enabled;
        }
    }

    fn step_line_counter_for_scanline(&mut self, scanline: u16) {
        if scanline > self.visible_scanlines_for_timing() {
            self.line_counter = self.registers[VDP_REGISTER_LINE_COUNTER];
            return;
        }

        if self.line_counter == 0 {
            self.line_counter = self.registers[VDP_REGISTER_LINE_COUNTER];
            self.line_interrupt_pending = true;
        } else {
            self.line_counter = self.line_counter.wrapping_sub(1);
        }
    }

    pub(super) fn update_v_counter(&mut self) {
        self.v_counter = self
            .video_standard
            .v_counter_for_scanline(self.mode4_display_height(), self.scanline);
    }

    pub(super) fn update_scanline_start_registers_if_not_rendering_visible_line(&mut self) {
        let visible_lines = if self.mode4_enabled() {
            self.mode4_display_height().lines()
        } else {
            SMS_VISIBLE_SCANLINES
        };
        if self.scanline_cycle == 0 || self.scanline >= visible_lines {
            self.scanline_start_registers = self.registers;
        }
    }

    pub(super) fn scanline_display_enabled_for_source_y(&self, source_y: usize) -> bool {
        self.scanline_display_enabled
            .get(source_y)
            .copied()
            .unwrap_or(false)
    }

    pub(super) fn copy_presented_area_rgba(
        &self,
        framebuffer: &mut [u8],
        area: Mode4RenderArea,
    ) -> bool {
        let expected_len = area.expected_rgba_len();
        if framebuffer.len() < expected_len
            || area.source_x + area.width > VDP_PRESENTED_FRAME_WIDTH
            || area.source_y + area.height > VDP_MAX_VISIBLE_SCANLINES
        {
            return false;
        }
        if !self.presented_scanline_valid[area.source_y..area.source_y + area.height]
            .iter()
            .all(|valid| *valid)
        {
            return false;
        }

        for dest_y in 0..area.height {
            let src_y = area.source_y + dest_y;
            let src_start = src_y * VDP_PRESENTED_SCANLINE_BYTES + area.source_x * RGBA_CHANNELS;
            let src_end = src_start + area.width * RGBA_CHANNELS;
            let dest_start = dest_y * area.width * RGBA_CHANNELS;
            let dest_end = dest_start + area.width * RGBA_CHANNELS;
            framebuffer[dest_start..dest_end]
                .copy_from_slice(&self.presented_framebuffer[src_start..src_end]);
        }
        true
    }
}
