// Libretro core — C ABI callbacks receiving raw pointers from the frontend.

mod api;
mod callbacks;
mod core;
mod game;
mod input;
mod memory;
mod options;
mod serialization;

use api::*;
use callbacks::*;
use std::ffi::CStr;
use std::os::raw::{c_char, c_uint, c_void};
use std::panic::catch_unwind;
use std::sync::atomic::Ordering;

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

        options::set_core_options();

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
            options::apply_core_options(state);
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

        if options::check_variables_updated() {
            options::apply_core_options(state);
        }

        if let Some(poll) = *lock(&CB_INPUT_POLL) {
            unsafe { poll() };
        }

        let (buttons, dpad) = input::poll_joypad_port(0);
        state.set_input(buttons, dpad);

        if state.is_nes() {
            let p2_device = state.port_device[1];
            if p2_device == RETRO_DEVICE_LIGHTGUN {
                let (trigger, hit) = input::poll_lightgun_port(1);
                state.set_zapper_state(trigger, hit);
            } else {
                let (b2, d2) = input::poll_joypad_port(1);
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

#[unsafe(no_mangle)]
pub extern "C" fn retro_get_region() -> c_uint {
    RETRO_REGION_NTSC
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
