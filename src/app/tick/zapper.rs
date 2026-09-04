use super::super::{ActiveSystem, App};
use crate::emu_thread::ZapperInput;
use zeff_emu_common::system::{NES_SCREEN_SIZE, RGBA_BYTES_PER_PIXEL, rgba_framebuffer_len};

impl App {
    pub(in crate::app) fn nes_zapper_input(&self) -> ZapperInput {
        if self.active_system != ActiveSystem::Nes {
            return ZapperInput::default();
        }

        if let Some(zapper) = self.remote_zapper {
            return zapper;
        }

        if !self.settings.emulation.nes_zapper_enabled {
            return ZapperInput::default();
        }

        ZapperInput {
            enabled: true,
            trigger: self.mouse_left_pressed && self.game_view_focused,
            hit: self.nes_zapper_hit(),
            screen_pos: self.nes_zapper_screen_pos(),
        }
    }

    fn nes_zapper_screen_pos(&self) -> Option<(u16, u16)> {
        let (cursor_x, cursor_y) = self.cursor_pos?;
        let gfx = self.gfx.as_ref()?;
        let (pixel_x, pixel_y) = gfx.game_pixel_at_window_pos(cursor_x, cursor_y)?;
        let (nes_width, nes_height) = NES_SCREEN_SIZE;
        if pixel_x < nes_width && pixel_y < nes_height {
            Some((pixel_x as u16, pixel_y as u16))
        } else {
            None
        }
    }

    fn nes_zapper_hit(&self) -> bool {
        let Some((pixel_x, pixel_y)) = self.nes_zapper_screen_pos() else {
            return false;
        };
        let Some(frame) = self.latest_frame.as_ref() else {
            return false;
        };
        if frame.len() != rgba_framebuffer_len(NES_SCREEN_SIZE) {
            return false;
        }

        const SAMPLE_RADIUS: i32 = 4;
        let (nes_width, nes_height) = NES_SCREEN_SIZE;
        let max_x = nes_width as i32 - 1;
        let max_y = nes_height as i32 - 1;
        let center_x = pixel_x as i32;
        let center_y = pixel_y as i32;

        for y in (center_y - SAMPLE_RADIUS).max(0)..=(center_y + SAMPLE_RADIUS).min(max_y) {
            for x in (center_x - SAMPLE_RADIUS).max(0)..=(center_x + SAMPLE_RADIUS).min(max_x) {
                let idx = ((y as usize * nes_width as usize + x as usize) * RGBA_BYTES_PER_PIXEL)
                    .min(frame.len() - RGBA_BYTES_PER_PIXEL);
                if Self::nes_zapper_pixel_is_bright(frame[idx], frame[idx + 1], frame[idx + 2]) {
                    return true;
                }
            }
        }

        false
    }

    fn nes_zapper_pixel_is_bright(r: u8, g: u8, b: u8) -> bool {
        let min_component = r.min(g).min(b);
        let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
        min_component >= 160 && luma >= 190.0
    }
}
