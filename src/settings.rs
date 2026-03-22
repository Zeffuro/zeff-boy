use std::fs;
use std::path::{Path, PathBuf};

use serde::de::Deserializer;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use winit::keyboard::KeyCode;

use crate::hardware::types::hardware_mode::HardwareModePreference;

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

fn keycode_from_string(name: &str) -> Option<KeyCode> {
    match name {
        "ArrowUp" => Some(KeyCode::ArrowUp),
        "ArrowDown" => Some(KeyCode::ArrowDown),
        "ArrowLeft" => Some(KeyCode::ArrowLeft),
        "ArrowRight" => Some(KeyCode::ArrowRight),
        "Enter" => Some(KeyCode::Enter),
        "ShiftLeft" => Some(KeyCode::ShiftLeft),
        "ShiftRight" => Some(KeyCode::ShiftRight),
        "ControlLeft" => Some(KeyCode::ControlLeft),
        "ControlRight" => Some(KeyCode::ControlRight),
        "AltLeft" => Some(KeyCode::AltLeft),
        "AltRight" => Some(KeyCode::AltRight),
        "Space" => Some(KeyCode::Space),
        "Backspace" => Some(KeyCode::Backspace),
        "Escape" => Some(KeyCode::Escape),
        "Tab" => Some(KeyCode::Tab),
        "CapsLock" => Some(KeyCode::CapsLock),
        "Minus" => Some(KeyCode::Minus),
        "Equal" => Some(KeyCode::Equal),
        "BracketLeft" => Some(KeyCode::BracketLeft),
        "BracketRight" => Some(KeyCode::BracketRight),
        "Backslash" => Some(KeyCode::Backslash),
        "Semicolon" => Some(KeyCode::Semicolon),
        "Quote" => Some(KeyCode::Quote),
        "Comma" => Some(KeyCode::Comma),
        "Period" => Some(KeyCode::Period),
        "Slash" => Some(KeyCode::Slash),
        "Backquote" => Some(KeyCode::Backquote),
        _ if name.len() == 4 && name.starts_with("Key") => {
            match name.chars().nth(3).unwrap_or_default() {
                'A' => Some(KeyCode::KeyA),
                'B' => Some(KeyCode::KeyB),
                'C' => Some(KeyCode::KeyC),
                'D' => Some(KeyCode::KeyD),
                'E' => Some(KeyCode::KeyE),
                'F' => Some(KeyCode::KeyF),
                'G' => Some(KeyCode::KeyG),
                'H' => Some(KeyCode::KeyH),
                'I' => Some(KeyCode::KeyI),
                'J' => Some(KeyCode::KeyJ),
                'K' => Some(KeyCode::KeyK),
                'L' => Some(KeyCode::KeyL),
                'M' => Some(KeyCode::KeyM),
                'N' => Some(KeyCode::KeyN),
                'O' => Some(KeyCode::KeyO),
                'P' => Some(KeyCode::KeyP),
                'Q' => Some(KeyCode::KeyQ),
                'R' => Some(KeyCode::KeyR),
                'S' => Some(KeyCode::KeyS),
                'T' => Some(KeyCode::KeyT),
                'U' => Some(KeyCode::KeyU),
                'V' => Some(KeyCode::KeyV),
                'W' => Some(KeyCode::KeyW),
                'X' => Some(KeyCode::KeyX),
                'Y' => Some(KeyCode::KeyY),
                'Z' => Some(KeyCode::KeyZ),
                _ => None,
            }
        }
        _ if name.len() == 6 && name.starts_with("Digit") => {
            match name.chars().nth(5).unwrap_or_default() {
                '0' => Some(KeyCode::Digit0),
                '1' => Some(KeyCode::Digit1),
                '2' => Some(KeyCode::Digit2),
                '3' => Some(KeyCode::Digit3),
                '4' => Some(KeyCode::Digit4),
                '5' => Some(KeyCode::Digit5),
                '6' => Some(KeyCode::Digit6),
                '7' => Some(KeyCode::Digit7),
                '8' => Some(KeyCode::Digit8),
                '9' => Some(KeyCode::Digit9),
                _ => None,
            }
        }
        _ => match name {
            "F1" => Some(KeyCode::F1),
            "F2" => Some(KeyCode::F2),
            "F3" => Some(KeyCode::F3),
            "F4" => Some(KeyCode::F4),
            "F5" => Some(KeyCode::F5),
            "F6" => Some(KeyCode::F6),
            "F7" => Some(KeyCode::F7),
            "F8" => Some(KeyCode::F8),
            "F9" => Some(KeyCode::F9),
            "F10" => Some(KeyCode::F10),
            "F11" => Some(KeyCode::F11),
            "F12" => Some(KeyCode::F12),
            _ => None,
        },
    }
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
    pub(crate) mute_audio_during_fast_forward: bool,
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
            mute_audio_during_fast_forward: false,
        }
    }
}

impl Settings {
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
