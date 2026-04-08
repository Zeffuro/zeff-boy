use crate::api::*;
use crate::callbacks::{env_cmd, retro_log_info};
use std::ffi::CStr;
use std::os::raw::c_void;

const OPT_DMG_PALETTE: &CStr = c"zeff_dmg_palette";
const OPT_NES_PALETTE: &CStr = c"zeff_nes_palette_mode";
const OPT_SGB_BORDER: &CStr = c"zeff_sgb_border";

pub(crate) fn set_core_options() {
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

pub(crate) fn get_variable(key: &CStr) -> Option<String> {
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

pub(crate) fn check_variables_updated() -> bool {
    let mut updated: bool = false;
    env_cmd(
        RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE,
        &mut updated as *mut bool as *mut c_void,
    );
    updated
}

pub(crate) fn apply_core_options(state: &mut crate::core::CoreState) {
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
