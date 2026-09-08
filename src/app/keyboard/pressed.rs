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
    blocked_gameplay: Vec<KeyCode>,
    expression_keys: Vec<KeyCode>,
    expression_targets: Vec<PressTarget>,
}

impl PressedKeyboardTargets {
    pub(super) fn contains_key(&self, key: KeyCode) -> bool {
        self.entries.iter().any(|(pressed, _)| *pressed == key)
            || self.expression_keys.contains(&key)
            || self.blocked_gameplay.contains(&key)
    }

    pub(super) fn expression_keys(&self) -> &[KeyCode] {
        &self.expression_keys
    }

    pub(super) fn press_expression_key(&mut self, key: KeyCode) {
        if !self.gameplay_blocked(key) && !self.expression_keys.contains(&key) {
            self.expression_keys.push(key);
        }
    }

    pub(super) fn replace_expression_targets(
        &mut self,
        mut targets: Vec<PressTarget>,
    ) -> Vec<(PressTarget, bool)> {
        let mut unique = Vec::new();
        targets.retain(|target| {
            if unique.contains(target) {
                return false;
            }
            unique.push(*target);
            true
        });
        let legacy_holds = |target: &PressTarget| {
            self.entries
                .iter()
                .any(|(_, values)| values.contains(target))
        };
        let mut changes = Vec::new();
        for target in &self.expression_targets {
            if !targets.contains(target) && !legacy_holds(target) {
                changes.push((*target, false));
            }
        }
        for target in &targets {
            if !self.expression_targets.contains(target) && !legacy_holds(target) {
                changes.push((*target, true));
            }
        }
        self.expression_targets = targets;
        changes
    }

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
        self.blocked_gameplay.retain(|blocked| *blocked != key);
        self.expression_keys.retain(|pressed| *pressed != key);
        let Some(index) = self.entries.iter().position(|(pressed, _)| *pressed == key) else {
            return Vec::new();
        };
        let released = self.entries.swap_remove(index).1;
        released
            .into_iter()
            .filter(|target| {
                !self.expression_targets.contains(target)
                    && !self
                        .entries
                        .iter()
                        .any(|(_, targets)| targets.contains(target))
            })
            .collect()
    }

    pub(super) fn drain(&mut self) -> impl Iterator<Item = PressTarget> + '_ {
        self.blocked_gameplay.clear();
        self.expression_keys.clear();
        self.entries
            .drain(..)
            .flat_map(|(_, targets)| targets)
            .chain(self.expression_targets.drain(..))
    }

    pub(super) fn gameplay_blocked(&self, key: KeyCode) -> bool {
        self.blocked_gameplay.contains(&key)
    }

    pub(super) fn suppress_gameplay_until_release(&mut self, key: KeyCode) {
        if !self.blocked_gameplay.contains(&key) {
            self.blocked_gameplay.push(key);
        }
    }

    pub(super) fn drain_gameplay(&mut self) -> Vec<PressTarget> {
        let mut released = std::mem::take(&mut self.expression_targets);
        for key in self.expression_keys.drain(..) {
            if !self.blocked_gameplay.contains(&key) {
                self.blocked_gameplay.push(key);
            }
        }
        for (key, targets) in &mut self.entries {
            let mut removed = false;
            targets.retain(|target| {
                if matches!(target, PressTarget::Frontend(_)) {
                    return true;
                }
                released.push(*target);
                removed = true;
                false
            });
            if removed && !self.blocked_gameplay.contains(key) {
                self.blocked_gameplay.push(*key);
            }
        }
        self.entries.retain(|(_, targets)| !targets.is_empty());
        released
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
    fn expression_and_legacy_owners_release_only_after_both_finish() {
        let mut ledger = PressedKeyboardTargets::default();
        let target = PressTarget::Joypad {
            player: 1,
            button: HostButton::A,
        };
        ledger.press(KeyCode::KeyX, target);
        assert!(
            ledger
                .replace_expression_targets(vec![target, target])
                .is_empty()
        );
        assert!(ledger.release(KeyCode::KeyX).is_empty());
        assert_eq!(
            ledger.replace_expression_targets(vec![]),
            vec![(target, false)]
        );
        assert_eq!(
            ledger.replace_expression_targets(vec![target]),
            vec![(target, true)]
        );
        ledger.press(KeyCode::KeyZ, target);
        assert!(ledger.replace_expression_targets(vec![]).is_empty());
        assert_eq!(ledger.release(KeyCode::KeyZ), vec![target]);
    }

    #[test]
    fn expression_barrier_requires_each_held_key_to_release() {
        let mut ledger = PressedKeyboardTargets::default();
        let target = PressTarget::Joypad {
            player: 2,
            button: HostButton::B,
        };
        ledger.press_expression_key(KeyCode::ControlLeft);
        ledger.press_expression_key(KeyCode::KeyZ);
        ledger.replace_expression_targets(vec![target]);
        assert_eq!(ledger.drain_gameplay(), vec![target]);
        ledger.press_expression_key(KeyCode::KeyZ);
        assert!(ledger.expression_keys().is_empty());
        assert!(ledger.contains_key(KeyCode::KeyZ));
        ledger.release(KeyCode::KeyZ);
        ledger.press_expression_key(KeyCode::KeyZ);
        assert_eq!(ledger.expression_keys(), &[KeyCode::KeyZ]);
        assert!(ledger.gameplay_blocked(KeyCode::ControlLeft));
        ledger.drain().for_each(drop);
        assert!(!ledger.contains_key(KeyCode::ControlLeft));
    }

    #[test]
    fn scope_barrier_releases_gameplay_but_preserves_frontend_targets_and_waits_for_key_up() {
        let mut pressed = PressedKeyboardTargets::default();
        let button = PressTarget::Joypad {
            player: 1,
            button: HostButton::A,
        };
        let frontend = PressTarget::Frontend(HeldFrontendAction::FastForward);
        pressed.press(KeyCode::KeyX, button);
        pressed.press(KeyCode::KeyX, frontend);
        assert_eq!(pressed.drain_gameplay(), vec![button]);
        assert!(pressed.gameplay_blocked(KeyCode::KeyX));
        assert!(pressed.drain_gameplay().is_empty());
        assert_eq!(pressed.release(KeyCode::KeyX), vec![frontend]);
        assert!(!pressed.gameplay_blocked(KeyCode::KeyX));
    }

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
