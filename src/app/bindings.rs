use super::App;
use crate::input::HostButton;
use crate::settings::{TiltBindingAction, WonderSwanButton};
use winit::keyboard::KeyCode;

impl App {
    pub(super) fn map_key(&self, key: KeyCode) -> Option<HostButton> {
        map_key_bindings(&self.settings.key_bindings, key)
    }

    pub(super) fn map_key_p2(&self, key: KeyCode) -> Option<HostButton> {
        map_key_bindings(&self.settings.key_bindings_p2, key)
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

    pub(super) fn map_ws_key(&self, key: KeyCode) -> Option<WonderSwanButton> {
        let kb = &self.settings.ws_key_bindings;
        WonderSwanButton::ALL
            .iter()
            .copied()
            .find(|&action| kb.get(action) == key)
    }
}

fn map_key_bindings(kb: &crate::settings::KeyBindings, key: KeyCode) -> Option<HostButton> {
    let bindings: [(KeyCode, HostButton); 10] = [
        (kb.right, HostButton::Right),
        (kb.left, HostButton::Left),
        (kb.up, HostButton::Up),
        (kb.down, HostButton::Down),
        (kb.a, HostButton::A),
        (kb.b, HostButton::B),
        (kb.l, HostButton::L),
        (kb.r, HostButton::R),
        (kb.start, HostButton::Start),
        (kb.select, HostButton::Select),
    ];
    bindings.iter().find(|(k, _)| *k == key).map(|(_, j)| *j)
}
