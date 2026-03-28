mod enums;
mod keybindings;
mod serde_helpers;

pub(crate) use enums::{
    build_gpu_params, default_color_correction_matrix, default_offscreen_scale,
    AudioRecordingFormat, ColorCorrection, EffectPreset, LeftStickMode, ScalingMode, ShaderParams,
    ShaderPreset, TiltInputMode, VsyncMode,
};
pub(crate) use keybindings::{
    BindingAction, GamepadAction, GamepadBindings, InputBindingAction, KeyBindings, ShortcutAction,
    ShortcutBindings, TiltBindingAction, TiltKeyBindings,
};
pub(crate) use serde_helpers::keycode_from_string;

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

use zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference;

const MAX_RECENT_ROMS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct RecentRomEntry {
    pub(crate) path: String,
    pub(crate) name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) hardware_mode_preference: HardwareModePreference,
    pub(crate) fast_forward_multiplier: usize,
    pub(crate) uncapped_frames_per_tick: usize,
    pub(crate) uncapped_speed: bool,
    pub(crate) show_fps: bool,
    pub(crate) key_bindings: KeyBindings,
    pub(crate) tilt_key_bindings: TiltKeyBindings,
    pub(crate) left_stick_mode: LeftStickMode,
    pub(crate) tilt_input_mode: TiltInputMode,
    pub(crate) tilt_sensitivity: f32,
    pub(crate) tilt_lerp: f32,
    pub(crate) tilt_deadzone: f32,
    pub(crate) tilt_invert_x: bool,
    pub(crate) tilt_invert_y: bool,
    pub(crate) stick_tilt_bypass_lerp: bool,
    pub(crate) master_volume: f32,
    #[serde(skip)]
    pub(crate) pre_mute_volume: Option<f32>,
    pub(crate) mute_audio_during_fast_forward: bool,
    #[serde(default)]
    pub(crate) audio_recording_format: AudioRecordingFormat,
    pub(crate) frame_skip: bool,
    pub(crate) enable_memory_editing: bool,
    pub(crate) auto_save_state: bool,
    pub(crate) recent_roms: Vec<RecentRomEntry>,
    pub(crate) speedup_key: String,
    pub(crate) rewind_enabled: bool,
    pub(crate) rewind_key: String,
    #[serde(default = "default_rewind_speed")]
    pub(crate) rewind_speed: usize,
    #[serde(default = "default_rewind_seconds")]
    pub(crate) rewind_seconds: usize,
    pub(crate) shader_preset: ShaderPreset,
    #[serde(default)]
    pub(crate) scaling_mode: ScalingMode,
    #[serde(default)]
    pub(crate) effect_preset: EffectPreset,
    #[serde(default = "default_offscreen_scale")]
    pub(crate) offscreen_scale: u32,
    #[serde(default)]
    pub(crate) shader_params: ShaderParams,
    #[serde(default)]
    pub(crate) custom_shader_path: String,
    #[serde(default)]
    pub(crate) color_correction: ColorCorrection,
    #[serde(default = "default_color_correction_matrix")]
    pub(crate) color_correction_matrix: [f32; 9],
    #[serde(default)]
    pub(crate) vsync_mode: VsyncMode,
    #[serde(default)]
    pub(crate) autohide_menu_bar: bool,
    #[serde(default = "default_ui_scale")]
    pub(crate) ui_scale: f32,
    #[serde(skip)]
    pub(crate) ui_scale_needs_auto: bool,
    #[serde(default)]
    pub(crate) shortcut_bindings: ShortcutBindings,
    #[serde(default)]
    pub(crate) gamepad_bindings: GamepadBindings,
    #[serde(default)]
    pub(crate) camera_device_index: u32,
    #[serde(default)]
    pub(crate) camera_auto_levels: bool,
    #[serde(default = "default_camera_gamma")]
    pub(crate) camera_gamma: f32,
    #[serde(default = "default_camera_brightness")]
    pub(crate) camera_brightness: f32,
    #[serde(default = "default_camera_contrast")]
    pub(crate) camera_contrast: f32,
    pub(crate) open_debug_tabs: Vec<String>,
}

fn default_rewind_speed() -> usize {
    3
}
fn default_rewind_seconds() -> usize {
    10
}
fn default_ui_scale() -> f32 {
    1.0
}

fn default_camera_gamma() -> f32 {
    1.05
}

fn default_camera_contrast() -> f32 {
    1.65
}

