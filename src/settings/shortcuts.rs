use serde::{Deserialize, Serialize};
use winit::keyboard::KeyCode;

use super::keycode_serde::{keycode_from_string, keycode_to_string};

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
