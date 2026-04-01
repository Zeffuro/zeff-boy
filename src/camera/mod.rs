use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[cfg(feature = "camera")]
use std::time::Instant;

mod image_processing;

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

pub(crate) struct CameraCapture {
    latest_frame: Arc<Mutex<Vec<u8>>>,
    config: Arc<Mutex<CameraHostSettings>>,
    running: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl CameraCapture {
    pub(crate) fn start(initial_settings: CameraHostSettings) -> Self {
        let latest_frame = Arc::new(Mutex::new(checkerboard_frame()));
        let config = Arc::new(Mutex::new(initial_settings));
        let running = Arc::new(AtomicBool::new(true));
        let force_pattern = env_flag(CAMERA_FORCE_PATTERN_ENV);

        let thread_frame = Arc::clone(&latest_frame);
        #[cfg(feature = "camera")]
        let thread_config = Arc::clone(&config);
        let thread_running = Arc::clone(&running);

        let join = thread::spawn(move || {
            if force_pattern {
                log::info!(
                    "Pocket Camera: forcing test pattern due to {}=1",
                    CAMERA_FORCE_PATTERN_ENV
                );
                run_capture_loop_fallback(thread_frame, thread_running);
                return;
            }

            #[cfg(feature = "camera")]
            {
                run_capture_loop_with_webcam(thread_frame, thread_config, thread_running);
            }

            #[cfg(not(feature = "camera"))]
            {
                log::info!("Pocket Camera: webcam feature disabled, using fallback pattern");
                run_capture_loop_fallback(thread_frame, thread_running);
            }
        });

        Self {
            latest_frame,
            config,
            running,
            join: Some(join),
        }
    }

    pub(crate) fn update_settings(&self, settings: CameraHostSettings) {
        if let Ok(mut cfg) = self.config.lock() {
            *cfg = settings;
        }
    }

    pub(crate) fn latest_frame(&self) -> Vec<u8> {
        self.latest_frame
            .lock()
            .map(|f| f.clone())
            .unwrap_or_else(|_| checkerboard_frame())
    }

    pub(crate) fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(join) = self.join.take()
            && join.join().is_err()
        {
            log::error!("camera capture thread panicked");
        }
    }
}

impl Drop for CameraCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run_capture_loop_fallback(latest_frame: Arc<Mutex<Vec<u8>>>, running: Arc<AtomicBool>) {
    let frame = checkerboard_frame();
    while running.load(Ordering::Relaxed) {
        if let Ok(mut dst) = latest_frame.lock() {
            *dst = frame.clone();
        }
        thread::sleep(Duration::from_millis(33));
    }
}

