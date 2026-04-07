mod image_processing;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native::CameraCapture;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub(crate) use wasm::CameraCapture;

pub(crate) const CAMERA_WIDTH: usize = 128;
pub(crate) const CAMERA_HEIGHT: usize = 112;
const FRAME_LEN: usize = CAMERA_WIDTH * CAMERA_HEIGHT;
const CAMERA_FORCE_PATTERN_ENV: &str = "ZEFF_CAMERA_FORCE_PATTERN";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CameraDeviceInfo {
    pub(crate) index: u32,
    pub(crate) name: String,
}

pub(crate) fn host_camera_supported() -> bool {
    cfg!(feature = "camera")
}

#[cfg(feature = "camera")]
pub(crate) fn query_host_cameras() -> anyhow::Result<Vec<CameraDeviceInfo>> {
    use nokhwa::{
        query,
        utils::{ApiBackend, CameraIndex},
    };

    let devices = query(ApiBackend::Auto)
        .map_err(|e| anyhow::anyhow!("Unable to query host cameras: {e}"))?
        .into_iter()
        .filter_map(|info| {
            let idx = match info.index() {
                CameraIndex::Index(v) => *v,
                CameraIndex::String(s) => s.parse::<u32>().ok()?,
            };
            Some(CameraDeviceInfo {
                index: idx,
                name: info.human_name(),
            })
        })
        .collect::<Vec<_>>();

    Ok(devices)
}

#[cfg(not(feature = "camera"))]
pub(crate) fn query_host_cameras() -> anyhow::Result<Vec<CameraDeviceInfo>> {
    anyhow::bail!(
        "This build was compiled without host camera support (feature `camera` disabled)."
    )
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

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "1" || v == "true" || v == "yes" || v == "on"
        })
        .unwrap_or(false)
}

fn checkerboard_frame() -> Vec<u8> {
    let mut frame = vec![0u8; FRAME_LEN];
    for y in 0..CAMERA_HEIGHT {
        for x in 0..CAMERA_WIDTH {
            let block = ((x / 8) + (y / 8)) & 1;
            frame[y * CAMERA_WIDTH + x] = if block == 0 { 16 } else { 240 };
        }
    }
    frame
}

#[cfg(test)]
mod tests;
