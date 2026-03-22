use super::App;
use crate::hardware::joypad::JoypadKey;
use crate::settings::TiltBindingAction;
use winit::keyboard::KeyCode;

impl App {
    pub(super) fn keycode_to_state_slot(key: KeyCode) -> Option<u8> {
        match key {
            KeyCode::F1 => Some(1),
            KeyCode::F2 => Some(2),
            KeyCode::F3 => Some(3),
            KeyCode::F4 => Some(4),
            _ => None,
        }
    }

    pub(super) fn map_key(&self, key: KeyCode) -> Option<JoypadKey> {
        let keys = &self.settings.key_bindings;
        if key == keys.right {
            Some(JoypadKey::Right)
        } else if key == keys.left {
            Some(JoypadKey::Left)
        } else if key == keys.up {
            Some(JoypadKey::Up)
        } else if key == keys.down {
            Some(JoypadKey::Down)
        } else if key == keys.a {
            Some(JoypadKey::A)
        } else if key == keys.b {
            Some(JoypadKey::B)
        } else if key == keys.start {
            Some(JoypadKey::Start)
        } else if key == keys.select {
            Some(JoypadKey::Select)
        } else {
            None
        }
    }

    pub(super) fn map_tilt_key(&self, key: KeyCode) -> Option<TiltBindingAction> {
        let keys = &self.settings.tilt_key_bindings;
        if key == keys.left {
            Some(TiltBindingAction::Left)
        } else if key == keys.right {
            Some(TiltBindingAction::Right)
        } else if key == keys.up {
            Some(TiltBindingAction::Up)
        } else if key == keys.down {
            Some(TiltBindingAction::Down)
        } else {
            None
        }
    }
}
