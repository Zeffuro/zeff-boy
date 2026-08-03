use serde::{Deserialize, Serialize};

use super::binding_actions::{BindingAction, WonderSwanButton};

fn default_ws_a() -> String {
    "South".to_string()
}

fn default_ws_b() -> String {
    "East".to_string()
}

fn default_ws_start() -> String {
    "Start".to_string()
}

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
    #[serde(default)]
    pub(crate) ws_x1: String,
    #[serde(default)]
    pub(crate) ws_x2: String,
    #[serde(default)]
    pub(crate) ws_x3: String,
    #[serde(default)]
    pub(crate) ws_x4: String,
    #[serde(default)]
    pub(crate) ws_y1: String,
    #[serde(default)]
    pub(crate) ws_y2: String,
    #[serde(default)]
    pub(crate) ws_y3: String,
    #[serde(default)]
    pub(crate) ws_y4: String,
    #[serde(default = "default_ws_a")]
    pub(crate) ws_a: String,
    #[serde(default = "default_ws_b")]
    pub(crate) ws_b: String,
    #[serde(default = "default_ws_start")]
    pub(crate) ws_start: String,
    #[serde(default)]
    pub(crate) wonderswan_defaults_initialized: bool,
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
            ws_x1: String::new(),
            ws_x2: String::new(),
            ws_x3: String::new(),
            ws_x4: String::new(),
            ws_y1: String::new(),
            ws_y2: String::new(),
            ws_y3: String::new(),
            ws_y4: String::new(),
            ws_a: default_ws_a(),
            ws_b: default_ws_b(),
            ws_start: default_ws_start(),
            wonderswan_defaults_initialized: true,
        }
    }
}

impl GamepadBindings {
    pub(crate) fn migrate_wonderswan_defaults(&mut self) {
        if !self.wonderswan_defaults_initialized && self.wonderswan_direct_bindings_are_empty() {
            self.reset_wonderswan_defaults();
        }
        self.wonderswan_defaults_initialized = true;
    }

    pub(crate) fn reset_wonderswan_defaults(&mut self) {
        self.ws_x1.clear();
        self.ws_x2.clear();
        self.ws_x3.clear();
        self.ws_x4.clear();
        self.ws_y1.clear();
        self.ws_y2.clear();
        self.ws_y3.clear();
        self.ws_y4.clear();
        self.ws_a = default_ws_a();
        self.ws_b = default_ws_b();
        self.ws_start = default_ws_start();
        self.wonderswan_defaults_initialized = true;
    }

    pub(crate) fn clear_wonderswan_direct_bindings(&mut self) {
        self.ws_x1.clear();
        self.ws_x2.clear();
        self.ws_x3.clear();
        self.ws_x4.clear();
        self.ws_y1.clear();
        self.ws_y2.clear();
        self.ws_y3.clear();
        self.ws_y4.clear();
        self.ws_a.clear();
        self.ws_b.clear();
        self.ws_start.clear();
        self.wonderswan_defaults_initialized = true;
    }

    fn wonderswan_direct_bindings_are_empty(&self) -> bool {
        self.ws_x1.is_empty()
            && self.ws_x2.is_empty()
            && self.ws_x3.is_empty()
            && self.ws_x4.is_empty()
            && self.ws_y1.is_empty()
            && self.ws_y2.is_empty()
            && self.ws_y3.is_empty()
            && self.ws_y4.is_empty()
            && self.ws_a.is_empty()
            && self.ws_b.is_empty()
            && self.ws_start.is_empty()
    }

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

    pub(crate) fn map_ws_button_name(&self, name: &str) -> Option<WonderSwanButton> {
        for &button in WonderSwanButton::ALL {
            let bound = self.get_ws(button);
            if !bound.is_empty() && name == bound {
                return Some(button);
            }
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

    pub(crate) fn get_ws(&self, button: WonderSwanButton) -> &str {
        match button {
            WonderSwanButton::X1 => &self.ws_x1,
            WonderSwanButton::X2 => &self.ws_x2,
            WonderSwanButton::X3 => &self.ws_x3,
            WonderSwanButton::X4 => &self.ws_x4,
            WonderSwanButton::Y1 => &self.ws_y1,
            WonderSwanButton::Y2 => &self.ws_y2,
            WonderSwanButton::Y3 => &self.ws_y3,
            WonderSwanButton::Y4 => &self.ws_y4,
            WonderSwanButton::A => &self.ws_a,
            WonderSwanButton::B => &self.ws_b,
            WonderSwanButton::Start => &self.ws_start,
        }
    }

    pub(crate) fn set_ws(&mut self, button: WonderSwanButton, button_name: &str) {
        let s = button_name.to_string();
        match button {
            WonderSwanButton::X1 => self.ws_x1 = s,
            WonderSwanButton::X2 => self.ws_x2 = s,
            WonderSwanButton::X3 => self.ws_x3 = s,
            WonderSwanButton::X4 => self.ws_x4 = s,
            WonderSwanButton::Y1 => self.ws_y1 = s,
            WonderSwanButton::Y2 => self.ws_y2 = s,
            WonderSwanButton::Y3 => self.ws_y3 = s,
            WonderSwanButton::Y4 => self.ws_y4 = s,
            WonderSwanButton::A => self.ws_a = s,
            WonderSwanButton::B => self.ws_b = s,
            WonderSwanButton::Start => self.ws_start = s,
        }
        self.wonderswan_defaults_initialized = true;
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
