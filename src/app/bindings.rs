use super::App;
use crate::input::HostButton;
use crate::settings::{BindingTarget, TiltBindingAction, WonderSwanButton};
use winit::keyboard::KeyCode;

impl App {
    pub(super) fn map_key(&self, key: KeyCode) -> Option<HostButton> {
        map_key_bindings(&self.input_configuration.resolved, 1, key)
    }

    pub(super) fn map_key_p2(&self, key: KeyCode) -> Option<HostButton> {
        map_key_bindings(&self.input_configuration.resolved, 2, key)
    }

    pub(super) fn map_key_pce_multitap(&self, player: u8, key: KeyCode) -> Option<HostButton> {
        if self.active_system != crate::emu_backend::ActiveSystem::Pce {
            return None;
        }
        if !(3..=5).contains(&player) {
            return None;
        }
        crate::settings::BindingAction::ALL
            .iter()
            .copied()
            .find_map(|action| {
                (self
                    .input_configuration
                    .resolved
                    .keyboard_binding(BindingTarget::Joypad { player, action })
                    == Some(key)
                    && !self.input_configuration.resolved.has_typed_binding(
                        BindingTarget::Joypad { player, action },
                        crate::settings::GameplayBindingSource::Keyboard,
                    ))
                .then(|| host_button_for_action(action))
            })
    }

    pub(super) fn map_tilt_key(&self, key: KeyCode) -> Option<TiltBindingAction> {
        [
            TiltBindingAction::Left,
            TiltBindingAction::Right,
            TiltBindingAction::Up,
            TiltBindingAction::Down,
        ]
        .into_iter()
        .find(|&action| {
            self.input_configuration
                .resolved
                .keyboard_binding(BindingTarget::Tilt(action))
                == Some(key)
                && !self.input_configuration.resolved.has_typed_binding(
                    BindingTarget::Tilt(action),
                    crate::settings::GameplayBindingSource::Keyboard,
                )
        })
    }

    pub(super) fn map_ws_key(&self, key: KeyCode) -> Option<WonderSwanButton> {
        WonderSwanButton::ALL.iter().copied().find(|&action| {
            self.input_configuration
                .resolved
                .keyboard_binding(BindingTarget::WonderSwan(action))
                == Some(key)
                && !self.input_configuration.resolved.has_typed_binding(
                    BindingTarget::WonderSwan(action),
                    crate::settings::GameplayBindingSource::Keyboard,
                )
        })
    }

    pub(super) fn coleco_keypad_key(&self, key: KeyCode) -> Option<u8> {
        (self.active_system == crate::emu_backend::ActiveSystem::Coleco)
            .then(|| coleco_keypad_key(key))
            .flatten()
    }
}

fn coleco_keypad_key(key: KeyCode) -> Option<u8> {
    Some(match key {
        KeyCode::Digit0 => 0,
        KeyCode::Digit1 => 1,
        KeyCode::Digit2 => 2,
        KeyCode::Digit3 => 3,
        KeyCode::Digit4 => 4,
        KeyCode::Digit5 => 5,
        KeyCode::Digit6 => 6,
        KeyCode::Digit7 => 7,
        KeyCode::Digit8 => 8,
        KeyCode::Digit9 => 9,
        KeyCode::Minus => 10,
        KeyCode::Equal => 11,
        _ => return None,
    })
}

fn map_key_bindings(
    bindings: &crate::settings::ResolvedGameplayInput,
    player: u8,
    key: KeyCode,
) -> Option<HostButton> {
    use crate::settings::BindingAction::*;
    [Right, Left, Up, Down, A, B, X, Y, L, R, Start, Select]
        .into_iter()
        .find(|&action| {
            bindings.keyboard_binding(BindingTarget::Joypad { player, action }) == Some(key)
                && !bindings.has_typed_binding(
                    BindingTarget::Joypad { player, action },
                    crate::settings::GameplayBindingSource::Keyboard,
                )
        })
        .map(host_button_for_action)
}

pub(in crate::app) const fn host_button_for_action(
    action: crate::settings::BindingAction,
) -> HostButton {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coleco_keypad_reserves_only_the_number_row() {
        assert_eq!(coleco_keypad_key(KeyCode::Digit0), Some(0));
        assert_eq!(coleco_keypad_key(KeyCode::Minus), Some(10));
        assert_eq!(coleco_keypad_key(KeyCode::Equal), Some(11));
        assert_eq!(coleco_keypad_key(KeyCode::Numpad8), None);
        assert_eq!(coleco_keypad_key(KeyCode::NumpadAdd), None);
    }
}
