use super::*;

impl App {
    pub(super) fn render_tick(&mut self) {
        let should_render = match self.speed_mode() {
            SpeedMode::Normal | SpeedMode::SlowMotion => {
                let background_ui = self.emu_thread.is_none() || {
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.show_settings_window
                            || self.show_mods_window
                            || self.show_cheats_window
                            || self.show_audio_explorer
                    }
                    #[cfg(target_arch = "wasm32")]
                    {
                        false
                    }
                };
                !background_ui
                    || Instant::now().duration_since(self.timing.last_render_time)
                        >= VIEWER_UPDATE_INTERVAL
            }
            SpeedMode::FastForward | SpeedMode::Uncapped => {
                let now = Instant::now();
                if now.duration_since(self.timing.last_render_time) >= UI_RENDER_INTERVAL {
                    self.timing.last_render_time = now;
                    true
                } else {
                    false
                }
            }
        };

        if !should_render {
            return;
        }
        self.timing.last_render_time = Instant::now();

        self.update_debug_cache_edges();

        if let Some(frame) = self.latest_frame.take() {
            self.last_core_frame = Some(frame.clone());
            let Some(display_frame) = self.display_frame_for_upload(frame) else {
                log::warn!(
                    "Skipping frame upload with unexpected WonderSwan size for {:?}",
                    self.active_system
                );
                return;
            };

            let Some((native_w, native_h)) = self.display_size_for_frame(&display_frame) else {
                log::warn!(
                    "Skipping frame upload with unexpected size: {} bytes for {:?}",
                    display_frame.len(),
                    self.active_system
                );
                return;
            };

            if let Some(gfx) = self.gfx.as_mut() {
                gfx.set_native_size(native_w, native_h);
                let (width, height) =
                    crate::app::display::presentation_size(self.active_system, native_w, native_h);
                gfx.set_presentation_size(width, height);
                gfx.upload_framebuffer(&display_frame);
            }
            self.last_displayed_frame = Some(display_frame);
        }

        self.debug_windows.memory.enable_editing = self.settings.ui.enable_memory_editing;

        let ui_frame_data = self.cached_ui_data.take();
        let rendered = self.render_frame(ui_frame_data.as_ref());
        #[cfg(not(target_arch = "wasm32"))]
        self.render_native_auxiliary_windows(ui_frame_data.as_ref());
        self.cached_ui_data = ui_frame_data;
        #[cfg(all(test, target_arch = "wasm32", feature = "wasm-browser-tests"))]
        super::browser_speculation_test::observe_app_tick(self, rendered);
        if !rendered {
            return;
        }

        self.debug_windows.tile_viewer_was_open =
            is_tab_open(&self.debug_dock, DebugTab::TileViewer);
        self.debug_windows.tilemap_viewer_was_open =
            is_tab_open(&self.debug_dock, DebugTab::TilemapViewer);
    }
}
