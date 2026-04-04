use super::{App, CameraCapture, CameraHostSettings};

fn camera_settings_from_app(app: &App) -> CameraHostSettings {
    CameraHostSettings {
        device_index: app.settings.camera.device_index,
        auto_levels: app.settings.camera.auto_levels,
        gamma: app.settings.camera.gamma,
        brightness: app.settings.camera.brightness,
        contrast: app.settings.camera.contrast,
    }
}

impl App {
    pub(super) fn camera_frame(&mut self) -> Option<Vec<u8>> {
        if !self.rom_info.is_pocket_camera {
            self.stop_camera_capture();
            return None;
        }

        if self.camera_capture.is_none() {
            self.camera_capture = Some(CameraCapture::start(camera_settings_from_app(self)));
            self.camera_capture_index = Some(self.settings.camera.device_index);
            log::info!("Pocket Camera host capture started");
        } else if self.camera_capture_index != Some(self.settings.camera.device_index) {
            self.stop_camera_capture();
            self.camera_capture = Some(CameraCapture::start(camera_settings_from_app(self)));
            self.camera_capture_index = Some(self.settings.camera.device_index);
            log::info!(
                "Pocket Camera host capture restarted on device index {}",
                self.settings.camera.device_index
            );
        }

        if let Some(capture) = self.camera_capture.as_ref() {
            capture.update_settings(camera_settings_from_app(self));
        }

        self.camera_capture
            .as_ref()
            .map(CameraCapture::latest_frame)
    }

    pub(super) fn stop_camera_capture(&mut self) {
        if self.camera_capture.take().is_some() {
            self.camera_capture_index = None;
            log::info!("Pocket Camera host capture stopped");
        }
    }
}
