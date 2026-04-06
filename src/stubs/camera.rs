pub(crate) const CAMERA_WIDTH: usize = 128;
pub(crate) const CAMERA_HEIGHT: usize = 112;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CameraDeviceInfo {
    pub(crate) index: u32,
    pub(crate) name: String,
}

pub(crate) fn host_camera_supported() -> bool {
    false
}

pub(crate) fn query_host_cameras() -> anyhow::Result<Vec<CameraDeviceInfo>> {
    anyhow::bail!("camera not supported on web")
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CameraHostSettings {
    pub(crate) device_index: u32,
    pub(crate) auto_levels: bool,
    pub(crate) gamma: f32,
    pub(crate) brightness: f32,
    pub(crate) contrast: f32,
}

impl Default for CameraHostSettings {
    fn default() -> Self {
        Self {
            device_index: 0,
            auto_levels: false,
            gamma: 1.0,
            brightness: 0.0,
            contrast: 1.0,
        }
    }
}

pub(crate) struct CameraCapture;

impl CameraCapture {
    pub(crate) fn start(_settings: CameraHostSettings) -> Self {
        Self
    }

    pub(crate) fn update_settings(&self, _settings: CameraHostSettings) {}

    pub(crate) fn latest_frame(&self) -> Vec<u8> {
        vec![128u8; CAMERA_WIDTH * CAMERA_HEIGHT]
    }
}


