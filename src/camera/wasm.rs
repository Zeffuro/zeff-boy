use super::{CAMERA_HEIGHT, CAMERA_WIDTH, CameraHostSettings};

pub(crate) struct CameraCapture;

impl CameraCapture {
    pub(crate) fn start(_settings: CameraHostSettings) -> Self {
        Self
    }

    pub(crate) fn update_settings(&self, _settings: CameraHostSettings) {}

    pub(crate) fn latest_frame(&self) -> Vec<u8> {
        vec![128u8; CAMERA_WIDTH * CAMERA_HEIGHT]
    }

    pub(crate) fn stop(&mut self) {}
}
