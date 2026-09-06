//! Small fixed-frame libretro frontend for local ABI and comparator measurements.

use libloading::{Library, Symbol};
use sha2::{Digest, Sha256};
use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::os::raw::c_uint;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const RETRO_DEVICE_JOYPAD: c_uint = 1;
const RETRO_ENVIRONMENT_GET_CAN_DUPE: c_uint = 3;
const RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL: c_uint = 8;
const RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY: c_uint = 9;
const RETRO_ENVIRONMENT_SET_PIXEL_FORMAT: c_uint = 10;
const RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS: c_uint = 11;
const RETRO_ENVIRONMENT_GET_VARIABLE: c_uint = 15;
const RETRO_ENVIRONMENT_SET_VARIABLES: c_uint = 16;
const RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE: c_uint = 17;
const RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME: c_uint = 18;
const RETRO_ENVIRONMENT_GET_LOG_INTERFACE: c_uint = 27;
const RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY: c_uint = 31;
const RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO: c_uint = 32;
const RETRO_ENVIRONMENT_SET_MEMORY_MAPS: c_uint = 36;
const RETRO_ENVIRONMENT_SET_GEOMETRY: c_uint = 37;
const RETRO_ENVIRONMENT_GET_LANGUAGE: c_uint = 39;
const RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS: c_uint = 42;
const RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE: c_uint = 47 | 0x10000;
const RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION: c_uint = 52;
const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2: c_uint = 67;
const RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL: c_uint = 68;
const RETRO_PIXEL_FORMAT_0RGB1555: c_uint = 0;
const RETRO_PIXEL_FORMAT_XRGB8888: c_uint = 1;
const RETRO_PIXEL_FORMAT_RGB565: c_uint = 2;
const RETRO_MEMORY_SAVE_RAM: c_uint = 0;
const RETRO_NUM_CORE_OPTION_VALUES_MAX: usize = 128;
const MAX_CAPTURE_BYTES: usize = 256 * 1024 * 1024;
const MAX_AUDIO_TELEMETRY_FRAMES: usize = 100_000;
const MAX_RAW_AUDIO_CAPTURE_BYTES: usize = 256 * 1024 * 1024;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RetroGameGeometry {
    pub base_width: c_uint,
    pub base_height: c_uint,
    pub max_width: c_uint,
    pub max_height: c_uint,
    pub aspect_ratio: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RetroSystemTiming {
    fps: f64,
    sample_rate: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
struct RetroSystemAvInfo {
    geometry: RetroGameGeometry,
    timing: RetroSystemTiming,
}

#[repr(C)]
struct RetroGameInfo {
    path: *const i8,
    data: *const c_void,
    size: usize,
    meta: *const i8,
}

#[repr(C)]
struct RetroVariable {
    key: *const i8,
    value: *const i8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct RetroCoreOptionValue {
    value: *const i8,
    label: *const i8,
}

#[repr(C)]
struct RetroCoreOptionV2Definition {
    key: *const i8,
    desc: *const i8,
    desc_categorized: *const i8,
    info: *const i8,
    info_categorized: *const i8,
    category_key: *const i8,
    values: [RetroCoreOptionValue; RETRO_NUM_CORE_OPTION_VALUES_MAX],
    default_value: *const i8,
}

#[repr(C)]
struct RetroCoreOptionsV2 {
    categories: *const c_void,
    definitions: *const RetroCoreOptionV2Definition,
}

#[repr(C)]
struct RetroCoreOptionsV2Intl {
    us: *const RetroCoreOptionsV2,
    local: *const RetroCoreOptionsV2,
}

#[repr(C)]
struct RetroLogCallback {
    log: Option<unsafe extern "C" fn(c_int, *const c_char, ...)>,
}

#[link(name = "zeff_libretro_harness_log", kind = "static")]
unsafe extern "C" {
    fn zeff_libretro_harness_log(level: c_int, format: *const c_char, ...);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelFormat {
    Xrgb8888,
    Rgb565,
}

impl PixelFormat {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "xrgb8888" => Some(Self::Xrgb8888),
            "rgb565" => Some(Self::Rgb565),
            _ => None,
        }
    }

    const fn retro_value(self) -> c_uint {
        match self {
            Self::Xrgb8888 => RETRO_PIXEL_FORMAT_XRGB8888,
            Self::Rgb565 => RETRO_PIXEL_FORMAT_RGB565,
        }
    }
}

impl std::fmt::Display for PixelFormat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Xrgb8888 => "XRGB8888",
            Self::Rgb565 => "RGB565",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JoypadInput {
    pub frame: usize,
    pub port: c_uint,
    /// Bit `n` is libretro joypad ID `n`, so `0x0100` is A.
    pub mask: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoreOption {
    pub key: String,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct FrameCaptureRequest {
    /// Absolute libretro frame index, including warmup frames.
    pub frame: usize,
    /// Fresh PNG destination. Existing files are never replaced.
    pub path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct HarnessConfig {
    pub core_path: PathBuf,
    pub content_path: PathBuf,
    pub rom_bytes: Vec<u8>,
    pub warmup_frames: usize,
    pub measurement_frames: usize,
    pub pixel_format: PixelFormat,
    pub inputs: Vec<JoypadInput>,
    pub core_options: Vec<CoreOption>,
    pub system_directory: Option<PathBuf>,
    pub save_directory: Option<PathBuf>,
    pub frame_capture: Option<FrameCaptureRequest>,
    /// Fresh CSV destination for generic per-emulated-frame audio telemetry.
    pub audio_frame_csv: Option<PathBuf>,
    pub audio_s16le: Option<PathBuf>,
    pub blackhole_output: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CallbackCounts {
    pub video_calls: usize,
    pub video_bytes: usize,
    pub visible_video_bytes: usize,
    pub audio_sample_calls: usize,
    pub audio_batch_calls: usize,
    pub audio_frames: usize,
    pub audio_bytes: usize,
    pub input_poll_calls: usize,
    pub input_state_calls: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct VideoFrameInfo {
    pub width: c_uint,
    pub height: c_uint,
    pub pitch: usize,
}

#[derive(Clone, Debug)]
pub struct HarnessResult {
    pub elapsed: Duration,
    pub frames_per_second: f64,
    pub callbacks: CallbackCounts,
    pub video_pixel_format: c_uint,
    pub invalid_audio_buffer_len: bool,
    pub callback_payload_hashing: bool,
    pub video_hash: Option<[u8; 32]>,
    pub audio_hash: Option<[u8; 32]>,
    pub unsupported_environment_commands: Vec<c_uint>,
    pub geometry: RetroGameGeometry,
    pub advertised_frames_per_second: f64,
    pub advertised_sample_rate: f64,
    pub last_video: VideoFrameInfo,
    pub serialize_size: usize,
    pub serialize_hash: [u8; 32],
    pub save_ram_size: usize,
    pub save_ram_nonnull: bool,
    pub save_ram_sha256: Option<[u8; 32]>,
    pub save_ram_post_roundtrip_sha256: Option<[u8; 32]>,
    pub state_roundtrip: bool,
    pub undersized_serialize_rejected: bool,
}

#[derive(Clone, Debug)]
pub struct RepeatedHarnessResult {
    pub runs: Vec<HarnessResult>,
    pub fps_p50: f64,
    pub fps_p95: f64,
    pub elapsed_ms_p50: f64,
    pub elapsed_ms_p95: f64,
    pub state_hashes_match: bool,
    pub video_hashes_match: Option<bool>,
    pub audio_hashes_match: Option<bool>,
    pub callback_counts_match: bool,
}

struct CoreOptionValue {
    key: CString,
    value: CString,
}

#[derive(Clone, Debug)]
struct CapturedFrame {
    width: u32,
    height: u32,
    rgb24: Vec<u8>,
}

#[derive(Clone, Default)]
struct FrameAudioStats {
    sample_calls: usize,
    batch_calls: usize,
    frames: usize,
    bytes: usize,
    hasher: Sha256,
}

struct FrameAudioReport {
    frame: usize,
    sample_calls: usize,
    batch_calls: usize,
    frames: usize,
    bytes: usize,
    hash: [u8; 32],
}

#[derive(Default)]
struct CallbackState {
    requested_pixel_format: c_uint,
    active_pixel_format: c_uint,
    frame_index: usize,
    inputs: Vec<JoypadInput>,
    options: Vec<CoreOptionValue>,
    system_directory: Option<CString>,
    save_directory: Option<CString>,
    counts: CallbackCounts,
    last_video: VideoFrameInfo,
    video_hasher: Sha256,
    audio_hasher: Sha256,
    invalid_video_pitch: bool,
    invalid_video_buffer_len: bool,
    invalid_audio_buffer_len: bool,
    capture_frame: Option<usize>,
    captured_frame: Option<CapturedFrame>,
    capture_error: Option<String>,
    frame_audio: Option<Vec<FrameAudioStats>>,
    audio_s16le: Option<BufWriter<File>>,
    audio_s16le_bytes: usize,
    audio_s16le_error: Option<String>,
    capture_audio_s16le: bool,
    blackhole_output: bool,
    invalid_audio_frame_index: bool,
    unsupported_environment_commands: Vec<c_uint>,
}

static CALLBACK_STATE: Mutex<Option<CallbackState>> = Mutex::new(None);

unsafe extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
    let mut state = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let state = state.as_mut().expect("libretro harness is not active");
    match cmd {
        RETRO_ENVIRONMENT_SET_PIXEL_FORMAT if !data.is_null() => {
            let format = unsafe { *data.cast::<c_uint>() };
            if format == state.requested_pixel_format {
                state.active_pixel_format = format;
                true
            } else {
                false
            }
        }
        RETRO_ENVIRONMENT_GET_CAN_DUPE if !data.is_null() => {
            unsafe { *data.cast::<bool>() = true };
            true
        }
        RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY => copy_directory(data, &state.system_directory),
        RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY => copy_directory(data, &state.save_directory),
        RETRO_ENVIRONMENT_GET_LOG_INTERFACE if !data.is_null() => {
            unsafe { (*data.cast::<RetroLogCallback>()).log = Some(zeff_libretro_harness_log) };
            true
        }
        RETRO_ENVIRONMENT_GET_VARIABLE if !data.is_null() => get_variable(data, state),
        RETRO_ENVIRONMENT_SET_VARIABLES if !data.is_null() => {
            unsafe { register_default_variables(data, state) };
            true
        }
        RETRO_ENVIRONMENT_GET_CORE_OPTIONS_VERSION if !data.is_null() => {
            unsafe { *data.cast::<c_uint>() = 2 };
            true
        }
        RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2 if !data.is_null() => {
            unsafe { register_default_v2_variables(data, state) };
            true
        }
        RETRO_ENVIRONMENT_SET_CORE_OPTIONS_V2_INTL if !data.is_null() => {
            unsafe { register_default_v2_intl_variables(data, state) };
            true
        }
        RETRO_ENVIRONMENT_GET_LANGUAGE if !data.is_null() => {
            unsafe { *data.cast::<c_uint>() = 0 };
            true
        }
        RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE if !data.is_null() => {
            unsafe { *data.cast::<bool>() = false };
            true
        }
        RETRO_ENVIRONMENT_GET_AUDIO_VIDEO_ENABLE if !data.is_null() => {
            unsafe { *data.cast::<c_uint>() = 3 };
            true
        }
        RETRO_ENVIRONMENT_SET_PERFORMANCE_LEVEL
        | RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS
        | RETRO_ENVIRONMENT_SET_SUPPORT_NO_GAME
        | RETRO_ENVIRONMENT_SET_SYSTEM_AV_INFO
        | RETRO_ENVIRONMENT_SET_MEMORY_MAPS
        | RETRO_ENVIRONMENT_SET_GEOMETRY
        | RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS => true,
        _ => {
            if !state.unsupported_environment_commands.contains(&cmd) {
                state.unsupported_environment_commands.push(cmd);
            }
            false
        }
    }
}

fn copy_directory(data: *mut c_void, directory: &Option<CString>) -> bool {
    let Some(directory) = directory else {
        return false;
    };
    unsafe { *data.cast::<*const i8>() = directory.as_ptr() };
    true
}

fn get_variable(data: *mut c_void, state: &mut CallbackState) -> bool {
    let variable = unsafe { &mut *data.cast::<RetroVariable>() };
    variable.value = std::ptr::null();
    if variable.key.is_null() {
        return false;
    }
    let key = unsafe { CStr::from_ptr(variable.key) };
    let Some(option) = state
        .options
        .iter()
        .find(|option| option.key.as_c_str() == key)
    else {
        return false;
    };
    variable.value = option.value.as_ptr();
    true
}

unsafe fn register_default_variables(data: *mut c_void, state: &mut CallbackState) {
    let mut variables = data.cast::<RetroVariable>();
    while !(unsafe { (*variables).key }).is_null() {
        let key = unsafe { CStr::from_ptr((*variables).key) };
        let value = unsafe { (*variables).value };
        if !value.is_null() {
            let value = unsafe { CStr::from_ptr(value) };
            if let Some(default) = default_core_option(value) {
                register_default_option(key, default, state);
            }
        }
        variables = unsafe { variables.add(1) };
    }
}

unsafe fn register_default_v2_variables(data: *mut c_void, state: &mut CallbackState) {
    let options = unsafe { &*data.cast::<RetroCoreOptionsV2>() };
    unsafe { register_default_v2_option_definitions(options.definitions, state) };
}

unsafe fn register_default_v2_intl_variables(data: *mut c_void, state: &mut CallbackState) {
    let options = unsafe { &*data.cast::<RetroCoreOptionsV2Intl>() };
    if !options.us.is_null() {
        let options = unsafe { &*options.us };
        unsafe { register_default_v2_option_definitions(options.definitions, state) };
    }
}

unsafe fn register_default_v2_option_definitions(
    mut definitions: *const RetroCoreOptionV2Definition,
    state: &mut CallbackState,
) {
    if definitions.is_null() {
        return;
    }
    while !(unsafe { (*definitions).key }).is_null() {
        let key = unsafe { CStr::from_ptr((*definitions).key) };
        let value = unsafe { (*definitions).default_value };
        if !value.is_null() {
            register_default_option(key, unsafe { CStr::from_ptr(value) }.to_owned(), state);
        }
        definitions = unsafe { definitions.add(1) };
    }
}

fn register_default_option(key: &CStr, value: CString, state: &mut CallbackState) {
    if !state
        .options
        .iter()
        .any(|option| option.key.as_c_str() == key)
    {
        state.options.push(CoreOptionValue {
            key: key.to_owned(),
            value,
        });
    }
}

fn default_core_option(value: &CStr) -> Option<CString> {
    let values = value.to_bytes().splitn(2, |byte| *byte == b';').nth(1)?;
    let default = values
        .iter()
        .copied()
        .skip_while(|byte| byte.is_ascii_whitespace())
        .take_while(|byte| *byte != b'|')
        .collect::<Vec<_>>();
    CString::new(default).ok()
}

unsafe extern "C" fn video_refresh(
    data: *const c_void,
    width: c_uint,
    height: c_uint,
    pitch: usize,
) {
    let mut state = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let state = state.as_mut().expect("libretro harness is not active");
    state.counts.video_calls += 1;
    if !data.is_null() {
        state.counts.video_bytes = state
            .counts
            .video_bytes
            .saturating_add(pitch.saturating_mul(height as usize));
    }
    state.last_video = VideoFrameInfo {
        width,
        height,
        pitch,
    };
    if !data.is_null() && width != 0 && height != 0 && pitch == 0 {
        state.invalid_video_pitch = true;
        return;
    }
    if !data.is_null() && pitch != 0 {
        let bytes_per_pixel = match state.active_pixel_format {
            RETRO_PIXEL_FORMAT_XRGB8888 => 4,
            RETRO_PIXEL_FORMAT_0RGB1555 | RETRO_PIXEL_FORMAT_RGB565 => 2,
            _ => unreachable!(),
        };
        let visible_row_bytes = (width as usize).saturating_mul(bytes_per_pixel);
        if pitch < visible_row_bytes {
            state.invalid_video_pitch = true;
            return;
        }
        let Some(frame_bytes) = pitch
            .checked_mul(height as usize)
            .filter(|length| *length <= isize::MAX as usize)
        else {
            state.invalid_video_buffer_len = true;
            return;
        };
        state.counts.visible_video_bytes = state
            .counts
            .visible_video_bytes
            .saturating_add(visible_row_bytes.saturating_mul(height as usize));
        if !state.blackhole_output {
            let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), frame_bytes) };
            state
                .video_hasher
                .update(state.active_pixel_format.to_le_bytes());
            state.video_hasher.update(width.to_le_bytes());
            state.video_hasher.update(height.to_le_bytes());
            for row in bytes.chunks_exact(pitch).take(height as usize) {
                state.video_hasher.update(&row[..visible_row_bytes]);
            }
            if state.capture_frame == Some(state.frame_index) {
                match capture_rgb24(bytes, width, height, pitch, state.active_pixel_format) {
                    Ok(captured) => state.captured_frame = Some(captured),
                    Err(error) => state.capture_error = Some(error.to_string()),
                }
            }
        }
    }
}

unsafe extern "C" fn audio_sample(left: i16, right: i16) {
    let mut state = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let state = state.as_mut().expect("libretro harness is not active");
    state.counts.audio_sample_calls += 1;
    state.counts.audio_frames += 1;
    state.counts.audio_bytes += std::mem::size_of::<i16>() * 2;
    if !state.blackhole_output {
        state.audio_hasher.update(left.to_le_bytes());
        state.audio_hasher.update(right.to_le_bytes());
        let [left_low, left_high] = left.to_le_bytes();
        let [right_low, right_high] = right.to_le_bytes();
        write_audio_s16le(state, &[left_low, left_high, right_low, right_high]);
        record_frame_audio(state, 1, 0, 1, std::mem::size_of::<i16>() * 2, |hasher| {
            hasher.update(left.to_le_bytes());
            hasher.update(right.to_le_bytes());
        });
    }
}

unsafe extern "C" fn audio_batch(data: *const i16, frames: usize) -> usize {
    let mut state = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let state = state.as_mut().expect("libretro harness is not active");
    state.counts.audio_batch_calls += 1;
    state.counts.audio_frames = state.counts.audio_frames.saturating_add(frames);
    state.counts.audio_bytes = state
        .counts
        .audio_bytes
        .saturating_add(frames.saturating_mul(std::mem::size_of::<i16>() * 2));
    if !data.is_null() {
        let Some(sample_count) = frames
            .checked_mul(2)
            .filter(|count| *count <= isize::MAX as usize)
        else {
            state.invalid_audio_buffer_len = true;
            return 0;
        };
        if !state.blackhole_output {
            let samples = unsafe { std::slice::from_raw_parts(data, sample_count) };
            for sample in samples {
                state.audio_hasher.update(sample.to_le_bytes());
            }
            for sample in samples {
                write_audio_s16le(state, &sample.to_le_bytes());
            }
            record_frame_audio(
                state,
                0,
                1,
                frames,
                frames.saturating_mul(std::mem::size_of::<i16>() * 2),
                |hasher| {
                    for sample in samples {
                        hasher.update(sample.to_le_bytes());
                    }
                },
            );
        }
    } else {
        record_frame_audio(
            state,
            0,
            1,
            frames,
            frames.saturating_mul(std::mem::size_of::<i16>() * 2),
            |_| {},
        );
    }
    frames
}

fn write_audio_s16le(state: &mut CallbackState, bytes: &[u8]) {
    if !state.capture_audio_s16le || state.audio_s16le_error.is_some() {
        return;
    }
    let Some(total) = state.audio_s16le_bytes.checked_add(bytes.len()) else {
        state.audio_s16le_error = Some("raw audio capture length overflowed".into());
        return;
    };
    if total > MAX_RAW_AUDIO_CAPTURE_BYTES {
        state.audio_s16le_error = Some(format!(
            "raw audio capture exceeds {MAX_RAW_AUDIO_CAPTURE_BYTES}-byte limit"
        ));
        return;
    }
    let Some(writer) = state.audio_s16le.as_mut() else {
        return;
    };
    if let Err(error) = writer.write_all(bytes) {
        state.audio_s16le_error = Some(format!("failed to write raw audio capture: {error}"));
        return;
    }
    state.audio_s16le_bytes = total;
}

fn record_frame_audio(
    state: &mut CallbackState,
    sample_calls: usize,
    batch_calls: usize,
    frames: usize,
    bytes: usize,
    update: impl FnOnce(&mut Sha256),
) {
    let Some(frame_audio) = state.frame_audio.as_mut() else {
        return;
    };
    let Some(frame) = frame_audio.get_mut(state.frame_index) else {
        state.invalid_audio_frame_index = true;
        return;
    };
    frame.sample_calls = frame.sample_calls.saturating_add(sample_calls);
    frame.batch_calls = frame.batch_calls.saturating_add(batch_calls);
    frame.frames = frame.frames.saturating_add(frames);
    frame.bytes = frame.bytes.saturating_add(bytes);
    update(&mut frame.hasher);
}

fn capture_rgb24(
    bytes: &[u8],
    width: c_uint,
    height: c_uint,
    pitch: usize,
    pixel_format: c_uint,
) -> anyhow::Result<CapturedFrame> {
    let width = usize::try_from(width)?;
    let height = usize::try_from(height)?;
    let pixel_count = width
        .checked_mul(height)
        .ok_or_else(|| anyhow::anyhow!("captured frame dimensions overflow"))?;
    let output_len = pixel_count
        .checked_mul(3)
        .filter(|length| *length <= MAX_CAPTURE_BYTES)
        .ok_or_else(|| anyhow::anyhow!("captured RGB frame exceeds {MAX_CAPTURE_BYTES} bytes"))?;
    let bytes_per_pixel = match pixel_format {
        RETRO_PIXEL_FORMAT_XRGB8888 => 4,
        RETRO_PIXEL_FORMAT_0RGB1555 | RETRO_PIXEL_FORMAT_RGB565 => 2,
        _ => anyhow::bail!("cannot capture unsupported libretro pixel format {pixel_format}"),
    };
    let mut rgb24 = Vec::with_capacity(output_len);
    for row in bytes.chunks_exact(pitch).take(height) {
        for pixel in row.chunks_exact(bytes_per_pixel).take(width) {
            match pixel_format {
                RETRO_PIXEL_FORMAT_XRGB8888 => {
                    let value = u32::from_ne_bytes(pixel.try_into().expect("four-byte pixel"));
                    rgb24.extend_from_slice(&[
                        ((value >> 16) as u8),
                        ((value >> 8) as u8),
                        value as u8,
                    ]);
                }
                RETRO_PIXEL_FORMAT_0RGB1555 => {
                    let value = u16::from_ne_bytes(pixel.try_into().expect("two-byte pixel"));
                    rgb24.extend_from_slice(&[
                        expand_5bit(((value >> 10) & 0x1F) as u8),
                        expand_5bit(((value >> 5) & 0x1F) as u8),
                        expand_5bit((value & 0x1F) as u8),
                    ]);
                }
                RETRO_PIXEL_FORMAT_RGB565 => {
                    let value = u16::from_ne_bytes(pixel.try_into().expect("two-byte pixel"));
                    rgb24.extend_from_slice(&[
                        expand_5bit(((value >> 11) & 0x1F) as u8),
                        expand_6bit(((value >> 5) & 0x3F) as u8),
                        expand_5bit((value & 0x1F) as u8),
                    ]);
                }
                _ => unreachable!(),
            }
        }
    }
    anyhow::ensure!(
        rgb24.len() == output_len,
        "captured frame had insufficient visible pixels"
    );
    Ok(CapturedFrame {
        width: u32::try_from(width)?,
        height: u32::try_from(height)?,
        rgb24,
    })
}

const fn expand_5bit(value: u8) -> u8 {
    (value << 3) | (value >> 2)
}

const fn expand_6bit(value: u8) -> u8 {
    (value << 2) | (value >> 4)
}

unsafe extern "C" fn input_poll() {
    let mut state = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    state
        .as_mut()
        .expect("libretro harness is not active")
        .counts
        .input_poll_calls += 1;
}

unsafe extern "C" fn input_state(port: c_uint, device: c_uint, index: c_uint, id: c_uint) -> i16 {
    let mut state = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let state = state.as_mut().expect("libretro harness is not active");
    state.counts.input_state_calls += 1;
    if device != RETRO_DEVICE_JOYPAD || index != 0 || id >= u16::BITS {
        return 0;
    }
    let mask = state
        .inputs
        .iter()
        .rfind(|input| input.port == port && input.frame <= state.frame_index)
        .map_or(0, |input| input.mask);
    i16::from(mask & (1 << id) != 0)
}

pub fn run_fixed_frames(config: &HarnessConfig) -> anyhow::Result<HarnessResult> {
    anyhow::ensure!(
        config.measurement_frames > 0,
        "measurement_frames must be nonzero"
    );
    anyhow::ensure!(
        !config.rom_bytes.is_empty(),
        "the libretro harness requires nonempty content bytes"
    );
    anyhow::ensure!(
        !config.blackhole_output
            || (config.frame_capture.is_none()
                && config.audio_frame_csv.is_none()
                && config.audio_s16le.is_none()),
        "blackhole output cannot be combined with video or audio capture"
    );
    let telemetry_frame_count = config
        .warmup_frames
        .checked_add(config.measurement_frames)
        .ok_or_else(|| anyhow::anyhow!("warmup and measurement frame counts overflow"))?;
    if config.audio_frame_csv.is_some() {
        anyhow::ensure!(
            telemetry_frame_count <= MAX_AUDIO_TELEMETRY_FRAMES,
            "per-frame audio telemetry is limited to {MAX_AUDIO_TELEMETRY_FRAMES} frames"
        );
    }
    if let Some(capture) = &config.frame_capture {
        anyhow::ensure!(
            capture.frame < telemetry_frame_count,
            "capture frame {} is outside the {} emulated frames",
            capture.frame,
            telemetry_frame_count
        );
    }
    let content_path = CString::new(config.content_path.to_string_lossy().as_bytes())
        .map_err(|_| anyhow::anyhow!("content path contains an interior NUL"))?;
    let library = unsafe { Library::new(&config.core_path) }.map_err(|error| {
        anyhow::anyhow!("failed to load '{}': {error}", config.core_path.display())
    })?;
    let mut inputs = config.inputs.clone();
    inputs.sort_by_key(|input| (input.frame, input.port));
    let options = config
        .core_options
        .iter()
        .map(|option| {
            Ok(CoreOptionValue {
                key: CString::new(option.key.as_bytes()).map_err(|_| {
                    anyhow::anyhow!("core option key '{}' contains an interior NUL", option.key)
                })?,
                value: CString::new(option.value.as_bytes()).map_err(|_| {
                    anyhow::anyhow!(
                        "core option value for '{}' contains an interior NUL",
                        option.key
                    )
                })?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let audio_s16le = config
        .audio_s16le
        .as_ref()
        .map(|path| {
            File::create_new(path).map(BufWriter::new).map_err(|error| {
                anyhow::anyhow!(
                    "failed to create raw audio capture '{}': {error}",
                    path.display()
                )
            })
        })
        .transpose()?;
    *CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned") = Some(CallbackState {
        requested_pixel_format: config.pixel_format.retro_value(),
        inputs,
        options,
        system_directory: config
            .system_directory
            .as_deref()
            .map(path_to_cstring)
            .transpose()?,
        save_directory: config
            .save_directory
            .as_deref()
            .map(path_to_cstring)
            .transpose()?,
        capture_frame: config.frame_capture.as_ref().map(|capture| capture.frame),
        frame_audio: config
            .audio_frame_csv
            .as_ref()
            .map(|_| vec![FrameAudioStats::default(); telemetry_frame_count]),
        audio_s16le,
        blackhole_output: config.blackhole_output,
        ..CallbackState::default()
    });

    let result = unsafe { run_loaded_core(&library, config, &content_path) };
    *CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned") = None;
    result
}

pub fn run_repeated_fixed_frames(
    config: &HarnessConfig,
    repeats: usize,
) -> anyhow::Result<RepeatedHarnessResult> {
    anyhow::ensure!(repeats > 0, "repeats must be nonzero");
    anyhow::ensure!(
        repeats == 1
            || (config.frame_capture.is_none()
                && config.audio_frame_csv.is_none()
                && config.audio_s16le.is_none()),
        "frame capture and audio capture require --repeat 1"
    );
    let mut runs = Vec::with_capacity(repeats);
    for _ in 0..repeats {
        runs.push(run_fixed_frames(config)?);
    }
    let mut fps = runs
        .iter()
        .map(|run| run.frames_per_second)
        .collect::<Vec<_>>();
    fps.sort_by(f64::total_cmp);
    let mut elapsed_ms = runs
        .iter()
        .map(|run| run.elapsed.as_secs_f64() * 1_000.0)
        .collect::<Vec<_>>();
    elapsed_ms.sort_by(f64::total_cmp);
    Ok(RepeatedHarnessResult {
        fps_p50: percentile(&fps, 0.5),
        fps_p95: percentile(&fps, 0.95),
        elapsed_ms_p50: percentile(&elapsed_ms, 0.5),
        elapsed_ms_p95: percentile(&elapsed_ms, 0.95),
        state_hashes_match: all_equal(runs.iter().map(|run| run.serialize_hash)),
        video_hashes_match: repeated_callback_hashes_match(
            !config.blackhole_output,
            runs.iter().map(|run| run.video_hash),
        ),
        audio_hashes_match: repeated_callback_hashes_match(
            !config.blackhole_output,
            runs.iter().map(|run| run.audio_hash),
        ),
        callback_counts_match: all_equal(runs.iter().map(|run| run.callbacks)),
        runs,
    })
}

fn path_to_cstring(path: &Path) -> anyhow::Result<CString> {
    CString::new(path.to_string_lossy().as_bytes())
        .map_err(|_| anyhow::anyhow!("path '{}' contains an interior NUL", path.display()))
}

fn percentile(values: &[f64], percentile: f64) -> f64 {
    debug_assert!(!values.is_empty());
    let index = ((values.len() - 1) as f64 * percentile).round() as usize;
    values[index]
}

fn all_equal<T: PartialEq>(mut values: impl Iterator<Item = T>) -> bool {
    let Some(first) = values.next() else {
        return true;
    };
    values.all(|value| value == first)
}

fn repeated_callback_hashes_match<T: PartialEq>(
    hashing_enabled: bool,
    hashes: impl Iterator<Item = Option<T>>,
) -> Option<bool> {
    hashing_enabled.then(|| {
        all_equal(hashes.map(|hash| hash.expect("enabled callback hashing produced a hash")))
    })
}

unsafe fn run_loaded_core(
    library: &Library,
    config: &HarnessConfig,
    content_path: &CString,
) -> anyhow::Result<HarnessResult> {
    let set_environment: Symbol<
        unsafe extern "C" fn(unsafe extern "C" fn(c_uint, *mut c_void) -> bool),
    > = unsafe { library.get(b"retro_set_environment\0") }?;
    let set_video: Symbol<
        unsafe extern "C" fn(unsafe extern "C" fn(*const c_void, c_uint, c_uint, usize)),
    > = unsafe { library.get(b"retro_set_video_refresh\0") }?;
    let set_audio: Symbol<unsafe extern "C" fn(unsafe extern "C" fn(i16, i16))> =
        unsafe { library.get(b"retro_set_audio_sample\0") }?;
    let set_audio_batch: Symbol<
        unsafe extern "C" fn(unsafe extern "C" fn(*const i16, usize) -> usize),
    > = unsafe { library.get(b"retro_set_audio_sample_batch\0") }?;
    let set_input_poll: Symbol<unsafe extern "C" fn(unsafe extern "C" fn())> =
        unsafe { library.get(b"retro_set_input_poll\0") }?;
    let set_input_state: Symbol<
        unsafe extern "C" fn(unsafe extern "C" fn(c_uint, c_uint, c_uint, c_uint) -> i16),
    > = unsafe { library.get(b"retro_set_input_state\0") }?;
    let init: Symbol<unsafe extern "C" fn()> = unsafe { library.get(b"retro_init\0") }?;
    let deinit: Symbol<unsafe extern "C" fn()> = unsafe { library.get(b"retro_deinit\0") }?;
    let load_game: Symbol<unsafe extern "C" fn(*const RetroGameInfo) -> bool> =
        unsafe { library.get(b"retro_load_game\0") }?;
    let unload_game: Symbol<unsafe extern "C" fn()> =
        unsafe { library.get(b"retro_unload_game\0") }?;
    let get_av_info: Symbol<unsafe extern "C" fn(*mut RetroSystemAvInfo)> =
        unsafe { library.get(b"retro_get_system_av_info\0") }?;
    let run: Symbol<unsafe extern "C" fn()> = unsafe { library.get(b"retro_run\0") }?;
    let serialize_size: Symbol<unsafe extern "C" fn() -> usize> =
        unsafe { library.get(b"retro_serialize_size\0") }?;
    let serialize: Symbol<unsafe extern "C" fn(*mut c_void, usize) -> bool> =
        unsafe { library.get(b"retro_serialize\0") }?;
    let unserialize: Symbol<unsafe extern "C" fn(*const c_void, usize) -> bool> =
        unsafe { library.get(b"retro_unserialize\0") }?;
    let get_memory_data: Symbol<unsafe extern "C" fn(c_uint) -> *mut c_void> =
        unsafe { library.get(b"retro_get_memory_data\0") }?;
    let get_memory_size: Symbol<unsafe extern "C" fn(c_uint) -> usize> =
        unsafe { library.get(b"retro_get_memory_size\0") }?;

    unsafe {
        set_environment(environment);
        set_video(video_refresh);
        set_audio(audio_sample);
        set_audio_batch(audio_batch);
        set_input_poll(input_poll);
        set_input_state(input_state);
        init();
    }
    let game = RetroGameInfo {
        path: content_path.as_ptr(),
        data: config.rom_bytes.as_ptr().cast(),
        size: config.rom_bytes.len(),
        meta: std::ptr::null(),
    };
    if !unsafe { load_game(&game) } {
        unsafe { deinit() };
        anyhow::bail!(
            "retro_load_game rejected '{}'",
            config.content_path.display()
        );
    }

    let mut av_info = std::mem::MaybeUninit::<RetroSystemAvInfo>::zeroed();
    unsafe { get_av_info(av_info.as_mut_ptr()) };
    let av_info = unsafe { av_info.assume_init() };
    for frame in 0..config.warmup_frames {
        set_frame_index(frame);
        unsafe { run() };
    }
    reset_measurement_callbacks();
    let start = Instant::now();
    for offset in 0..config.measurement_frames {
        set_frame_index(config.warmup_frames + offset);
        unsafe { run() };
    }
    let elapsed = start.elapsed();
    let save_ram = unsafe { snapshot_save_ram(*get_memory_data, *get_memory_size)? };
    let state_size = unsafe { serialize_size() };
    anyhow::ensure!(state_size > 0, "retro_serialize_size returned zero");
    let undersized_serialize_rejected =
        unsafe { serialize_rejects_undersized_buffer(*serialize, state_size)? };
    let mut state = vec![0; state_size];
    anyhow::ensure!(
        unsafe { serialize(state.as_mut_ptr().cast(), state_size) },
        "retro_serialize failed"
    );
    let state_hash: [u8; 32] = Sha256::digest(&state).into();
    let state_roundtrip = unsafe { unserialize(state.as_ptr().cast(), state_size) }
        && unsafe { serialize(state.as_mut_ptr().cast(), state_size) }
        && Sha256::digest(&state).as_slice() == state_hash;
    let save_ram_post_roundtrip = unsafe { snapshot_save_ram(*get_memory_data, *get_memory_size)? };
    unsafe {
        unload_game();
        deinit();
    }
    let mut callback_guard = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let callback_state = callback_guard
        .as_mut()
        .expect("libretro harness is not active");
    validate_callback_buffers(callback_state)?;
    anyhow::ensure!(
        !callback_state.invalid_audio_frame_index,
        "libretro core emitted audio outside the requested telemetry frame range"
    );
    if let Some(error) = &callback_state.capture_error {
        anyhow::bail!("failed to capture requested frame: {error}");
    }
    if let Some(error) = &callback_state.audio_s16le_error {
        anyhow::bail!("{error}");
    }
    if let Some(writer) = callback_state.audio_s16le.as_mut() {
        writer
            .flush()
            .map_err(|error| anyhow::anyhow!("failed to flush raw audio capture: {error}"))?;
    }
    let captured_frame = callback_state.captured_frame.clone();
    let frame_audio = callback_state.frame_audio.as_ref().map(|frames| {
        frames
            .iter()
            .enumerate()
            .map(|(frame, stats)| FrameAudioReport {
                frame,
                sample_calls: stats.sample_calls,
                batch_calls: stats.batch_calls,
                frames: stats.frames,
                bytes: stats.bytes,
                hash: stats.hasher.clone().finalize().into(),
            })
            .collect::<Vec<_>>()
    });
    let result = HarnessResult {
        elapsed,
        frames_per_second: config.measurement_frames as f64 / elapsed.as_secs_f64(),
        callbacks: callback_state.counts,
        video_pixel_format: callback_state.active_pixel_format,
        invalid_audio_buffer_len: callback_state.invalid_audio_buffer_len,
        callback_payload_hashing: !callback_state.blackhole_output,
        video_hash: (!callback_state.blackhole_output)
            .then(|| callback_state.video_hasher.clone().finalize().into()),
        audio_hash: (!callback_state.blackhole_output)
            .then(|| callback_state.audio_hasher.clone().finalize().into()),
        unsupported_environment_commands: callback_state.unsupported_environment_commands.clone(),
        geometry: av_info.geometry,
        advertised_frames_per_second: av_info.timing.fps,
        advertised_sample_rate: av_info.timing.sample_rate,
        last_video: callback_state.last_video,
        serialize_size: state_size,
        serialize_hash: state_hash,
        save_ram_size: save_ram.bytes.len(),
        save_ram_nonnull: save_ram.nonnull,
        save_ram_sha256: save_ram.hash,
        save_ram_post_roundtrip_sha256: save_ram_post_roundtrip.hash,
        state_roundtrip,
        undersized_serialize_rejected,
    };
    drop(callback_guard);
    if let Some(capture) = &config.frame_capture {
        let frame = captured_frame.ok_or_else(|| {
            anyhow::anyhow!(
                "the core produced no visible frame for capture frame {}",
                capture.frame
            )
        })?;
        write_capture_png(&capture.path, &frame)?;
    }
    if let Some(path) = &config.audio_frame_csv {
        write_audio_frame_csv(path, frame_audio.as_deref().expect("telemetry requested"))?;
    }
    Ok(result)
}

fn write_capture_png(path: &Path, frame: &CapturedFrame) -> anyhow::Result<()> {
    let file = File::create_new(path).map_err(|error| {
        anyhow::anyhow!("failed to create capture '{}': {error}", path.display())
    })?;
    let mut encoder = png::Encoder::new(BufWriter::new(file), frame.width, frame.height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|error| {
        anyhow::anyhow!("failed to start PNG capture '{}': {error}", path.display())
    })?;
    writer.write_image_data(&frame.rgb24).map_err(|error| {
        anyhow::anyhow!("failed to write PNG capture '{}': {error}", path.display())
    })
}

fn write_audio_frame_csv(path: &Path, frames: &[FrameAudioReport]) -> anyhow::Result<()> {
    let file = File::create_new(path).map_err(|error| {
        anyhow::anyhow!(
            "failed to create audio telemetry '{}': {error}",
            path.display()
        )
    })?;
    let mut writer = BufWriter::new(file);
    writeln!(
        writer,
        "frame,audio_sample_calls,audio_batch_calls,audio_frames,audio_bytes,audio_sha256"
    )?;
    for frame in frames {
        writeln!(
            writer,
            "{},{},{},{},{},{}",
            frame.frame,
            frame.sample_calls,
            frame.batch_calls,
            frame.frames,
            frame.bytes,
            sha256_hex(&frame.hash),
        )?;
    }
    writer.flush()?;
    Ok(())
}

fn sha256_hex(hash: &[u8; 32]) -> String {
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn validate_callback_buffers(callback_state: &CallbackState) -> anyhow::Result<()> {
    anyhow::ensure!(
        !callback_state.invalid_video_pitch,
        "libretro core supplied pitch {} for a {}x{} frame in pixel format {}",
        callback_state.last_video.pitch,
        callback_state.last_video.width,
        callback_state.last_video.height,
        callback_state.active_pixel_format
    );
    anyhow::ensure!(
        !callback_state.invalid_video_buffer_len,
        "libretro core supplied an invalid video buffer length"
    );
    anyhow::ensure!(
        !callback_state.invalid_audio_buffer_len,
        "libretro core supplied an invalid audio buffer length"
    );
    Ok(())
}

fn set_frame_index(frame_index: usize) {
    CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned")
        .as_mut()
        .expect("libretro harness is not active")
        .frame_index = frame_index;
}

unsafe fn serialize_rejects_undersized_buffer(
    serialize: unsafe extern "C" fn(*mut c_void, usize) -> bool,
    state_size: usize,
) -> anyhow::Result<bool> {
    if state_size <= 1 {
        return Ok(false);
    }
    let probe_size = state_size
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("retro_serialize_size is too large to probe"))?;
    let mut probe = vec![0; probe_size];
    Ok(!unsafe { serialize(probe.as_mut_ptr().cast(), state_size - 1) })
}

#[derive(Debug)]
struct SaveRamSnapshot {
    bytes: Vec<u8>,
    nonnull: bool,
    hash: Option<[u8; 32]>,
}

unsafe fn snapshot_save_ram(
    get_memory_data: unsafe extern "C" fn(c_uint) -> *mut c_void,
    get_memory_size: unsafe extern "C" fn(c_uint) -> usize,
) -> anyhow::Result<SaveRamSnapshot> {
    let size = unsafe { get_memory_size(RETRO_MEMORY_SAVE_RAM) };
    let data = unsafe { get_memory_data(RETRO_MEMORY_SAVE_RAM) };
    unsafe { copy_save_ram(data, size) }
}

unsafe fn copy_save_ram(data: *mut c_void, size: usize) -> anyhow::Result<SaveRamSnapshot> {
    anyhow::ensure!(
        size == 0 || !data.is_null(),
        "retro_get_memory_data returned null for {size} bytes of save RAM"
    );
    let bytes = if size == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) }.to_vec()
    };
    let hash = (!bytes.is_empty()).then(|| Sha256::digest(&bytes).into());
    Ok(SaveRamSnapshot {
        nonnull: !data.is_null(),
        bytes,
        hash,
    })
}

fn reset_measurement_callbacks() {
    let mut state = CALLBACK_STATE
        .lock()
        .expect("libretro callback mutex poisoned");
    let state = state.as_mut().expect("libretro harness is not active");
    state.counts = CallbackCounts::default();
    state.last_video = VideoFrameInfo::default();
    state.video_hasher = Sha256::default();
    state.audio_hasher = Sha256::default();
    state.audio_s16le_bytes = 0;
    state.capture_audio_s16le = true;
}

pub fn load_rom(path: &Path) -> anyhow::Result<Vec<u8>> {
    std::fs::read(path)
        .map_err(|error| anyhow::anyhow!("failed to read '{}': {error}", path.display()))
}

#[cfg(test)]
#[path = "libretro_harness/tests.rs"]
mod tests;
