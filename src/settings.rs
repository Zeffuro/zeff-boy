use std::fs;
use std::path::{Path, PathBuf};

use serde::de::Deserializer;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use winit::keyboard::KeyCode;

use crate::hardware::types::hardware_mode::HardwareModePreference;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AudioRecordingFormat {
    Wav16,
    WavFloat,
}

impl AudioRecordingFormat {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Wav16 => "WAV 16-bit PCM",
            Self::WavFloat => "WAV 32-bit Float",
        }
    }

    pub(crate) fn extension(self) -> &'static str {
        "wav"
    }
}

impl Default for AudioRecordingFormat {
    fn default() -> Self {
        Self::Wav16
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingAction {
    Up,
    Down,
    Left,
    Right,
    A,
    B,
    Start,
    Select,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TiltBindingAction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InputBindingAction {
    Joypad(BindingAction),
    Tilt(TiltBindingAction),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ShaderPreset {
    None,
    CRT,
    Scanlines,
    LCDGrid,
}

impl Default for ShaderPreset {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub(crate) struct ShaderParams {
    #[serde(default = "default_scanline_intensity")]
    pub(crate) scanline_intensity: f32,
    #[serde(default = "default_crt_curvature")]
    pub(crate) crt_curvature: f32,
    #[serde(default = "default_grid_intensity")]
    pub(crate) grid_intensity: f32,
}

fn default_scanline_intensity() -> f32 { 0.18 }
fn default_crt_curvature() -> f32 { 0.3 }
fn default_grid_intensity() -> f32 { 0.3 }

impl Default for ShaderParams {
    fn default() -> Self {
        Self {
            scanline_intensity: default_scanline_intensity(),
            crt_curvature: default_crt_curvature(),
            grid_intensity: default_grid_intensity(),
        }
    }
}

impl ShaderParams {
    pub(crate) fn to_gpu_bytes(&self) -> [u8; 16] {
        let mut buf = [0u8; 16];
        buf[0..4].copy_from_slice(&self.scanline_intensity.to_le_bytes());
        buf[4..8].copy_from_slice(&self.crt_curvature.to_le_bytes());
        buf[8..12].copy_from_slice(&self.grid_intensity.to_le_bytes());
        buf
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LeftStickMode {
    Dpad,
    Tilt,
    Auto,
}

impl Default for LeftStickMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TiltInputMode {
    Keyboard,
    Mouse,
    Auto,
}

impl Default for TiltInputMode {
    fn default() -> Self {
        Self::Keyboard
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeyBindings {
    pub(crate) up: KeyCode,
    pub(crate) down: KeyCode,
    pub(crate) left: KeyCode,
    pub(crate) right: KeyCode,
    pub(crate) a: KeyCode,
    pub(crate) b: KeyCode,
    pub(crate) start: KeyCode,
    pub(crate) select: KeyCode,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            up: KeyCode::ArrowUp,
            down: KeyCode::ArrowDown,
            left: KeyCode::ArrowLeft,
            right: KeyCode::ArrowRight,
            a: KeyCode::KeyZ,
            b: KeyCode::KeyX,
            start: KeyCode::Enter,
            select: KeyCode::ShiftRight,
        }
    }
}

impl KeyBindings {
    pub(crate) fn get(&self, action: BindingAction) -> KeyCode {
        match action {
            BindingAction::Up => self.up,
            BindingAction::Down => self.down,
            BindingAction::Left => self.left,
            BindingAction::Right => self.right,
            BindingAction::A => self.a,
            BindingAction::B => self.b,
            BindingAction::Start => self.start,
            BindingAction::Select => self.select,
        }
    }

    pub(crate) fn set(&mut self, action: BindingAction, key: KeyCode) {
        match action {
            BindingAction::Up => self.up = key,
            BindingAction::Down => self.down = key,
            BindingAction::Left => self.left = key,
            BindingAction::Right => self.right = key,
            BindingAction::A => self.a = key,
            BindingAction::B => self.b = key,
            BindingAction::Start => self.start = key,
            BindingAction::Select => self.select = key,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TiltKeyBindings {
    pub(crate) up: KeyCode,
    pub(crate) down: KeyCode,
    pub(crate) left: KeyCode,
    pub(crate) right: KeyCode,
}

impl Default for TiltKeyBindings {
    fn default() -> Self {
        Self {
            up: KeyCode::KeyW,
            down: KeyCode::KeyS,
            left: KeyCode::KeyA,
            right: KeyCode::KeyD,
        }
    }
}

impl TiltKeyBindings {
    pub(crate) fn get(&self, action: TiltBindingAction) -> KeyCode {
        match action {
            TiltBindingAction::Up => self.up,
            TiltBindingAction::Down => self.down,
            TiltBindingAction::Left => self.left,
            TiltBindingAction::Right => self.right,
        }
    }

    pub(crate) fn set(&mut self, action: TiltBindingAction, key: KeyCode) {
        match action {
            TiltBindingAction::Up => self.up = key,
            TiltBindingAction::Down => self.down = key,
            TiltBindingAction::Left => self.left = key,
            TiltBindingAction::Right => self.right = key,
        }
    }

    pub(crate) fn set_wasd_defaults(&mut self) {
        *self = Self::default();
    }
}

impl Serialize for TiltKeyBindings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("TiltKeyBindings", 4)?;
        state.serialize_field("up", &keycode_to_string(self.up))?;
        state.serialize_field("down", &keycode_to_string(self.down))?;
        state.serialize_field("left", &keycode_to_string(self.left))?;
        state.serialize_field("right", &keycode_to_string(self.right))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for TiltKeyBindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawTiltKeyBindings {
            up: Option<String>,
            down: Option<String>,
            left: Option<String>,
            right: Option<String>,
        }

        let raw = RawTiltKeyBindings::deserialize(deserializer)?;
        let defaults = Self::default();
        Ok(Self {
            up: raw
                .up
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.up),
            down: raw
                .down
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.down),
            left: raw
                .left
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.left),
            right: raw
                .right
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.right),
        })
    }
}

impl Serialize for KeyBindings {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("KeyBindings", 8)?;
        state.serialize_field("up", &keycode_to_string(self.up))?;
        state.serialize_field("down", &keycode_to_string(self.down))?;
        state.serialize_field("left", &keycode_to_string(self.left))?;
        state.serialize_field("right", &keycode_to_string(self.right))?;
        state.serialize_field("a", &keycode_to_string(self.a))?;
        state.serialize_field("b", &keycode_to_string(self.b))?;
        state.serialize_field("start", &keycode_to_string(self.start))?;
        state.serialize_field("select", &keycode_to_string(self.select))?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for KeyBindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawKeyBindings {
            up: Option<String>,
            down: Option<String>,
            left: Option<String>,
            right: Option<String>,
            a: Option<String>,
            b: Option<String>,
            start: Option<String>,
            select: Option<String>,
        }

        let raw = RawKeyBindings::deserialize(deserializer)?;
        let defaults = Self::default();
        Ok(Self {
            up: raw
                .up
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.up),
            down: raw
                .down
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.down),
            left: raw
                .left
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.left),
            right: raw
                .right
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.right),
            a: raw
                .a
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.a),
            b: raw
                .b
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.b),
            start: raw
                .start
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.start),
            select: raw
                .select
                .as_deref()
                .and_then(keycode_from_string)
                .unwrap_or(defaults.select),
        })
    }
}

