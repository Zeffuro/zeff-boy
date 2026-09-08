use crate::input::HostButton;
use crate::settings::{TiltBindingAction, WonderSwanButton};
use winit::keyboard::KeyCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::app) enum HeldFrontendAction {
    FastForward,
    Rewind,
    Turbo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PressTarget {
    Joypad { player: u8, button: HostButton },
    WonderSwan(WonderSwanButton),
    Tilt(TiltBindingAction),
    ColecoKeypad { player: u8, key: u8 },
    Frontend(HeldFrontendAction),
}

#[derive(Debug, Default)]
pub(in crate::app) struct PressedKeyboardTargets {
    entries: Vec<(KeyCode, Vec<PressTarget>)>,
}

impl PressedKeyboardTargets {
    pub(super) fn press(&mut self, key: KeyCode, target: PressTarget) {
        if let Some((_, targets)) = self.entries.iter_mut().find(|(pressed, _)| *pressed == key) {
            if !targets.contains(&target) {
                targets.push(target);
            }
            return;
        }
        self.entries.push((key, vec![target]));
    }

    pub(super) fn release(&mut self, key: KeyCode) -> Vec<PressTarget> {
        let Some(index) = self.entries.iter().position(|(pressed, _)| *pressed == key) else {
            return Vec::new();
        };
        let released = self.entries.swap_remove(index).1;
        released
            .into_iter()
            .filter(|target| {
                !self
                    .entries
                    .iter()
                    .any(|(_, targets)| targets.contains(target))
            })
            .collect()
    }

    pub(super) fn drain(&mut self) -> impl Iterator<Item = PressTarget> + '_ {
        self.entries.drain(..).flat_map(|(_, targets)| targets)
    }
}

#[derive(Debug, Default)]
pub(in crate::app) struct HeldFrontendSources {
    keyboard: [bool; 3],
    gamepad: [bool; 3],
    remote: [bool; 3],
}

impl HeldFrontendSources {
    const fn index(action: HeldFrontendAction) -> usize {
        match action {
            HeldFrontendAction::FastForward => 0,
            HeldFrontendAction::Rewind => 1,
            HeldFrontendAction::Turbo => 2,
        }
    }

    pub(super) fn set_keyboard(&mut self, action: HeldFrontendAction, held: bool) -> bool {
        let index = Self::index(action);
        self.keyboard[index] = held;
        self.is_held(index)
    }

    pub(super) fn set_gamepad(&mut self, action: HeldFrontendAction, held: bool) -> bool {
        let index = Self::index(action);
        self.gamepad[index] = held;
        self.is_held(index)
    }

    pub(super) fn set_remote(&mut self, action: HeldFrontendAction, held: bool) -> bool {
        let index = Self::index(action);
        self.remote[index] = held;
        self.is_held(index)
    }

    pub(super) fn clear_keyboard(&mut self, action: HeldFrontendAction) -> bool {
        self.set_keyboard(action, false)
    }

    pub(super) fn clear_action(&mut self, action: HeldFrontendAction) {
        let index = Self::index(action);
        self.keyboard[index] = false;
        self.gamepad[index] = false;
        self.remote[index] = false;
    }

    pub(super) fn clear_all(&mut self) {
        *self = Self::default();
    }

    fn is_held(&self, index: usize) -> bool {
        self.keyboard[index] || self.gamepad[index] || self.remote[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_uses_press_time_targets_after_bindings_change() {
        let mut pressed = PressedKeyboardTargets::default();
        pressed.press(
            KeyCode::KeyA,
            PressTarget::Joypad {
                player: 1,
                button: HostButton::A,
            },
        );

        assert_eq!(
            pressed.release(KeyCode::KeyA),
            vec![PressTarget::Joypad {
                player: 1,
                button: HostButton::A,
            }]
        );
        assert!(pressed.release(KeyCode::KeyA).is_empty());
    }

    #[test]
    fn one_physical_key_can_own_each_target_it_pressed() {
        let mut pressed = PressedKeyboardTargets::default();
        pressed.press(
            KeyCode::KeyW,
            PressTarget::Joypad {
                player: 1,
                button: HostButton::Up,
            },
        );
        pressed.press(KeyCode::KeyW, PressTarget::Tilt(TiltBindingAction::Up));

        assert_eq!(pressed.release(KeyCode::KeyW).len(), 2);
    }

    #[test]
    fn target_releases_only_after_its_last_physical_key() {
        let mut pressed = PressedKeyboardTargets::default();
        let target = PressTarget::Frontend(HeldFrontendAction::FastForward);
        pressed.press(KeyCode::Space, target);
        pressed.press(KeyCode::Backquote, target);

        assert!(pressed.release(KeyCode::Space).is_empty());
        assert_eq!(pressed.release(KeyCode::Backquote), vec![target]);
    }

    #[test]
    fn keyboard_release_does_not_erase_gamepad_hold() {
        let mut sources = HeldFrontendSources::default();
        assert!(sources.set_keyboard(HeldFrontendAction::Turbo, true));
        assert!(sources.set_gamepad(HeldFrontendAction::Turbo, true));
        assert!(sources.set_keyboard(HeldFrontendAction::Turbo, false));
        assert!(!sources.set_gamepad(HeldFrontendAction::Turbo, false));
    }

    #[test]
    fn device_release_does_not_erase_remote_hold() {
        let mut sources = HeldFrontendSources::default();
        assert!(sources.set_remote(HeldFrontendAction::FastForward, true));
        assert!(sources.set_gamepad(HeldFrontendAction::FastForward, true));
        assert!(sources.set_gamepad(HeldFrontendAction::FastForward, false));
        sources.clear_action(HeldFrontendAction::FastForward);
        assert!(!sources.set_remote(HeldFrontendAction::FastForward, false));
    }
}
