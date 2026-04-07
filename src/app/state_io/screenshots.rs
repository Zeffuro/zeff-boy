use super::App;

impl App {
    pub(in crate::app) fn take_screenshot(&mut self) {
        let (native_w, native_h) = self.active_system.screen_size();
        let expected_len = (native_w * native_h * 4) as usize;
        let fb = match &self.last_displayed_frame {
            Some(fb) if fb.len() == expected_len => fb,
            _ => {
                self.toast_manager.error("No framebuffer available");
                return;
            }
        };

        let game_name = self
            .rom_info
            .rom_path
            .as_ref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("screenshot");

        let timestamp = crate::platform::timestamp_string();
        let filename = format!("{game_name}_{timestamp}.png");

        let image =
            egui::ColorImage::from_rgba_unmultiplied([native_w as usize, native_h as usize], fb);

        #[cfg(not(target_arch = "wasm32"))]
        {
            let dir = crate::platform::screenshots_dir();
            if let Err(e) = std::fs::create_dir_all(&dir) {
                self.toast_manager
                    .error(format!("Can't create screenshots dir: {e}"));
                return;
            }
            let path = dir.join(&filename);
            match crate::debug::export::export_color_image_as_png(&path, &image) {
                Ok(()) => {
                    log::info!("Screenshot saved to {}", path.display());
                    self.toast_manager.success(format!("📸 {filename}"));
                }
                Err(err) => {
                    log::error!("Failed to save screenshot: {}", err);
                    self.toast_manager
                        .error(format!("Screenshot failed: {err}"));
                }
            }
        }

        #[cfg(target_arch = "wasm32")]
        {
            match crate::debug::export::encode_color_image_as_png_bytes(&image) {
                Ok(bytes) => {
                    crate::platform::download_file(&filename, &bytes);
                    log::info!("Screenshot download triggered: {filename}");
                    self.toast_manager.success(format!("📸 {filename}"));
                }
                Err(err) => {
                    log::error!("Failed to encode screenshot: {}", err);
                    self.toast_manager
                        .error(format!("Screenshot failed: {err}"));
                }
            }
        }
    }
}
