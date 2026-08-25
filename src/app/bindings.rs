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

    pub(super) fn map_key_pce_multitap(&self, player: u8, key: KeyCode) -> Option<HostButton> {
        if self.active_system != crate::emu_backend::ActiveSystem::Pce {
            return None;
        }
        let bindings = player.checked_sub(3).and_then(|index| {
            self.settings
                .pce_multitap_key_bindings
                .get(usize::from(index))
        })?;
        crate::settings::BindingAction::ALL
            .iter()
            .copied()
            .find_map(|action| {
                (bindings.get(action) == Some(key)).then(|| host_button_for_action(action))
            })
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
    let bindings: [(KeyCode, HostButton); 12] = [
        (kb.right, HostButton::Right),
        (kb.left, HostButton::Left),
        (kb.up, HostButton::Up),
        (kb.down, HostButton::Down),
        (kb.a, HostButton::A),
        (kb.b, HostButton::B),
        (kb.x, HostButton::X),
        (kb.y, HostButton::Y),
        (kb.l, HostButton::L),
        (kb.r, HostButton::R),
        (kb.start, HostButton::Start),
        (kb.select, HostButton::Select),
    ];
    bindings.iter().find(|(k, _)| *k == key).map(|(_, j)| *j)
}

const fn host_button_for_action(action: crate::settings::BindingAction) -> HostButton {
    use crate::settings::BindingAction;
    match action {
        BindingAction::Up => HostButton::Up,
        BindingAction::Down => HostButton::Down,
        BindingAction::Left => HostButton::Left,
        BindingAction::Right => HostButton::Right,
        BindingAction::A => HostButton::A,
        BindingAction::B => HostButton::B,
        BindingAction::X => HostButton::X,
        BindingAction::Y => HostButton::Y,
        BindingAction::L => HostButton::L,
        BindingAction::R => HostButton::R,
        BindingAction::Start => HostButton::Start,
        BindingAction::Select => HostButton::Select,
    }
}
