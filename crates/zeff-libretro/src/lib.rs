// Libretro core — C ABI callbacks receiving raw pointers from the frontend.

mod api;
mod core;

use api::*;
use std::ffi::CStr;
use std::os::raw::{c_char, c_uint, c_void};
use std::panic::catch_unwind;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

// ── Global state ─────────────────────────────────────────────────────

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

static CORE: Mutex<Option<core::CoreState>> = Mutex::new(None);
static SRAM_BUF: Mutex<Vec<u8>> = Mutex::new(Vec::new());
static MAX_SERIALIZE_SIZE: Mutex<usize> = Mutex::new(0);
static FRAME_COUNTER: Mutex<u64> = Mutex::new(0);
#[allow(dead_code)]
static OPTIONS_DIRTY: Mutex<bool> = Mutex::new(false);

static CB_ENVIRONMENT: Mutex<Option<retro_environment_t>> = Mutex::new(None);
static CB_VIDEO_REFRESH: Mutex<Option<retro_video_refresh_t>> = Mutex::new(None);
static CB_AUDIO_SAMPLE: Mutex<Option<retro_audio_sample_t>> = Mutex::new(None);
static CB_AUDIO_SAMPLE_BATCH: Mutex<Option<retro_audio_sample_batch_t>> = Mutex::new(None);
static CB_INPUT_POLL: Mutex<Option<retro_input_poll_t>> = Mutex::new(None);
static CB_INPUT_STATE: Mutex<Option<retro_input_state_t>> = Mutex::new(None);
static CB_LOG: Mutex<Option<retro_log_printf_t>> = Mutex::new(None);
static CB_RUMBLE: Mutex<Option<retro_rumble_set_state_t>> = Mutex::new(None);

static USE_XRGB8888: AtomicBool = AtomicBool::new(false);

const LIB_NAME: &CStr = c"zeff-boy";
const LIB_VERSION: &CStr = c"0.1.0";
const VALID_EXTENSIONS: &CStr = c"gb|gbc|nes";

fn retro_log(level: c_uint, msg: &str) {
    if let Some(log_fn) = *lock(&CB_LOG) {
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();
        unsafe { log_fn(level, c"%s\n".as_ptr(), c_msg.as_ptr()) };
        return;
    }
    log_to_file(msg);
}

fn retro_log_info(msg: &str) {
    retro_log(RETRO_LOG_INFO, msg);
}

#[allow(unused)]
fn retro_log_warn(msg: &str) {
    retro_log(RETRO_LOG_WARN, msg);
}

