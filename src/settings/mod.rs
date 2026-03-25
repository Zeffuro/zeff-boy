mod enums;
mod keybindings;
mod serde_helpers;

pub(crate) use enums::*;
pub(crate) use keybindings::*;
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
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, serialized);
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
                    let mut s = Self::default();
                    s.ui_scale_needs_auto = true;
                    s
                }
            }
        } else {
            Self::load_from_path(&Self::legacy_path()).unwrap_or_else(|| {
                let mut s = Self::default();
                s.ui_scale_needs_auto = true;
                s
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
mod tests {
    use super::*;

    #[test]
    fn settings_default_roundtrip() {
        let defaults = Settings::default();
        let json = serde_json::to_string_pretty(&defaults).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(defaults, restored);
    }

    #[test]
    fn settings_with_modified_values_roundtrip() {
        let mut s = Settings::default();
        s.fast_forward_multiplier = 8;
        s.master_volume = 0.5;
        s.rewind_speed = 5;
        s.rewind_seconds = 30;
        s.rewind_enabled = false;
        s.shader_preset = ShaderPreset::CRT;
        s.custom_shader_path = "C:/shaders/custom.wgsl".to_string();
        s.autohide_menu_bar = true;
        s.frame_skip = true;

        let json = serde_json::to_string(&s).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, restored);
    }

    #[test]
    fn settings_backward_compat_missing_fields_use_defaults() {
        let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.rewind_speed, default_rewind_speed());
        assert_eq!(s.rewind_seconds, default_rewind_seconds());
        assert_eq!(s.shader_preset, ShaderPreset::None);
        assert!(!s.autohide_menu_bar);
    }

    #[test]
    fn key_bindings_serde_roundtrip() {
        let mut bindings = KeyBindings::default();
        bindings.a = KeyCode::KeyQ;
        bindings.b = KeyCode::KeyE;

        let json = serde_json::to_string(&bindings).unwrap();
        let restored: KeyBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(bindings, restored);
    }

    #[test]
    fn key_bindings_deserialize_unknown_falls_back_to_defaults() {
        let json = r#"{"up":"ArrowUp","down":"ArrowDown","left":"UNKNOWN_KEY","right":"ArrowRight","a":"KeyZ","b":"KeyX","start":"Enter","select":"ShiftRight"}"#;
        let bindings: KeyBindings = serde_json::from_str(json).unwrap();
        assert_eq!(bindings.left, KeyCode::ArrowLeft);
        assert_eq!(bindings.up, KeyCode::ArrowUp);
    }

    #[test]
    fn shortcut_bindings_get_returns_default_for_unknown_string() {
        let mut bindings = ShortcutBindings::default();
        bindings.fullscreen = "NONSENSE".to_string();

        assert_eq!(bindings.get(ShortcutAction::Fullscreen), KeyCode::F11);
    }

    #[test]
    fn shortcut_bindings_set_and_get() {
        let mut bindings = ShortcutBindings::default();
        bindings.set(ShortcutAction::Pause, KeyCode::KeyP);
        assert_eq!(bindings.get(ShortcutAction::Pause), KeyCode::KeyP);
    }

    #[test]
    fn gamepad_bindings_roundtrip() {
        let mut gb = GamepadBindings::default();
        gb.set(BindingAction::A, "West");
        let json = serde_json::to_string(&gb).unwrap();
        let restored: GamepadBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.get(BindingAction::A), "West");
        assert_eq!(restored.get(BindingAction::B), "East");
    }

    #[test]
    fn tilt_key_bindings_serde_roundtrip() {
        let mut bindings = TiltKeyBindings::default();
        bindings.up = KeyCode::KeyI;
        let json = serde_json::to_string(&bindings).unwrap();
        let restored: TiltKeyBindings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.up, KeyCode::KeyI);
        assert_eq!(restored.down, KeyCode::KeyS);
    }

    #[test]
    fn recent_roms_add_and_dedup() {
        let mut s = Settings::default();
        s.add_recent_rom(Path::new("game1.gb"));
        s.add_recent_rom(Path::new("game2.gb"));
        s.add_recent_rom(Path::new("game1.gb"));
        assert_eq!(s.recent_roms.len(), 2);
        assert_eq!(s.recent_roms[0].name, "game1.gb");
        assert_eq!(s.recent_roms[1].name, "game2.gb");
    }

    #[test]
    fn recent_roms_truncates_at_max() {
        let mut s = Settings::default();
        for i in 0..15 {
            s.add_recent_rom(Path::new(&format!("game{i}.gb")));
        }
        assert_eq!(s.recent_roms.len(), MAX_RECENT_ROMS);
    }

    #[test]
    fn default_rewind_speed_is_3() {
        assert_eq!(Settings::default().rewind_speed, 3);
    }

    #[test]
    fn pre_mute_volume_is_skipped_in_serde() {
        let mut s = Settings::default();
        s.pre_mute_volume = Some(0.75);
        let json = serde_json::to_string(&s).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.pre_mute_volume, None);
    }

    #[test]
    fn shader_params_roundtrip() {
        let params = ShaderParams {
            scanline_intensity: 0.5,
            crt_curvature: 0.8,
            grid_intensity: 0.1,
            upscale_edge_strength: 0.75,
            palette_mix: 0.9,
            palette_warmth: 0.2,
        };
        let json = serde_json::to_string(&params).unwrap();
        let restored: ShaderParams = serde_json::from_str(&json).unwrap();
        assert_eq!(params, restored);
    }

    #[test]
    fn shader_params_to_gpu_bytes() {
        let params = ShaderParams::default();
        let bytes = params.to_gpu_bytes();
        let scanline = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let curvature = f32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let edge = f32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        let mix = f32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        assert!((scanline - params.scanline_intensity).abs() < f32::EPSILON);
        assert!((curvature - params.crt_curvature).abs() < f32::EPSILON);
        assert!((edge - params.upscale_edge_strength).abs() < f32::EPSILON);
        assert!((mix - params.palette_mix).abs() < f32::EPSILON);
    }

    #[test]
    fn build_gpu_params_includes_color_correction() {
        let params = ShaderParams::default();
        let buf = build_gpu_params(&params, ColorCorrection::GbcLcd, default_color_correction_matrix());
        let mode = u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]);
        assert_eq!(mode, 1);
        let r00 = f32::from_le_bytes([buf[48], buf[49], buf[50], buf[51]]);
        assert!((r00 - 26.0 / 32.0).abs() < f32::EPSILON);
    }

    #[test]
    fn build_gpu_params_none_mode_is_identity() {
        let params = ShaderParams::default();
        let buf = build_gpu_params(&params, ColorCorrection::None, default_color_correction_matrix());
        let mode = u32::from_le_bytes([buf[32], buf[33], buf[34], buf[35]]);
        assert_eq!(mode, 0);
        let r00 = f32::from_le_bytes([buf[48], buf[49], buf[50], buf[51]]);
        assert!((r00 - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn rewind_capture_interval_is_4() {
        let s = Settings::default();
        assert_eq!(s.rewind_capture_interval(), 4);
    }

    #[test]
    fn color_correction_serde_roundtrip() {
        let mut s = Settings::default();
        s.color_correction = ColorCorrection::GbcLcd;
        let json = serde_json::to_string(&s).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.color_correction, ColorCorrection::GbcLcd);
    }

    #[test]
    fn color_correction_defaults_to_none_when_missing() {
        let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.color_correction, ColorCorrection::None);
        assert_eq!(s.color_correction_matrix, default_color_correction_matrix());
    }

    #[test]
    fn custom_color_correction_matrix_roundtrip() {
        let mut s = Settings::default();
        s.color_correction = ColorCorrection::Custom;
        s.color_correction_matrix = [
            1.0, 0.2, 0.0,
            0.1, 0.9, 0.0,
            0.0, 0.3, 0.8,
        ];
        let json = serde_json::to_string(&s).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.color_correction, ColorCorrection::Custom);
        assert_eq!(restored.color_correction_matrix, s.color_correction_matrix);
    }

    #[test]
    fn vsync_mode_serde_roundtrip() {
        let mut s = Settings::default();
        s.vsync_mode = VsyncMode::Off;
        let json = serde_json::to_string(&s).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.vsync_mode, VsyncMode::Off);

        s.vsync_mode = VsyncMode::Adaptive;
        let json = serde_json::to_string(&s).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.vsync_mode, VsyncMode::Adaptive);
    }

    #[test]
    fn vsync_mode_defaults_to_on_when_missing() {
        let json = r#"{"hardware_mode_preference":"Auto","fast_forward_multiplier":4}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.vsync_mode, VsyncMode::On);
    }
}

