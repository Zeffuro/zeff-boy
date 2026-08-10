use std::sync::Arc;

use super::{ActiveSystem, App};
use zeff_emu_common::system::{
    RGBA_BYTES_PER_PIXEL, SUPER_GAME_BOY_SCREEN_SIZE, WS_SCREEN_SIZE, rgba_framebuffer_len,
};

const SGB_FRAME_LEN: usize = rgba_framebuffer_len(SUPER_GAME_BOY_SCREEN_SIZE);
const WS_FRAME_LEN: usize = rgba_framebuffer_len(WS_SCREEN_SIZE);

impl App {
    pub(super) fn display_size_for_frame_len(&self, frame_len: usize) -> Option<(u32, u32)> {
        if self.active_system == ActiveSystem::GameBoy && frame_len == SGB_FRAME_LEN {
            return Some(SUPER_GAME_BOY_SCREEN_SIZE);
        }

        if frame_len != self.active_system.framebuffer_len() {
            return None;
        }

        if self.active_system == ActiveSystem::WonderSwan && self.ws_display_rotated {
            let (width, height) = WS_SCREEN_SIZE;
            Some((height, width))
        } else {
            Some(self.active_system.screen_size())
        }
    }

    pub(super) fn active_display_size(&self) -> (u32, u32) {
        if self.active_system == ActiveSystem::WonderSwan && self.ws_display_rotated {
            let (width, height) = WS_SCREEN_SIZE;
            (height, width)
        } else {
            self.active_system.screen_size()
        }
    }

    pub(super) fn display_frame_for_upload(&self, frame: Arc<Vec<u8>>) -> Option<Arc<Vec<u8>>> {
        if self.active_system != ActiveSystem::WonderSwan || !self.ws_display_rotated {
            return Some(frame);
        }

        if frame.len() != WS_FRAME_LEN {
            return None;
        }

        Some(Arc::new(rotate_ws_frame_ccw(&frame)))
    }

    pub(super) fn latest_display_frame_snapshot(&self) -> Option<Arc<Vec<u8>>> {
        self.last_displayed_frame.as_ref().cloned().or_else(|| {
            self.latest_frame
                .as_ref()
                .and_then(|frame| self.display_frame_for_upload(Arc::clone(frame)))
        })
    }

    pub(super) fn toggle_ws_rotation(&mut self) {
        if self.active_system != ActiveSystem::WonderSwan {
            return;
        }

        self.ws_display_rotated = !self.ws_display_rotated;
        self.apply_display_orientation();
        let label = if self.ws_display_rotated {
            "WonderSwan rotated"
        } else {
            "WonderSwan horizontal"
        };
        self.toast_manager.info(label);
    }

    pub(super) fn apply_display_orientation(&mut self) {
        let (native_w, native_h) = self.active_display_size();
        let raw_frame = self.last_core_frame.as_ref().or(self.latest_frame.as_ref());
        let display_frame =
            raw_frame.and_then(|frame| self.display_frame_for_upload(Arc::clone(frame)));

        if let Some(gfx) = self.gfx.as_mut() {
            gfx.set_native_size(native_w, native_h);
            if let Some(frame) = display_frame.as_ref() {
                gfx.upload_framebuffer(frame);
            }
        }

        if let Some(frame) = display_frame {
            self.last_displayed_frame = Some(frame);
        }
    }
}

fn rotate_ws_frame_ccw(frame: &[u8]) -> Vec<u8> {
    let (ws_width, ws_height) = WS_SCREEN_SIZE;
    let src_w = ws_width as usize;
    let src_h = ws_height as usize;
    let dst_w = src_h;
    let dst_h = src_w;
    let mut rotated = vec![0; frame.len()];

    for y in 0..src_h {
        for x in 0..src_w {
            let src = (y * src_w + x) * RGBA_BYTES_PER_PIXEL;
            let dst_x = y;
            let dst_y = src_w - 1 - x;
            let dst = (dst_y * dst_w + dst_x) * RGBA_BYTES_PER_PIXEL;
            rotated[dst..dst + RGBA_BYTES_PER_PIXEL]
                .copy_from_slice(&frame[src..src + RGBA_BYTES_PER_PIXEL]);
        }
    }

    debug_assert_eq!(dst_w * dst_h * RGBA_BYTES_PER_PIXEL, rotated.len());
    rotated
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotates_ws_frame_counter_clockwise() {
        let mut frame = vec![0; WS_FRAME_LEN];
        let (ws_width, ws_height) = WS_SCREEN_SIZE;
        set_pixel(&mut frame, 0, 0, [1, 2, 3, 4]);
        set_pixel(&mut frame, (ws_width - 1) as usize, 0, [5, 6, 7, 8]);
        set_pixel(&mut frame, 0, (ws_height - 1) as usize, [9, 10, 11, 12]);

        let rotated = rotate_ws_frame_ccw(&frame);

        assert_eq!(pixel(&rotated, 0, (ws_width - 1) as usize), [1, 2, 3, 4]);
        assert_eq!(pixel(&rotated, 0, 0), [5, 6, 7, 8]);
        assert_eq!(
            pixel(&rotated, (ws_height - 1) as usize, (ws_width - 1) as usize),
            [9, 10, 11, 12]
        );
    }

    fn set_pixel(frame: &mut [u8], x: usize, y: usize, rgba: [u8; 4]) {
        let idx = (y * WS_SCREEN_SIZE.0 as usize + x) * RGBA_BYTES_PER_PIXEL;
        frame[idx..idx + RGBA_BYTES_PER_PIXEL].copy_from_slice(&rgba);
    }

    fn pixel(frame: &[u8], x: usize, y: usize) -> [u8; 4] {
        let idx = (y * WS_SCREEN_SIZE.1 as usize + x) * RGBA_BYTES_PER_PIXEL;
        [frame[idx], frame[idx + 1], frame[idx + 2], frame[idx + 3]]
    }
}