#[allow(unused)]
fn retro_log_error(msg: &str) {
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

// ── Helpers ──────────────────────────────────────────────────────────

fn env_cmd(cmd: c_uint, data: *mut c_void) -> bool {
    if let Some(cb) = *lock(&CB_ENVIRONMENT) {
        unsafe { cb(cmd, data) }
    } else {
        false
    }
}

const OPT_DMG_PALETTE: &CStr = c"zeff_dmg_palette";
const OPT_NES_PALETTE: &CStr = c"zeff_nes_palette_mode";
const OPT_SGB_BORDER: &CStr = c"zeff_sgb_border";

fn set_core_options() {
    let vars: &[retro_variable] = &[
        retro_variable {
            key: OPT_DMG_PALETTE.as_ptr(),
            value: c"DMG Palette; DMG Green|Gray|Pocket|Mint|Chocolate".as_ptr(),
        },
        retro_variable {
            key: OPT_NES_PALETTE.as_ptr(),
            value: c"NES Palette Mode; Raw|NTSC|PAL".as_ptr(),
        },
        retro_variable {
            key: OPT_SGB_BORDER.as_ptr(),
            value: c"SGB Border; disabled|enabled".as_ptr(),
        },
        retro_variable {
            key: std::ptr::null(),
            value: std::ptr::null(),
        },
    ];
    env_cmd(
        RETRO_ENVIRONMENT_SET_VARIABLES,
        vars.as_ptr() as *mut c_void,
    );
}

fn get_variable(key: &CStr) -> Option<String> {
    let mut var = retro_variable {
        key: key.as_ptr(),
        value: std::ptr::null(),
    };
    let ok = env_cmd(
        RETRO_ENVIRONMENT_GET_VARIABLE,
        &mut var as *mut retro_variable as *mut c_void,
    );
    if ok && !var.value.is_null() {
        unsafe { CStr::from_ptr(var.value) }
            .to_str()
            .ok()
            .map(|s| s.to_string())
    } else {
        None
    }
}

fn check_variables_updated() -> bool {
    let mut updated: bool = false;
    env_cmd(
        RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
        &mut updated as *mut bool as *mut c_void,
    );
    updated
}

fn apply_core_options(state: &mut core::CoreState) {
    if let Some(val) = get_variable(OPT_DMG_PALETTE) {
        use zeff_gb_core::hardware::ppu::DmgPalettePreset;
        let preset = match val.as_str() {
            "Gray" => DmgPalettePreset::Gray,
            "Pocket" => DmgPalettePreset::Pocket,
            "Mint" => DmgPalettePreset::Mint,
            "Chocolate" => DmgPalettePreset::Chocolate,
            _ => DmgPalettePreset::DmgGreen,
        };
        state.set_dmg_palette(preset);
    }

    if let Some(val) = get_variable(OPT_NES_PALETTE) {
        use zeff_nes_core::hardware::ppu::NesPaletteMode;
        let mode = match val.as_str() {
            "NTSC" => NesPaletteMode::Ntsc,
            "PAL" => NesPaletteMode::Pal,
            _ => NesPaletteMode::Raw,
        };
        state.set_nes_palette_mode(mode);
    }

    if let Some(val) = get_variable(OPT_SGB_BORDER) {
        let enabled = val == "enabled";
        let was_active = state.sgb_border_active();
        state.set_sgb_border_enabled(enabled);
        let now_active = state.sgb_border_active();

        if was_active != now_active {
            let mut geom = retro_game_geometry {
                base_width: state.native_width(),
                base_height: state.native_height(),
                max_width: 256,
                max_height: 240,
                aspect_ratio: 0.0,
            };
            env_cmd(
                RETRO_ENVIRONMENT_SET_GEOMETRY,
                &mut geom as *mut retro_game_geometry as *mut c_void,
            );
            retro_log_info(&format!(
                "Geometry updated: {}x{}",
                geom.base_width, geom.base_height
            ));
        }
    }
}

fn poll_joypad_port(port: c_uint) -> (u8, u8) {
    let cb_state = *lock(&CB_INPUT_STATE);
    let query = |id: c_uint| -> bool {
        cb_state.is_some_and(|cb| unsafe { cb(port, RETRO_DEVICE_JOYPAD, 0, id) != 0 })
    };

    let mut buttons: u8 = 0;
    let mut dpad: u8 = 0;

    if query(RETRO_DEVICE_ID_JOYPAD_A) {
        buttons |= 0x01;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_B) {
        buttons |= 0x02;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_SELECT) {
        buttons |= 0x04;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_START) {
        buttons |= 0x08;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_RIGHT) {
        dpad |= 0x01;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_LEFT) {
        dpad |= 0x02;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_UP) {
        dpad |= 0x04;
    }
    if query(RETRO_DEVICE_ID_JOYPAD_DOWN) {
        dpad |= 0x08;
    }

    (buttons, dpad)
}

fn poll_lightgun_port(port: c_uint) -> (bool, bool) {
    let cb_state = *lock(&CB_INPUT_STATE);
    let query = |id: c_uint| -> bool {
        cb_state.is_some_and(|cb| unsafe { cb(port, RETRO_DEVICE_LIGHTGUN, 0, id) != 0 })
    };
    let trigger = query(RETRO_DEVICE_ID_LIGHTGUN_TRIGGER);
    let offscreen = query(RETRO_DEVICE_ID_LIGHTGUN_IS_OFFSCREEN);
    let hit = trigger && !offscreen;
    (trigger, hit)
}

fn set_input_descriptors(is_nes: bool) {
    let mut descs: Vec<retro_input_descriptor> = vec![
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_A,
            description: c"A".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_B,
            description: c"B".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_SELECT,
            description: c"Select".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_START,
            description: c"Start".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_UP,
            description: c"D-Pad Up".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_DOWN,
            description: c"D-Pad Down".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_LEFT,
            description: c"D-Pad Left".as_ptr(),
        },
        retro_input_descriptor {
            port: 0,
            device: RETRO_DEVICE_JOYPAD,
            index: 0,
            id: RETRO_DEVICE_ID_JOYPAD_RIGHT,
            description: c"D-Pad Right".as_ptr(),
        },
    ];

    if is_nes {
        descs.extend_from_slice(&[
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_A,
                description: c"A".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_B,
                description: c"B".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_SELECT,
                description: c"Select".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_START,
                description: c"Start".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_UP,
                description: c"D-Pad Up".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_DOWN,
                description: c"D-Pad Down".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_LEFT,
                description: c"D-Pad Left".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_JOYPAD,
                index: 0,
                id: RETRO_DEVICE_ID_JOYPAD_RIGHT,
                description: c"D-Pad Right".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_LIGHTGUN,
                index: 0,
                id: RETRO_DEVICE_ID_LIGHTGUN_TRIGGER,
                description: c"Zapper Trigger".as_ptr(),
            },
            retro_input_descriptor {
                port: 1,
                device: RETRO_DEVICE_LIGHTGUN,
                index: 0,
                id: RETRO_DEVICE_ID_LIGHTGUN_IS_OFFSCREEN,
                description: c"Zapper Off-screen".as_ptr(),
            },
        ]);
    }

    descs.push(retro_input_descriptor {
        port: 0,
        device: RETRO_DEVICE_NONE,
        index: 0,
        id: 0,
        description: std::ptr::null(),
    });

    env_cmd(
        RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS,
        descs.as_ptr() as *mut c_void,
    );
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_environment(cb: retro_environment_t) {
    *lock(&CB_ENVIRONMENT) = Some(cb);

    let _ = catch_unwind(|| {
        let mut log_cb = retro_log_callback { log: None };
        if env_cmd(
            RETRO_ENVIRONMENT_GET_LOG_INTERFACE,
            &mut log_cb as *mut retro_log_callback as *mut c_void,
        ) && let Some(log_fn) = log_cb.log
        {
            *lock(&CB_LOG) = Some(log_fn);
        }

        retro_log_info("retro_set_environment");

        set_core_options();

        let mut support_achievements: bool = true;
        env_cmd(
            RETRO_ENVIRONMENT_SET_SUPPORT_ACHIEVEMENTS,
            &mut support_achievements as *mut bool as *mut c_void,
        );
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_video_refresh(cb: retro_video_refresh_t) {
    *lock(&CB_VIDEO_REFRESH) = Some(cb);
}
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_audio_sample(cb: retro_audio_sample_t) {
    *lock(&CB_AUDIO_SAMPLE) = Some(cb);
}
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_audio_sample_batch(cb: retro_audio_sample_batch_t) {
    *lock(&CB_AUDIO_SAMPLE_BATCH) = Some(cb);
}
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_input_poll(cb: retro_input_poll_t) {
    *lock(&CB_INPUT_POLL) = Some(cb);
}
#[unsafe(no_mangle)]
pub extern "C" fn retro_set_input_state(cb: retro_input_state_t) {
    *lock(&CB_INPUT_STATE) = Some(cb);
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_init() {
    let _ = catch_unwind(|| {
        let _ = env_logger::try_init();
        retro_log_info("retro_init");

        // Acquire rumble interface
        let mut rumble = retro_rumble_interface {
            set_rumble_state: None,
        };
        if env_cmd(
            RETRO_ENVIRONMENT_GET_RUMBLE_INTERFACE,
            &mut rumble as *mut retro_rumble_interface as *mut c_void,
        ) && let Some(rumble_fn) = rumble.set_rumble_state
        {
            *lock(&CB_RUMBLE) = Some(rumble_fn);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_deinit() {
    retro_log_info("retro_deinit");
    *lock(&CORE) = None;
    lock(&SRAM_BUF).clear();
    *lock(&MAX_SERIALIZE_SIZE) = 0;
    *lock(&CB_LOG) = None;
    *lock(&CB_RUMBLE) = None;
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_api_version() -> c_uint {
    RETRO_API_VERSION
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn retro_get_system_info(info: *mut retro_system_info) {
    if info.is_null() {
        return;
    }
    unsafe {
        (*info).library_name = LIB_NAME.as_ptr();
        (*info).library_version = LIB_VERSION.as_ptr();
        (*info).valid_extensions = VALID_EXTENSIONS.as_ptr();
        (*info).need_fullpath = false;
        (*info).block_extract = false;
    }
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn retro_get_system_av_info(info: *mut retro_system_av_info) {
    if info.is_null() {
        return;
    }
    let (w, h, fps, sr) = {
        let core = lock(&CORE);
        if let Some(state) = core.as_ref() {
            (
                state.native_width(),
                state.native_height(),
                state.fps(),
                state.sample_rate as f64,
            )
        } else {
            (160, 144, 59.7275, 48000.0)
        }
    };

    retro_log_info(&format!(
        "retro_get_system_av_info: {w}x{h} @ {fps:.2} Hz, sr={sr}"
    ));

    unsafe {
        (*info).geometry = retro_game_geometry {
            base_width: w,
            base_height: h,
            max_width: 256,
            max_height: 240,
            aspect_ratio: 0.0,
        };
        (*info).timing = retro_system_timing {
            fps,
            sample_rate: sr,
        };
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_set_controller_port_device(port: c_uint, device: c_uint) {
    retro_log_info(&format!(
        "retro_set_controller_port_device: port={port}, device={device}"
    ));
    let mut core = lock(&CORE);
    if let Some(state) = core.as_mut()
        && (port as usize) < state.port_device.len()
    {
        state.port_device[port as usize] = device;
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_reset() {
    let _ = catch_unwind(|| {
        let mut core = lock(&CORE);
        if let Some(state) = core.as_mut() {
            state.reset();
            apply_core_options(state);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_run() {
    let result = catch_unwind(|| {
        let mut core = lock(&CORE);
        let Some(state) = core.as_mut() else {
            return;
        };

        {
            let mut counter = lock(&FRAME_COUNTER);
            *counter += 1;
        }

        if check_variables_updated() {
            apply_core_options(state);
        }

        if let Some(poll) = *lock(&CB_INPUT_POLL) {
            unsafe { poll() };
        }

        let (buttons, dpad) = poll_joypad_port(0);
        state.set_input(buttons, dpad);

        if state.is_nes() {
            let p2_device = state.port_device[1];
            if p2_device == RETRO_DEVICE_LIGHTGUN {
                let (trigger, hit) = poll_lightgun_port(1);
                state.set_zapper_state(trigger, hit);
            } else {
                let (b2, d2) = poll_joypad_port(1);
                state.set_input_p2(b2, d2);
            }
        }

        state.step_frame();

        let w = state.native_width();
        let h = state.native_height();
        let use_xrgb = USE_XRGB8888.load(Ordering::Relaxed);

        if use_xrgb {
            let xrgb = state.framebuffer_as_xrgb8888();
            let pitch = w as usize * 4;
            if let Some(cb) = *lock(&CB_VIDEO_REFRESH) {
                unsafe { cb(xrgb.as_ptr() as *const c_void, w, h, pitch) };
            }
        } else {
            let rgb565 = state.framebuffer_as_rgb565();
            let pitch = w as usize * 2;
            if let Some(cb) = *lock(&CB_VIDEO_REFRESH) {
                unsafe { cb(rgb565.as_ptr() as *const c_void, w, h, pitch) };
            }
        }

        state.drain_audio();
        let samples = &state.audio_buf;

        if !samples.is_empty() {
            let i16_buf: Vec<i16> = samples
                .iter()
                .map(|&s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
                .collect();
            let frames = i16_buf.len() / 2;

            if let Some(cb) = *lock(&CB_AUDIO_SAMPLE_BATCH) {
                unsafe { cb(i16_buf.as_ptr(), frames) };
            }
        }

        state.sync_sram_to_buf(&mut lock(&SRAM_BUF));

        state.refresh_system_ram();
        state.refresh_video_ram();
    });
    if let Err(e) = result {
        retro_log_error(&format!("retro_run PANIC: {e:?}"));
    }
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn retro_load_game(info: *const retro_game_info) -> bool {
    let result = catch_unwind(|| {
        if info.is_null() {
            retro_log_error("retro_load_game: info is null");
            return false;
        }

        let mut fmt = RETRO_PIXEL_FORMAT_XRGB8888;
        let accepted = env_cmd(
            RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
            &mut fmt as *mut c_uint as *mut c_void,
        );
        if !accepted {
            fmt = RETRO_PIXEL_FORMAT_RGB565;
            env_cmd(
                RETRO_ENVIRONMENT_SET_PIXEL_FORMAT,
                &mut fmt as *mut c_uint as *mut c_void,
            );
        }
        USE_XRGB8888.store(accepted, Ordering::Relaxed);
        retro_log_info(&format!(
            "retro_load_game: pixel format = {}",
            if accepted { "XRGB8888" } else { "RGB565" }
        ));

        let (data, path_str) = unsafe {
            let gi = &*info;
            if gi.data.is_null() || gi.size == 0 {
                retro_log_error("retro_load_game: data is null or size is 0");
                return false;
            }
            let data = std::slice::from_raw_parts(gi.data as *const u8, gi.size);
            let path = if gi.path.is_null() {
                "rom.gb"
            } else {
                CStr::from_ptr(gi.path).to_str().unwrap_or("rom.gb")
            };
            (data, path)
        };

        retro_log_info(&format!(
            "retro_load_game: path='{}' size={}",
            path_str,
            data.len()
        ));

        match core::CoreState::from_rom(data, path_str) {
            Ok(mut state) => {
                let sram_size = state.sram_size();
                let is_nes = state.is_nes();
                retro_log_info(&format!(
                    "retro_load_game OK: {}x{} @ {:.2} Hz, sample_rate={}, sram_size={}, system={}",
                    state.native_width(),
                    state.native_height(),
                    state.fps(),
                    state.sample_rate,
                    sram_size,
                    if is_nes { "NES" } else { "GB/GBC" },
                ));

                apply_core_options(&mut state);

                {
                    let mut sram_buf = lock(&SRAM_BUF);
                    sram_buf.clear();
                    if let Some(sram) = state.battery_sram() {
                        *sram_buf = sram;
                    }
                }
                *lock(&CORE) = Some(state);
                *lock(&FRAME_COUNTER) = 0;
                *lock(&MAX_SERIALIZE_SIZE) = 0;

                set_input_descriptors(is_nes);

                true
            }
            Err(e) => {
                retro_log_error(&format!("retro_load_game FAILED: {e:#}"));
                false
            }
        }
    });
    result.unwrap_or_else(|e| {
        retro_log_error(&format!("retro_load_game PANIC: {e:?}"));
        false
    })
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_load_game_special(
    _game_type: c_uint,
    _info: *const retro_game_info,
    _num_info: usize,
) -> bool {
    false
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_unload_game() {
    retro_log_info("retro_unload_game");
    let mut core = lock(&CORE);
    if let Some(state) = core.as_ref() {
        state.sync_sram_to_buf(&mut lock(&SRAM_BUF));
    }
    *core = None;
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_region() -> c_uint {
    RETRO_REGION_NTSC
}

const SERIALIZE_LENGTH_PREFIX: usize = 4;

#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize_size() -> usize {
    catch_unwind(|| {
        let core = lock(&CORE);
        let actual = core
            .as_ref()
            .and_then(|s| s.encode_state().ok())
            .map_or(0, |v| v.len());
        let mut max = lock(&MAX_SERIALIZE_SIZE);
        if actual > *max {
            *max = actual;
        }
        SERIALIZE_LENGTH_PREFIX + *max + 4096
    })
    .unwrap_or(0)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn retro_serialize(data: *mut c_void, size: usize) -> bool {
    if data.is_null() {
        return false;
    }
    catch_unwind(|| {
        let core = lock(&CORE);
        let Some(state) = core.as_ref() else {
            return false;
        };
        match state.encode_state() {
            Ok(bytes) if SERIALIZE_LENGTH_PREFIX + bytes.len() <= size => {
                unsafe {
                    std::ptr::write_bytes(data as *mut u8, 0, size);
                    let len_bytes = (bytes.len() as u32).to_le_bytes();
                    std::ptr::copy_nonoverlapping(
                        len_bytes.as_ptr(),
                        data as *mut u8,
                        SERIALIZE_LENGTH_PREFIX,
                    );
                    std::ptr::copy_nonoverlapping(
                        bytes.as_ptr(),
                        (data as *mut u8).add(SERIALIZE_LENGTH_PREFIX),
                        bytes.len(),
                    );
                }
                true
            }
            _ => false,
        }
    })
    .unwrap_or(false)
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn retro_unserialize(data: *const c_void, size: usize) -> bool {
    if data.is_null() || size < SERIALIZE_LENGTH_PREFIX {
        return false;
    }
    catch_unwind(|| {
        let mut core = lock(&CORE);
        let Some(state) = core.as_mut() else {
            return false;
        };
        let all_bytes = unsafe { std::slice::from_raw_parts(data as *const u8, size) };
        let Ok(len_bytes) = all_bytes[..4].try_into() else {
            return false;
        };
        let payload_len = u32::from_le_bytes(len_bytes) as usize;
        if payload_len == 0 || SERIALIZE_LENGTH_PREFIX + payload_len > size {
            return false;
        }
        let payload = &all_bytes[SERIALIZE_LENGTH_PREFIX..SERIALIZE_LENGTH_PREFIX + payload_len];
        state.load_state(payload).is_ok()
    })
    .unwrap_or(false)
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_data(id: c_uint) -> *mut c_void {
    // SAFETY: The returned pointer points into a Vec that lives in a static Mutex.
    // The libretro contract guarantees single-threaded access; the frontend will not
    // use this pointer concurrently with calls that reallocate the underlying buffer.
    match id {
        RETRO_MEMORY_SAVE_RAM => {
            let mut buf = lock(&SRAM_BUF);
            if !buf.is_empty() {
                buf.as_mut_ptr() as *mut c_void
            } else {
                std::ptr::null_mut()
            }
        }
        RETRO_MEMORY_SYSTEM_RAM => {
            let mut core = lock(&CORE);
            if let Some(state) = core.as_mut()
                && !state.system_ram_buf.is_empty()
            {
                return state.system_ram_buf.as_mut_ptr() as *mut c_void;
            }
            std::ptr::null_mut()
        }
        RETRO_MEMORY_VIDEO_RAM => {
            let mut core = lock(&CORE);
            if let Some(state) = core.as_mut()
                && !state.video_ram_buf.is_empty()
            {
                return state.video_ram_buf.as_mut_ptr() as *mut c_void;
            }
            std::ptr::null_mut()
        }
        _ => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_memory_size(id: c_uint) -> usize {
    match id {
        RETRO_MEMORY_SAVE_RAM => lock(&SRAM_BUF).len(),
        RETRO_MEMORY_SYSTEM_RAM => lock(&CORE).as_ref().map_or(0, |s| s.system_ram_size()),
        RETRO_MEMORY_VIDEO_RAM => lock(&CORE).as_ref().map_or(0, |s| s.video_ram_size()),
        _ => 0,
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn retro_cheat_reset() {
    let _ = catch_unwind(|| {
        let mut core = lock(&CORE);
        if let Some(state) = core.as_mut() {
            state.cheat_reset();
            retro_log_info("retro_cheat_reset");
        }
    });
}

#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[unsafe(no_mangle)]
pub extern "C" fn retro_cheat_set(_index: c_uint, enabled: bool, code: *const c_char) {
    let _ = catch_unwind(|| {
        if !enabled || code.is_null() {
            return;
        }
        let code_str = unsafe { CStr::from_ptr(code) };
        let Ok(code_str) = code_str.to_str() else {
            return;
        };
        let mut core = lock(&CORE);
        if let Some(state) = core.as_mut() {
            retro_log_info(&format!("retro_cheat_set: '{code_str}'"));
            state.cheat_set(code_str);
        }
    });
}
