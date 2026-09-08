use super::pressed::PressTarget;
use crate::emu_backend::ActiveSystem;
use crate::settings::{
    BindingExpressionKind, BindingTarget, GameplayBindingSource, ResolvedGameplayInput,
};
use winit::keyboard::KeyCode;

pub(super) fn active_targets(
    bindings: &ResolvedGameplayInput,
    system: ActiveSystem,
    keys: &[KeyCode],
    fds_disk_shortcuts: bool,
) -> Vec<PressTarget> {
    bindings
        .typed_binding_sets()
        .filter_map(|(target, source, set)| {
            if source != GameplayBindingSource::Keyboard {
                return None;
            }
            let active = set.evaluate(|_, atom| match atom {
                BindingExpressionKind::Keyboard(key) => {
                    keys.contains(key) && !reserved_chord_key(*key, keys, fds_disk_shortcuts)
                }
                _ => false,
            });
            if !active {
                return None;
            }
            match target {
                BindingTarget::Joypad { player, action }
                    if player == 1
                        || player == 2 && system != ActiveSystem::WonderSwan
                        || (3..=5).contains(&player) && system == ActiveSystem::Pce =>
                {
                    Some(PressTarget::Joypad {
                        player,
                        button: crate::app::bindings::host_button_for_action(action),
                    })
                }
                BindingTarget::WonderSwan(button) if system == ActiveSystem::WonderSwan => {
                    Some(PressTarget::WonderSwan(button))
                }
                BindingTarget::Tilt(action) => Some(PressTarget::Tilt(action)),
                _ => None,
            }
        })
        .collect()
}

fn reserved_chord_key(key: KeyCode, keys: &[KeyCode], fds_disk_shortcuts: bool) -> bool {
    let ctrl = keys
        .iter()
        .any(|key| matches!(key, KeyCode::ControlLeft | KeyCode::ControlRight));
    let alt = keys
        .iter()
        .any(|key| matches!(key, KeyCode::AltLeft | KeyCode::AltRight));
    key == KeyCode::KeyR && ctrl
        || key == KeyCode::Enter && alt
        || fds_disk_shortcuts && ctrl && alt && matches!(key, KeyCode::KeyA | KeyCode::KeyB)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{BindingAction, BindingExpression, BindingSet, InputScope, Settings};

    fn mappings(
        target: BindingTarget,
        expressions: Vec<BindingExpression>,
    ) -> ResolvedGameplayInput {
        let mut settings = Settings::default();
        let mut set = BindingSet::new(expressions[0].clone());
        for expression in expressions.into_iter().skip(1) {
            set.add_expression(expression).unwrap();
        }
        settings
            .set_binding_set(
                &InputScope::Global,
                target,
                GameplayBindingSource::Keyboard,
                Some(set),
            )
            .unwrap();
        settings.resolve_gameplay_input(&InputScope::Global)
    }

    #[test]
    fn alternatives_and_chords_follow_all_held_atoms_in_either_press_order() {
        let target = BindingTarget::Joypad {
            player: 1,
            action: BindingAction::A,
        };
        let bindings = mappings(
            target,
            vec![
                BindingExpression::keyboard(KeyCode::KeyX),
                BindingExpression::chord(vec![
                    BindingExpression::keyboard(KeyCode::ControlLeft),
                    BindingExpression::keyboard(KeyCode::KeyZ),
                ]),
            ],
        );
        let active =
            |keys: &[KeyCode]| active_targets(&bindings, ActiveSystem::Gb, keys, false).len();
        assert_eq!(active(&[KeyCode::KeyX]), 1);
        assert_eq!(
            active(&[KeyCode::KeyX, KeyCode::ControlLeft, KeyCode::KeyZ]),
            1
        );
        assert_eq!(active(&[KeyCode::ControlLeft, KeyCode::KeyZ]), 1);
        assert_eq!(active(&[KeyCode::KeyZ, KeyCode::ControlLeft]), 1);
        assert_eq!(active(&[KeyCode::ControlRight, KeyCode::KeyZ]), 0);
        assert_eq!(active(&[KeyCode::KeyZ]), 0);
        assert_eq!(active(&[KeyCode::ControlLeft]), 0);
    }

    #[test]
    fn reserved_frontend_chords_do_not_activate_gameplay_in_reverse_order() {
        let bindings = mappings(
            BindingTarget::Joypad {
                player: 1,
                action: BindingAction::A,
            },
            vec![BindingExpression::keyboard(KeyCode::KeyR)],
        );
        assert_eq!(
            active_targets(&bindings, ActiveSystem::Gb, &[KeyCode::KeyR], false).len(),
            1
        );
        assert!(
            active_targets(
                &bindings,
                ActiveSystem::Gb,
                &[KeyCode::KeyR, KeyCode::ControlLeft],
                false
            )
            .is_empty()
        );
    }

    #[test]
    fn typed_multiplayer_bindings_obey_existing_system_topology() {
        let bindings = mappings(
            BindingTarget::Joypad {
                player: 5,
                action: BindingAction::A,
            },
            vec![BindingExpression::keyboard(KeyCode::KeyX)],
        );
        assert!(active_targets(&bindings, ActiveSystem::Gb, &[KeyCode::KeyX], false).is_empty());
        assert_eq!(
            active_targets(&bindings, ActiveSystem::Pce, &[KeyCode::KeyX], false).len(),
            1
        );
    }

    #[test]
    fn disk_shortcuts_suppress_both_keys_with_either_physical_modifier_side() {
        for key in [KeyCode::KeyA, KeyCode::KeyB] {
            let bindings = mappings(
                BindingTarget::Joypad {
                    player: 1,
                    action: BindingAction::A,
                },
                vec![BindingExpression::keyboard(key)],
            );
            for ctrl in [KeyCode::ControlLeft, KeyCode::ControlRight] {
                for alt in [KeyCode::AltLeft, KeyCode::AltRight] {
                    assert_eq!(
                        active_targets(&bindings, ActiveSystem::Nes, &[key, ctrl], true).len(),
                        1
                    );
                    assert_eq!(
                        active_targets(&bindings, ActiveSystem::Nes, &[key, alt], true).len(),
                        1
                    );
                    assert!(
                        active_targets(&bindings, ActiveSystem::Nes, &[key, ctrl, alt], true)
                            .is_empty()
                    );
                    assert!(
                        active_targets(&bindings, ActiveSystem::Nes, &[alt, ctrl, key], true)
                            .is_empty()
                    );
                    assert_eq!(
                        active_targets(&bindings, ActiveSystem::Nes, &[key, ctrl, alt], false)
                            .len(),
                        1
                    );
                }
            }
        }
    }

    #[test]
    fn reserved_modifiers_do_not_suppress_unrelated_alternatives() {
        let bindings = mappings(
            BindingTarget::Joypad {
                player: 1,
                action: BindingAction::A,
            },
            vec![
                BindingExpression::keyboard(KeyCode::KeyA),
                BindingExpression::keyboard(KeyCode::KeyX),
            ],
        );
        assert_eq!(
            active_targets(
                &bindings,
                ActiveSystem::Nes,
                &[
                    KeyCode::KeyA,
                    KeyCode::ControlLeft,
                    KeyCode::AltRight,
                    KeyCode::KeyX
                ],
                true
            )
            .len(),
            1
        );
    }
}
