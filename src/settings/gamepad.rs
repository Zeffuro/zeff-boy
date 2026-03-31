use serde::{Deserialize, Serialize};

use super::binding_actions::BindingAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GamepadAction {
    SpeedUp,
    Rewind,
    Pause,
    Turbo,
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
    pub(crate) fn map_button_name(
        &self,
        name: &str,
    ) -> Option<zeff_gb_core::hardware::joypad::JoypadKey> {
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