fn default_camera_brightness() -> f32 {
    0.15
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hardware_mode_preference: HardwareModePreference::Auto,
            fast_forward_multiplier: 4,
            uncapped_frames_per_tick: 60,
            uncapped_speed: false,
            show_fps: true,
            key_bindings: KeyBindings::default(),
            tilt_key_bindings: TiltKeyBindings::default(),
            left_stick_mode: LeftStickMode::Auto,
            tilt_input_mode: TiltInputMode::default(),
            tilt_sensitivity: 1.0,
            tilt_lerp: 0.25,
            tilt_deadzone: 0.12,
            tilt_invert_x: false,
            tilt_invert_y: false,
            stick_tilt_bypass_lerp: true,
            master_volume: 1.0,
            pre_mute_volume: None,
            mute_audio_during_fast_forward: false,
            audio_recording_format: AudioRecordingFormat::default(),
            frame_skip: false,
            enable_memory_editing: false,
            auto_save_state: false,
            recent_roms: Vec::new(),
            speedup_key: "Space".to_string(),
            rewind_enabled: true,
            rewind_key: "KeyR".to_string(),
            rewind_speed: default_rewind_speed(),
            rewind_seconds: default_rewind_seconds(),
            shader_preset: ShaderPreset::None,
            scaling_mode: ScalingMode::PixelPerfect,
            effect_preset: EffectPreset::None,
            offscreen_scale: default_offscreen_scale(),
            shader_params: ShaderParams::default(),
            custom_shader_path: String::new(),
            color_correction: ColorCorrection::None,
            color_correction_matrix: default_color_correction_matrix(),
            vsync_mode: VsyncMode::default(),
            autohide_menu_bar: false,
            ui_scale: default_ui_scale(),
            ui_scale_needs_auto: false,
            shortcut_bindings: ShortcutBindings::default(),
            gamepad_bindings: GamepadBindings::default(),
            camera_device_index: 0,
            camera_auto_levels: false,
            camera_gamma: default_camera_gamma(),
            camera_brightness: default_camera_brightness(),
            camera_contrast: default_camera_contrast(),
            open_debug_tabs: Vec::new(),
        }
    }
}

impl Settings {
    pub(crate) fn speedup_key_code(&self) -> KeyCode {
        keycode_from_string(&self.speedup_key).unwrap_or(KeyCode::Backquote)
    }

    pub(crate) fn rewind_key_code(&self) -> KeyCode {
        keycode_from_string(&self.rewind_key).unwrap_or(KeyCode::KeyR)
    }

    pub(crate) fn rewind_capture_interval(&self) -> usize {
        4
    }

    pub(crate) fn add_recent_rom(&mut self, path: &Path) {
        let path_str = path.to_string_lossy().to_string();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        self.recent_roms.retain(|r| r.path != path_str);

        self.recent_roms.insert(
            0,
            RecentRomEntry {
                path: path_str,
                name,
            },
        );

        self.recent_roms.truncate(MAX_RECENT_ROMS);
    }

    fn legacy_path() -> PathBuf {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("settings.json")
    }

    fn config_path() -> Option<PathBuf> {
        dirs::config_dir().map(|base| base.join("zeff-boy").join("settings.json"))
    }

    fn active_path() -> PathBuf {
        Self::config_path().unwrap_or_else(Self::legacy_path)
    }

    pub(crate) fn settings_dir() -> PathBuf {
        Self::active_path()
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    fn load_from_path(path: &Path) -> Option<Self> {
        let bytes = fs::read(path).ok()?;
        serde_json::from_slice::<Self>(&bytes).ok()
    }

    fn save_to_path(&self, path: &Path) {
        let Ok(serialized) = serde_json::to_vec_pretty(self) else {
            log::error!("failed to serialize settings");
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(e) = fs::create_dir_all(parent)
        {
            log::error!("failed to create settings directory {}: {e}", parent.display());
            return;
        }
        if let Err(e) = fs::write(path, serialized) {
            log::error!("failed to write settings to {}: {e}", path.display());
        }
    }

    pub(crate) fn load_or_default() -> Self {
        let mut settings = if let Some(config_path) = Self::config_path() {
            if let Some(settings) = Self::load_from_path(&config_path) {
                settings
            } else {
                let legacy_path = Self::legacy_path();
                if let Some(settings) = Self::load_from_path(&legacy_path) {
                    settings.save_to_path(&config_path);
                    settings
                } else {
                    Settings { ui_scale_needs_auto: true, ..Default::default() }
                }
            }
        } else {
            Self::load_from_path(&Self::legacy_path()).unwrap_or_else(|| {
                Settings { ui_scale_needs_auto: true, ..Default::default() }
            })
        };

        settings.migrate_shader_preset();
        settings
    }

    pub(crate) fn auto_detect_ui_scale(&mut self, monitor_height: u32, os_scale_factor: f64) {
        if !self.ui_scale_needs_auto {
            return;
        }
        self.ui_scale_needs_auto = false;

        if os_scale_factor > 1.1 {
            self.ui_scale = 1.0;
            return;
        }

        self.ui_scale = match monitor_height {
            0..=900 => 1.0,
            901..=1600 => 1.0,
            _ => 1.25,
        };
    }

    fn migrate_shader_preset(&mut self) {
        if self.shader_preset != ShaderPreset::None
            && self.scaling_mode == ScalingMode::PixelPerfect
            && self.effect_preset == EffectPreset::None
        {
            let (scaling, effect) = self.shader_preset.to_scaling_and_effect();
            self.scaling_mode = scaling;
            self.effect_preset = effect;
        }
    }

    pub(crate) fn save(&self) {
        self.save_to_path(&Self::active_path());
    }
}

#[cfg(test)]
mod tests;
