use super::App;
use crate::settings::TiltBindingAction;
use winit::keyboard::KeyCode;
use zeff_gb_core::hardware::joypad::JoypadKey;

impl App {
    pub(super) fn map_key(&self, key: KeyCode) -> Option<JoypadKey> {
        let kb = &self.settings.key_bindings;
        let bindings: [(KeyCode, JoypadKey); 8] = [
            (kb.right, JoypadKey::Right),
            (kb.left, JoypadKey::Left),
            (kb.up, JoypadKey::Up),
            (kb.down, JoypadKey::Down),
            (kb.a, JoypadKey::A),
            (kb.b, JoypadKey::B),
            (kb.start, JoypadKey::Start),
            (kb.select, JoypadKey::Select),
        ];
        bindings.iter().find(|(k, _)| *k == key).map(|(_, j)| *j)
    }

    pub(super) fn map_tilt_key(&self, key: KeyCode) -> Option<TiltBindingAction> {
        let tb = &self.settings.tilt.key_bindings;
        let bindings: [(KeyCode, TiltBindingAction); 4] = [
            (tb.left, TiltBindingAction::Left),
            (tb.right, TiltBindingAction::Right),
            (tb.up, TiltBindingAction::Up),
            (tb.down, TiltBindingAction::Down),
        ];
        bindings.iter().find(|(k, _)| *k == key).map(|(_, a)| *a)
    }
}