fn keycode_to_string(code: KeyCode) -> String {
    format!("{code:?}")
}

static KEYCODE_MAP: phf::Map<&'static str, KeyCode> = phf::phf_map! {
    "ArrowUp" => KeyCode::ArrowUp,
    "ArrowDown" => KeyCode::ArrowDown,
    "ArrowLeft" => KeyCode::ArrowLeft,
    "ArrowRight" => KeyCode::ArrowRight,
    "Enter" => KeyCode::Enter,
    "ShiftLeft" => KeyCode::ShiftLeft,
    "ShiftRight" => KeyCode::ShiftRight,
    "ControlLeft" => KeyCode::ControlLeft,
    "ControlRight" => KeyCode::ControlRight,
    "AltLeft" => KeyCode::AltLeft,
    "AltRight" => KeyCode::AltRight,
    "Space" => KeyCode::Space,
    "Backspace" => KeyCode::Backspace,
    "Escape" => KeyCode::Escape,
    "Tab" => KeyCode::Tab,
    "CapsLock" => KeyCode::CapsLock,
    "Minus" => KeyCode::Minus,
    "Equal" => KeyCode::Equal,
    "BracketLeft" => KeyCode::BracketLeft,
    "BracketRight" => KeyCode::BracketRight,
    "Backslash" => KeyCode::Backslash,
    "Semicolon" => KeyCode::Semicolon,
    "Quote" => KeyCode::Quote,
    "Comma" => KeyCode::Comma,
    "Period" => KeyCode::Period,
    "Slash" => KeyCode::Slash,
    "Backquote" => KeyCode::Backquote,
    "KeyA" => KeyCode::KeyA,
    "KeyB" => KeyCode::KeyB,
    "KeyC" => KeyCode::KeyC,
    "KeyD" => KeyCode::KeyD,
    "KeyE" => KeyCode::KeyE,
    "KeyF" => KeyCode::KeyF,
    "KeyG" => KeyCode::KeyG,
    "KeyH" => KeyCode::KeyH,
    "KeyI" => KeyCode::KeyI,
    "KeyJ" => KeyCode::KeyJ,
    "KeyK" => KeyCode::KeyK,
    "KeyL" => KeyCode::KeyL,
    "KeyM" => KeyCode::KeyM,
    "KeyN" => KeyCode::KeyN,
    "KeyO" => KeyCode::KeyO,
    "KeyP" => KeyCode::KeyP,
    "KeyQ" => KeyCode::KeyQ,
    "KeyR" => KeyCode::KeyR,
    "KeyS" => KeyCode::KeyS,
    "KeyT" => KeyCode::KeyT,
    "KeyU" => KeyCode::KeyU,
    "KeyV" => KeyCode::KeyV,
    "KeyW" => KeyCode::KeyW,
    "KeyX" => KeyCode::KeyX,
    "KeyY" => KeyCode::KeyY,
    "KeyZ" => KeyCode::KeyZ,
    "Digit0" => KeyCode::Digit0,
    "Digit1" => KeyCode::Digit1,
    "Digit2" => KeyCode::Digit2,
    "Digit3" => KeyCode::Digit3,
    "Digit4" => KeyCode::Digit4,
    "Digit5" => KeyCode::Digit5,
    "Digit6" => KeyCode::Digit6,
    "Digit7" => KeyCode::Digit7,
    "Digit8" => KeyCode::Digit8,
    "Digit9" => KeyCode::Digit9,
    "F1" => KeyCode::F1,
    "F2" => KeyCode::F2,
    "F3" => KeyCode::F3,
    "F4" => KeyCode::F4,
    "F5" => KeyCode::F5,
    "F6" => KeyCode::F6,
    "F7" => KeyCode::F7,
    "F8" => KeyCode::F8,
    "F9" => KeyCode::F9,
    "F10" => KeyCode::F10,
    "F11" => KeyCode::F11,
    "F12" => KeyCode::F12,
};

