mod binding_actions;
mod enums;
mod gamepad;
mod keyboard_bindings;
mod keycode_serde;
mod shortcuts;
mod structs;
mod tilt_bindings;

pub(crate) use binding_actions::{BindingAction, InputBindingAction};
pub(crate) use enums::{
    AudioRecordingFormat, ColorCorrection, DmgPalettePreset, EffectPreset, LeftStickMode,
    NesPaletteMode, ScalingMode, TiltInputMode, UiThemePreset, VsyncMode, build_gpu_params,
    default_color_correction_matrix, default_offscreen_scale,
};
pub(crate) use gamepad::{GamepadAction, GamepadBindings};
pub(crate) use keyboard_bindings::KeyBindings;
pub(crate) use keycode_serde::keycode_from_string;
pub(crate) use shortcuts::{ShortcutAction, ShortcutBindings};
pub(crate) use structs::{
    AudioSettings, CameraSettings, EmulationSettings, RecentRomEntry, RewindSettings, TiltSettings,
    UiSettings, VideoSettings,
};
pub(crate) use tilt_bindings::TiltBindingAction;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

use crate::platform;

const MAX_RECENT_ROMS: usize = 10;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub(crate) struct Settings {
    #[serde(flatten)]
    pub(crate) emulation: EmulationSettings,
    #[serde(flatten)]
    pub(crate) ui: UiSettings,
    pub(crate) key_bindings: KeyBindings,
    #[serde(flatten)]
    pub(crate) tilt: TiltSettings,
    #[serde(flatten)]
    pub(crate) audio: AudioSettings,
    pub(crate) recent_roms: Vec<RecentRomEntry>,
    pub(crate) speedup_key: String,
    #[serde(flatten)]
    pub(crate) rewind: RewindSettings,
    #[serde(flatten)]
    pub(crate) video: VideoSettings,
    #[serde(default)]
    pub(crate) shortcut_bindings: ShortcutBindings,
    #[serde(default)]
    pub(crate) gamepad_bindings: GamepadBindings,
    #[serde(flatten)]
    pub(crate) camera: CameraSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            emulation: EmulationSettings::default(),
            ui: UiSettings::default(),
            key_bindings: KeyBindings::default(),
            tilt: TiltSettings::default(),
            audio: AudioSettings::default(),
            recent_roms: Vec::new(),
            speedup_key: "Space".to_string(),
            rewind: RewindSettings::default(),
            video: VideoSettings::default(),
            shortcut_bindings: ShortcutBindings::default(),
            gamepad_bindings: GamepadBindings::default(),
            camera: CameraSettings::default(),
        }
    }
}

impl Settings {
    pub(crate) fn speedup_key_code(&self) -> KeyCode {
        keycode_from_string(&self.speedup_key).unwrap_or(KeyCode::Backquote)
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

    pub(crate) fn settings_dir() -> PathBuf {
        platform::settings_dir()
    }

    pub(crate) fn load_or_default() -> Self {
        if let Some(json) = platform::load_settings_json()
            && let Ok(mut settings) = serde_json::from_str::<Self>(&json)
        {
            settings.video.migrate_shader_preset();
            return settings;
        }
        Settings {
            ui: UiSettings {
                ui_scale_needs_auto: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub(crate) fn auto_detect_ui_scale(&mut self, monitor_height: u32, os_scale_factor: f64) {
        self.ui
            .auto_detect_ui_scale(monitor_height, os_scale_factor);
    }

    pub(crate) fn save(&self) {
        let Ok(json) = serde_json::to_string_pretty(self) else {
            log::error!("failed to serialize settings");
            return;
        };
        platform::save_settings_json(&json);
    }
}

#[cfg(test)]
mod tests;
