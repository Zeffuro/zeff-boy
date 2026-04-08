use crate::api::*;
use crate::callbacks::*;
use std::ffi::CStr;
use std::os::raw::{c_uint, c_void};
use std::panic::catch_unwind;
use std::sync::atomic::Ordering;

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

        match crate::core::CoreState::from_rom(data, path_str) {
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

                crate::options::apply_core_options(&mut state);

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

                crate::input::set_input_descriptors(is_nes);

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
