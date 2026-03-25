use serde::de::Deserializer;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use winit::keyboard::KeyCode;

use super::serde_helpers::{keycode_from_string, keycode_to_string};

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
pub(crate) enum GamepadAction {
    SpeedUp,
    Rewind,
    Pause,
    Turbo,
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
            a: KeyCode::KeyX,
            b: KeyCode::KeyZ,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ShortcutAction {
    Pause,
    Fullscreen,
    UncappedSpeed,
    MuteToggle,
    Screenshot,
    ResetGame,
    FrameAdvance,
    QuickSave,
    QuickLoad,
    SlotNext,
    SlotPrev,
    DebugContinue,
    DebugStep,
}

impl ShortcutAction {
    pub(crate) const ALL: &'static [ShortcutAction] = &[
        Self::Pause,
        Self::Fullscreen,
        Self::UncappedSpeed,
        Self::MuteToggle,
        Self::Screenshot,
        Self::ResetGame,
        Self::FrameAdvance,
        Self::QuickSave,
        Self::QuickLoad,
        Self::SlotNext,
        Self::SlotPrev,
        Self::DebugContinue,
        Self::DebugStep,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pause => "Pause / Resume",
            Self::Fullscreen => "Fullscreen",
            Self::UncappedSpeed => "Toggle uncapped",
            Self::MuteToggle => "Mute toggle",
            Self::Screenshot => "Screenshot",
            Self::ResetGame => "Reset game",
            Self::FrameAdvance => "Frame advance",
            Self::QuickSave => "Quick save",
            Self::QuickLoad => "Quick load",
            Self::SlotNext => "Next save slot",
            Self::SlotPrev => "Prev save slot",
            Self::DebugContinue => "Run (debug)",
            Self::DebugStep => "Step (debug)",
        }
    }

    fn default_keycode(self) -> KeyCode {
        match self {
            Self::Pause => KeyCode::KeyP,
            Self::Fullscreen => KeyCode::F11,
            Self::UncappedSpeed => KeyCode::F10,
            Self::MuteToggle => KeyCode::KeyM,
            Self::Screenshot => KeyCode::F12,
            Self::ResetGame => KeyCode::F6,
            Self::FrameAdvance => KeyCode::KeyN,
            Self::QuickSave => KeyCode::F5,
            Self::QuickLoad => KeyCode::F8,
            Self::SlotNext => KeyCode::BracketRight,
            Self::SlotPrev => KeyCode::BracketLeft,
            Self::DebugContinue => KeyCode::F9,
            Self::DebugStep => KeyCode::F7,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct ShortcutBindings {
    pub(crate) pause: String,
    pub(crate) fullscreen: String,
    pub(crate) uncapped_speed: String,
    pub(crate) mute_toggle: String,
    pub(crate) screenshot: String,
    pub(crate) reset_game: String,
    pub(crate) frame_advance: String,
    pub(crate) quick_save: String,
    pub(crate) quick_load: String,
    pub(crate) slot_next: String,
    pub(crate) slot_prev: String,
    pub(crate) debug_continue: String,
    pub(crate) debug_step: String,
}

impl Default for ShortcutBindings {
    fn default() -> Self {
        Self {
            pause: "KeyP".to_string(),
            fullscreen: "F11".to_string(),
            uncapped_speed: "F10".to_string(),
            mute_toggle: "KeyM".to_string(),
            screenshot: "F12".to_string(),
            reset_game: "F6".to_string(),
            frame_advance: "KeyN".to_string(),
            quick_save: "F5".to_string(),
            quick_load: "F8".to_string(),
            slot_next: "BracketRight".to_string(),
            slot_prev: "BracketLeft".to_string(),
            debug_continue: "F9".to_string(),
            debug_step: "F7".to_string(),
        }
    }
}

impl ShortcutBindings {
    pub(crate) fn get(&self, action: ShortcutAction) -> KeyCode {
        let s = self.key_str(action);
        keycode_from_string(s).unwrap_or(action.default_keycode())
    }

    pub(crate) fn set(&mut self, action: ShortcutAction, key: KeyCode) {
        let s = keycode_to_string(key);
        match action {
            ShortcutAction::Pause => self.pause = s,
            ShortcutAction::Fullscreen => self.fullscreen = s,
            ShortcutAction::UncappedSpeed => self.uncapped_speed = s,
            ShortcutAction::MuteToggle => self.mute_toggle = s,
            ShortcutAction::Screenshot => self.screenshot = s,
            ShortcutAction::ResetGame => self.reset_game = s,
            ShortcutAction::FrameAdvance => self.frame_advance = s,
            ShortcutAction::QuickSave => self.quick_save = s,
            ShortcutAction::QuickLoad => self.quick_load = s,
            ShortcutAction::SlotNext => self.slot_next = s,
            ShortcutAction::SlotPrev => self.slot_prev = s,
            ShortcutAction::DebugContinue => self.debug_continue = s,
            ShortcutAction::DebugStep => self.debug_step = s,
        }
    }

    pub(crate) fn key_str(&self, action: ShortcutAction) -> &str {
        match action {
            ShortcutAction::Pause => &self.pause,
            ShortcutAction::Fullscreen => &self.fullscreen,
            ShortcutAction::UncappedSpeed => &self.uncapped_speed,
            ShortcutAction::MuteToggle => &self.mute_toggle,
            ShortcutAction::Screenshot => &self.screenshot,
            ShortcutAction::ResetGame => &self.reset_game,
            ShortcutAction::FrameAdvance => &self.frame_advance,
            ShortcutAction::QuickSave => &self.quick_save,
            ShortcutAction::QuickLoad => &self.quick_load,
            ShortcutAction::SlotNext => &self.slot_next,
            ShortcutAction::SlotPrev => &self.slot_prev,
            ShortcutAction::DebugContinue => &self.debug_continue,
            ShortcutAction::DebugStep => &self.debug_step,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct GamepadBindings {
    pub(crate) a: String,
    pub(crate) b: String,
    pub(crate) start: String,
    pub(crate) select: String,
    pub(crate) up: String,
    pub(crate) down: String,
    pub(crate) left: String,
    pub(crate) right: String,
    #[serde(default)]
    pub(crate) speedup: String,
    #[serde(default)]
    pub(crate) rewind: String,
    #[serde(default)]
    pub(crate) pause: String,
    #[serde(default)]
    pub(crate) turbo: String,
}

impl Default for GamepadBindings {
    fn default() -> Self {
        Self {
            a: "South".to_string(),
            b: "East".to_string(),
            start: "Start".to_string(),
            select: "Select".to_string(),
            up: "DPadUp".to_string(),
            down: "DPadDown".to_string(),
            left: "DPadLeft".to_string(),
            right: "DPadRight".to_string(),
            speedup: String::new(),
            rewind: String::new(),
            pause: String::new(),
            turbo: String::new(),
        }
    }
}

impl GamepadBindings {
    pub(crate) fn map_button_name(&self, name: &str) -> Option<zeff_gb_core::hardware::joypad::JoypadKey> {
        use zeff_gb_core::hardware::joypad::JoypadKey;
        if name == self.a {
            return Some(JoypadKey::A);
        }
        if name == self.b {
            return Some(JoypadKey::B);
        }
        if name == self.start {
            return Some(JoypadKey::Start);
        }
        if name == self.select {
            return Some(JoypadKey::Select);
        }
        if name == self.up {
            return Some(JoypadKey::Up);
        }
        if name == self.down {
            return Some(JoypadKey::Down);
        }
        if name == self.left {
            return Some(JoypadKey::Left);
        }
        if name == self.right {
            return Some(JoypadKey::Right);
        }
        None
    }

    pub(crate) fn map_action_button_name(&self, name: &str) -> Option<GamepadAction> {
        if !self.speedup.is_empty() && name == self.speedup {
            return Some(GamepadAction::SpeedUp);
        }
        if !self.rewind.is_empty() && name == self.rewind {
            return Some(GamepadAction::Rewind);
        }
        if !self.pause.is_empty() && name == self.pause {
            return Some(GamepadAction::Pause);
        }
        if !self.turbo.is_empty() && name == self.turbo {
            return Some(GamepadAction::Turbo);
        }
        None
    }

    pub(crate) fn get(&self, action: BindingAction) -> &str {
        match action {
            BindingAction::A => &self.a,
            BindingAction::B => &self.b,
            BindingAction::Start => &self.start,
            BindingAction::Select => &self.select,
            BindingAction::Up => &self.up,
            BindingAction::Down => &self.down,
            BindingAction::Left => &self.left,
            BindingAction::Right => &self.right,
        }
    }

    pub(crate) fn set(&mut self, action: BindingAction, button_name: &str) {
        let s = button_name.to_string();
        match action {
            BindingAction::A => self.a = s,
            BindingAction::B => self.b = s,
            BindingAction::Start => self.start = s,
            BindingAction::Select => self.select = s,
            BindingAction::Up => self.up = s,
            BindingAction::Down => self.down = s,
            BindingAction::Left => self.left = s,
            BindingAction::Right => self.right = s,
        }
    }

    pub(crate) fn get_action(&self, action: GamepadAction) -> &str {
        match action {
            GamepadAction::SpeedUp => &self.speedup,
            GamepadAction::Rewind => &self.rewind,
            GamepadAction::Pause => &self.pause,
            GamepadAction::Turbo => &self.turbo,
        }
    }

    pub(crate) fn set_action(&mut self, action: GamepadAction, button_name: &str) {
        let s = button_name.to_string();
        match action {
            GamepadAction::SpeedUp => self.speedup = s,
            GamepadAction::Rewind => self.rewind = s,
            GamepadAction::Pause => self.pause = s,
            GamepadAction::Turbo => self.turbo = s,
        }
    }
}

