use std::sync::Arc;

use super::{ActiveSystem, App};
use crate::emu_thread::PublishedFramebuffer;
use zeff_emu_common::system::{
    RGBA_BYTES_PER_PIXEL, SUPER_GAME_BOY_SCREEN_SIZE, WS_SCREEN_SIZE, rgba_framebuffer_len,
};

const SGB_FRAME_LEN: usize = rgba_framebuffer_len(SUPER_GAME_BOY_SCREEN_SIZE);
const WS_FRAME_LEN: usize = rgba_framebuffer_len(WS_SCREEN_SIZE);

impl App {
    pub(super) fn display_size_for_frame(
        &self,
        frame: &PublishedFramebuffer,
    ) -> Option<(u32, u32)> {
        frame_dimensions(self.active_system, self.ws_display_rotated, frame)
    }

    pub(super) fn active_display_size(&self) -> (u32, u32) {
        if self.active_system == ActiveSystem::WonderSwan && self.ws_display_rotated {
            let (width, height) = WS_SCREEN_SIZE;
            (height, width)
        } else {
            self.active_system.screen_size()
        }
    }

    pub(super) fn display_frame_for_upload(
        &self,
        frame: Arc<PublishedFramebuffer>,
    ) -> Option<Arc<PublishedFramebuffer>> {
        if self.active_system != ActiveSystem::WonderSwan || !self.ws_display_rotated {
            return Some(frame);
        }

        if frame.len() != WS_FRAME_LEN {
            return None;
        }

        Some(Arc::new(rotate_ws_frame_ccw(&frame).into()))
    }

    pub(super) fn latest_display_frame_snapshot(&self) -> Option<Arc<PublishedFramebuffer>> {
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
        let raw_frame = self.last_core_frame.as_ref().or(self.latest_frame.as_ref());
        let display_frame =
            raw_frame.and_then(|frame| self.display_frame_for_upload(Arc::clone(frame)));
        let (native_w, native_h) = display_frame
            .as_ref()
            .and_then(|frame| self.display_size_for_frame(frame))
            .unwrap_or_else(|| self.active_display_size());

        if let Some(gfx) = self.gfx.as_mut() {
            gfx.set_native_size(native_w, native_h);
            if self.active_system == ActiveSystem::Pce {
                gfx.set_presentation_size(640, 480);
            }
            if let Some(frame) = display_frame.as_ref() {
                gfx.upload_framebuffer(frame);
            }
        }

        if let Some(frame) = display_frame {
            self.last_displayed_frame = Some(frame);
        }
    }
}

fn frame_dimensions(
    system: ActiveSystem,
    ws_rotated: bool,
    frame: &PublishedFramebuffer,
) -> Option<(u32, u32)> {
    if let Some((width, height)) = frame.dimensions() {
        if system != ActiveSystem::Pce
            || width == 0
            || height == 0
            || width > 640
            || height > 480
            || frame.len() != width as usize * height as usize * RGBA_BYTES_PER_PIXEL
        {
            return None;
        }
        return Some((width, height));
    }
    if system == ActiveSystem::GameBoy && frame.len() == SGB_FRAME_LEN {
        return Some(SUPER_GAME_BOY_SCREEN_SIZE);
    }
    if frame.len() != system.framebuffer_len() {
        return None;
    }
    if system == ActiveSystem::WonderSwan && ws_rotated {
        let (width, height) = WS_SCREEN_SIZE;
        Some((height, width))
    } else {
        Some(system.screen_size())
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
    fn native_dimensions_are_explicit_and_validated_before_upload() {
        use crate::emu_thread::framebuffer::{
            new_shared_framebuffer, publish_framebuffer_with_dimensions,
        };
        let shared = new_shared_framebuffer();
        publish_framebuffer_with_dimensions(&shared, &vec![0; 256 * 242 * 4], Some((256, 242)));
        let frame = shared.load_full().unwrap();
        assert_eq!(
            frame_dimensions(ActiveSystem::Pce, false, &frame),
            Some((256, 242))
        );
        assert_eq!(frame_dimensions(ActiveSystem::GameBoy, false, &frame), None);
        publish_framebuffer_with_dimensions(&shared, &vec![0; 256 * 242 * 4], Some((256, 240)));
        assert_eq!(
            frame_dimensions(ActiveSystem::Pce, false, &shared.load_full().unwrap()),
            None
        );
        publish_framebuffer_with_dimensions(&shared, &vec![0; 256 * 242 * 4], None);
        assert_eq!(
            frame_dimensions(ActiveSystem::Pce, false, &shared.load_full().unwrap()),
            None
        );
        let fixed = vec![0; 640 * 480 * 4].into();
        assert_eq!(
            frame_dimensions(ActiveSystem::Pce, false, &fixed),
            Some((640, 480))
        );
    }

    #[test]
    fn legacy_dynamic_layouts_keep_their_dimensions() {
        let sgb = vec![0; SGB_FRAME_LEN].into();
        assert_eq!(
            frame_dimensions(ActiveSystem::GameBoy, false, &sgb),
            Some(SUPER_GAME_BOY_SCREEN_SIZE)
        );
        let ws = vec![0; WS_FRAME_LEN].into();
        assert_eq!(
            frame_dimensions(ActiveSystem::WonderSwan, false, &ws),
            Some(WS_SCREEN_SIZE)
        );
        assert_eq!(
            frame_dimensions(ActiveSystem::WonderSwan, true, &ws),
            Some((144, 224))
        );
    }

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
