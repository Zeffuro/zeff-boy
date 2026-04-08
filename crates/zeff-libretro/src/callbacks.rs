use crate::api::*;
use std::ffi::CStr;
use std::os::raw::{c_uint, c_void};
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

pub(crate) fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) static CORE: Mutex<Option<super::core::CoreState>> = Mutex::new(None);
pub(crate) static SRAM_BUF: Mutex<Vec<u8>> = Mutex::new(Vec::new());
pub(crate) static MAX_SERIALIZE_SIZE: Mutex<usize> = Mutex::new(0);
pub(crate) static FRAME_COUNTER: Mutex<u64> = Mutex::new(0);

pub(crate) static CB_ENVIRONMENT: Mutex<Option<retro_environment_t>> = Mutex::new(None);
pub(crate) static CB_VIDEO_REFRESH: Mutex<Option<retro_video_refresh_t>> = Mutex::new(None);
pub(crate) static CB_AUDIO_SAMPLE: Mutex<Option<retro_audio_sample_t>> = Mutex::new(None);
pub(crate) static CB_AUDIO_SAMPLE_BATCH: Mutex<Option<retro_audio_sample_batch_t>> =
    Mutex::new(None);
pub(crate) static CB_INPUT_POLL: Mutex<Option<retro_input_poll_t>> = Mutex::new(None);
pub(crate) static CB_INPUT_STATE: Mutex<Option<retro_input_state_t>> = Mutex::new(None);
pub(crate) static CB_LOG: Mutex<Option<retro_log_printf_t>> = Mutex::new(None);
pub(crate) static CB_RUMBLE: Mutex<Option<retro_rumble_set_state_t>> = Mutex::new(None);

pub(crate) static USE_XRGB8888: AtomicBool = AtomicBool::new(false);

pub(crate) const LIB_NAME: &CStr = c"zeff-boy";
pub(crate) const LIB_VERSION: &CStr = const {
    const BYTES: &[u8] = concat!(env!("CARGO_PKG_VERSION"), "\0").as_bytes();
    match CStr::from_bytes_with_nul(BYTES) {
        Ok(s) => s,
        Err(_) => unreachable!(),
    }
};
pub(crate) const VALID_EXTENSIONS: &CStr = c"gb|gbc|nes";

pub(crate) fn env_cmd(cmd: c_uint, data: *mut c_void) -> bool {
    if let Some(cb) = *lock(&CB_ENVIRONMENT) {
        unsafe { cb(cmd, data) }
    } else {
        false
    }
}

pub(crate) fn retro_log(level: c_uint, msg: &str) {
    if let Some(log_fn) = *lock(&CB_LOG) {
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        unsafe { log_fn(level, c"%s\n".as_ptr(), c_msg.as_ptr()) };
        return;
    }
    log_to_file(msg);
}

pub(crate) fn retro_log_info(msg: &str) {
    retro_log(RETRO_LOG_INFO, msg);
}

#[allow(unused)]
pub(crate) fn retro_log_warn(msg: &str) {
    retro_log(RETRO_LOG_WARN, msg);
}

pub(crate) fn retro_log_error(msg: &str) {
    retro_log(RETRO_LOG_ERROR, msg);
}

#[allow(unused)]
fn log_to_file(msg: &str) {
    use std::io::Write;
    let path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("zeff_libretro.log")))
        .unwrap_or_else(|| std::path::PathBuf::from("zeff_libretro.log"));
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{msg}");
    }
}