fn keycode_from_string(name: &str) -> Option<KeyCode> {
    KEYCODE_MAP.get(name).copied()
}

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
    pub(crate) shader_preset: ShaderPreset,
    #[serde(default)]
    pub(crate) shader_params: ShaderParams,
    #[serde(default)]
    pub(crate) autohide_menu_bar: bool,
    pub(crate) open_debug_tabs: Vec<String>,
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
            speedup_key: "Backquote".to_string(),
            rewind_enabled: true,
            rewind_key: "KeyR".to_string(),
            shader_preset: ShaderPreset::None,
            shader_params: ShaderParams::default(),
            autohide_menu_bar: false,
            open_debug_tabs: vec!["CpuDebug".to_string()],
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

    pub(crate) fn add_recent_rom(&mut self, path: &Path) {
        let path_str = path.to_string_lossy().to_string();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("Unknown")
            .to_string();

        // Remove existing entry for the same path
        self.recent_roms.retain(|r| r.path != path_str);

        // Insert at front
        self.recent_roms.insert(
            0,
            RecentRomEntry {
                path: path_str,
                name,
            },
        );

        // Cap size
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
        if let Some(config_path) = Self::config_path() {
            if let Some(settings) = Self::load_from_path(&config_path) {
                return settings;
            }

            let legacy_path = Self::legacy_path();
            if let Some(settings) = Self::load_from_path(&legacy_path) {
                // One-time migration from the historical CWD-based path.
                settings.save_to_path(&config_path);
                return settings;
            }

            return Self::default();
        }

        Self::load_from_path(&Self::legacy_path()).unwrap_or_else(Self::default)
    }

    pub(crate) fn save(&self) {
        self.save_to_path(&Self::active_path());
    }
}
