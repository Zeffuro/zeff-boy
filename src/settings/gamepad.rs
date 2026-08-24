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

fn default_gp_a() -> String {
    "South".to_string()
}

fn default_gp_b() -> String {
    "East".to_string()
}

fn default_gp_x() -> String {
    "West".to_string()
}

fn default_gp_y() -> String {
    "North".to_string()
}

fn default_gp_l() -> String {
    "LeftTrigger".to_string()
}

fn default_gp_r() -> String {
    "RightTrigger".to_string()
}

fn default_gp_start() -> String {
    "Start".to_string()
}

fn default_gp_select() -> String {
    "Select".to_string()
}

fn default_gp_up() -> String {
    "DPadUp".to_string()
}

fn default_gp_down() -> String {
    "DPadDown".to_string()
}

fn default_gp_left() -> String {
    "DPadLeft".to_string()
}

fn default_gp_right() -> String {
    "DPadRight".to_string()
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
    #[serde(default = "default_gp_x")]
    pub(crate) x: String,
    #[serde(default = "default_gp_y")]
    pub(crate) y: String,
    pub(crate) l: String,
    pub(crate) r: String,
    pub(crate) start: String,
    pub(crate) select: String,
    pub(crate) up: String,
    pub(crate) down: String,
    pub(crate) left: String,
    pub(crate) right: String,
    #[serde(default = "default_gp_a")]
    pub(crate) p2_a: String,
    #[serde(default = "default_gp_b")]
    pub(crate) p2_b: String,
    #[serde(default = "default_gp_x")]
    pub(crate) p2_x: String,
    #[serde(default = "default_gp_y")]
    pub(crate) p2_y: String,
    #[serde(default = "default_gp_l")]
    pub(crate) p2_l: String,
    #[serde(default = "default_gp_r")]
    pub(crate) p2_r: String,
    #[serde(default = "default_gp_start")]
    pub(crate) p2_start: String,
    #[serde(default = "default_gp_select")]
    pub(crate) p2_select: String,
    #[serde(default = "default_gp_up")]
    pub(crate) p2_up: String,
    #[serde(default = "default_gp_down")]
    pub(crate) p2_down: String,
    #[serde(default = "default_gp_left")]
    pub(crate) p2_left: String,
    #[serde(default = "default_gp_right")]
    pub(crate) p2_right: String,
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
            a: default_gp_a(),
            b: default_gp_b(),
            x: default_gp_x(),
            y: default_gp_y(),
            l: default_gp_l(),
            r: default_gp_r(),
            start: default_gp_start(),
            select: default_gp_select(),
            up: default_gp_up(),
            down: default_gp_down(),
            left: default_gp_left(),
            right: default_gp_right(),
            p2_a: default_gp_a(),
            p2_b: default_gp_b(),
            p2_x: default_gp_x(),
            p2_y: default_gp_y(),
            p2_l: default_gp_l(),
            p2_r: default_gp_r(),
            p2_start: default_gp_start(),
            p2_select: default_gp_select(),
            p2_up: default_gp_up(),
            p2_down: default_gp_down(),
            p2_left: default_gp_left(),
            p2_right: default_gp_right(),
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

    pub(crate) fn map_button_name(&self, name: &str) -> Option<crate::input::HostButton> {
        self.map_button_name_for_player(name, 1)
    }

    pub(crate) fn map_button_name_p2(&self, name: &str) -> Option<crate::input::HostButton> {
        self.map_button_name_for_player(name, 2)
    }

    fn map_button_name_for_player(
        &self,
        name: &str,
        player: u8,
    ) -> Option<crate::input::HostButton> {
        use crate::input::HostButton;
        if name == self.get_for_player(BindingAction::A, player) {
            return Some(HostButton::A);
        }
        if name == self.get_for_player(BindingAction::B, player) {
            return Some(HostButton::B);
        }
        if name == self.get_for_player(BindingAction::X, player) {
            return Some(HostButton::X);
        }
        if name == self.get_for_player(BindingAction::Y, player) {
            return Some(HostButton::Y);
        }
        if name == self.get_for_player(BindingAction::L, player) {
            return Some(HostButton::L);
        }
        if name == self.get_for_player(BindingAction::R, player) {
            return Some(HostButton::R);
        }
        if name == self.get_for_player(BindingAction::Start, player) {
            return Some(HostButton::Start);
        }
        if name == self.get_for_player(BindingAction::Select, player) {
            return Some(HostButton::Select);
        }
        if name == self.get_for_player(BindingAction::Up, player) {
            return Some(HostButton::Up);
        }
        if name == self.get_for_player(BindingAction::Down, player) {
            return Some(HostButton::Down);
        }
        if name == self.get_for_player(BindingAction::Left, player) {
            return Some(HostButton::Left);
        }
        if name == self.get_for_player(BindingAction::Right, player) {
            return Some(HostButton::Right);
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
        self.get_for_player(action, 1)
    }

    pub(crate) fn get_p2(&self, action: BindingAction) -> &str {
        self.get_for_player(action, 2)
    }

    fn get_for_player(&self, action: BindingAction, player: u8) -> &str {
        if player == 2 {
            return match action {
                BindingAction::A => &self.p2_a,
                BindingAction::B => &self.p2_b,
                BindingAction::X => &self.p2_x,
                BindingAction::Y => &self.p2_y,
                BindingAction::L => &self.p2_l,
                BindingAction::R => &self.p2_r,
                BindingAction::Start => &self.p2_start,
                BindingAction::Select => &self.p2_select,
                BindingAction::Up => &self.p2_up,
                BindingAction::Down => &self.p2_down,
                BindingAction::Left => &self.p2_left,
                BindingAction::Right => &self.p2_right,
            };
        }

        match action {
            BindingAction::A => &self.a,
            BindingAction::B => &self.b,
            BindingAction::X => &self.x,
            BindingAction::Y => &self.y,
            BindingAction::L => &self.l,
            BindingAction::R => &self.r,
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
            BindingAction::X => self.x = s,
            BindingAction::Y => self.y = s,
            BindingAction::L => self.l = s,
            BindingAction::R => self.r = s,
            BindingAction::Start => self.start = s,
            BindingAction::Select => self.select = s,
            BindingAction::Up => self.up = s,
            BindingAction::Down => self.down = s,
            BindingAction::Left => self.left = s,
            BindingAction::Right => self.right = s,
        }
    }

    pub(crate) fn set_p2(&mut self, action: BindingAction, button_name: &str) {
        let s = button_name.to_string();
        match action {
            BindingAction::A => self.p2_a = s,
            BindingAction::B => self.p2_b = s,
            BindingAction::X => self.p2_x = s,
            BindingAction::Y => self.p2_y = s,
            BindingAction::L => self.p2_l = s,
            BindingAction::R => self.p2_r = s,
            BindingAction::Start => self.p2_start = s,
            BindingAction::Select => self.p2_select = s,
            BindingAction::Up => self.p2_up = s,
            BindingAction::Down => self.p2_down = s,
            BindingAction::Left => self.p2_left = s,
            BindingAction::Right => self.p2_right = s,
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