#[cfg(feature = "camera")]
fn run_capture_loop_with_webcam(
    latest_frame: Arc<Mutex<Vec<u8>>>,
    config: Arc<Mutex<CameraHostSettings>>,
    running: Arc<AtomicBool>,
) {
    use nokhwa::{
        Camera,
        pixel_format::RgbFormat,
        utils::{CameraIndex, RequestedFormat, RequestedFormatType},
    };
    use image_processing::{
        apply_host_postprocess, avg_luma, decode_compressed_to_grayscale_nearest,
        rgb_to_grayscale_nearest, rgba_to_grayscale_nearest,
    };

    let camera_index = current_camera_settings(&config).device_index;
    log::info!("Pocket Camera: attempting webcam index {}", camera_index);

    let mut camera = match Camera::new(
        CameraIndex::Index(camera_index),
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate),
    ) {
        Ok(c) => c,
        Err(err) => {
            log::warn!(
                "Pocket Camera: failed to open webcam index {}, using fallback pattern: {}",
                camera_index,
                err
            );
            run_capture_loop_fallback(latest_frame, running);
            return;
        }
    };

    if let Err(err) = camera.open_stream() {
        log::warn!(
            "Pocket Camera: failed to start webcam stream on index {}, using fallback pattern: {}",
            camera_index,
            err
        );
        run_capture_loop_fallback(latest_frame, running);
        return;
    }

    log::info!(
        "Pocket Camera: webcam stream active on index {}",
        camera_index
    );
    log::info!("Pocket Camera: host post-process active (settings tab controls)");

    let mut last_good = checkerboard_frame();
    let mut ok_frames: u64 = 0;
    let mut fail_frames: u64 = 0;
    let mut last_avg_luma: u8 = 0;
    let mut last_log = Instant::now();
    let mut warn_budget: u32 = 0;

    while running.load(Ordering::Relaxed) {
        let next = match camera.frame() {
            Ok(raw) => {
                let res = raw.resolution();
                let src_w = res.width() as usize;
                let src_h = res.height() as usize;

                let maybe_frame = match raw.decode_image::<RgbFormat>() {
                    Ok(decoded) => Some(rgb_to_grayscale_nearest(
                        decoded.as_raw(),
                        src_w,
                        src_h,
                        CAMERA_WIDTH,
                        CAMERA_HEIGHT,
                    )),
                    Err(decode_err) => {
                        let src = raw.buffer();
                        let maybe_compressed = decode_compressed_to_grayscale_nearest(
                            src,
                            CAMERA_WIDTH,
                            CAMERA_HEIGHT,
                        );
                        let maybe_raw = if maybe_compressed.is_some() {
                            maybe_compressed
                        } else if src.len() >= src_w.saturating_mul(src_h).saturating_mul(4) {
                            Some(rgba_to_grayscale_nearest(
                                src,
                                src_w,
                                src_h,
                                CAMERA_WIDTH,
                                CAMERA_HEIGHT,
                            ))
                        } else if src.len() >= src_w.saturating_mul(src_h).saturating_mul(3) {
                            Some(rgb_to_grayscale_nearest(
                                src,
                                src_w,
                                src_h,
                                CAMERA_WIDTH,
                                CAMERA_HEIGHT,
                            ))
                        } else {
                            None
                        };

                        if maybe_raw.is_none() && warn_budget < 6 {
                            warn_budget = warn_budget.saturating_add(1);
                            log::warn!(
                                "Pocket Camera: frame decode failed on index {} ({}), buffer={} bytes for {}x{}",
                                camera_index,
                                decode_err,
                                src.len(),
                                src_w,
                                src_h
                            );
                        }

                        maybe_raw
                    }
                };

                if let Some(frame) = maybe_frame {
                    let mut frame = frame;
                    let settings = current_camera_settings(&config);
                    apply_host_postprocess(&mut frame, settings);
                    ok_frames = ok_frames.saturating_add(1);
                    last_avg_luma = avg_luma(&frame);
                    last_good = frame.clone();
                    frame
                } else {
                    fail_frames = fail_frames.saturating_add(1);
                    last_good.clone()
                }
            }
            Err(err) => {
                fail_frames = fail_frames.saturating_add(1);
                if warn_budget < 6 {
                    warn_budget = warn_budget.saturating_add(1);
                    log::warn!(
                        "Pocket Camera: webcam frame grab failed (index {}): {}",
                        camera_index,
                        err
                    );
                }
                last_good.clone()
            }
        };

        if let Ok(mut dst) = latest_frame.lock() {
            *dst = next;
        }

        if last_log.elapsed() >= Duration::from_secs(1) {
            log::info!(
                "Pocket Camera: frame stats index {} ok={} fail={} avg_luma={}",
                camera_index,
                ok_frames,
                fail_frames,
                last_avg_luma
            );
            ok_frames = 0;
            fail_frames = 0;
            last_log = Instant::now();
        }

        thread::sleep(Duration::from_millis(10));
    }

    if let Err(e) = camera.stop_stream() {
        log::warn!("failed to stop camera stream: {e}");
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

#[cfg(feature = "camera")]
fn current_camera_settings(config: &Arc<Mutex<CameraHostSettings>>) -> CameraHostSettings {
    config.lock().map(|v| *v).unwrap_or_default()
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
