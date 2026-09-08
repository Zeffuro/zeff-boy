use super::*;

fn a() -> BindingTarget {
    BindingTarget::Joypad {
        player: 1,
        action: BindingAction::A,
    }
}
fn key(key: KeyCode) -> Option<PhysicalBinding> {
    Some(PhysicalBinding::Keyboard(key))
}

fn alternatives() -> BindingSet {
    let mut bindings = BindingSet::new(super::super::BindingExpression::keyboard(KeyCode::KeyX));
    bindings
        .add_expression(super::super::BindingExpression::chord(vec![
            super::super::BindingExpression::keyboard(KeyCode::ControlLeft),
            super::super::BindingExpression::keyboard(KeyCode::KeyZ),
        ]))
        .unwrap();
    bindings
}

#[test]
fn typed_sets_inherit_pin_unbind_and_return_to_legacy_without_ghosts() {
    let mut settings = Settings::default();
    let global = InputScope::Global;
    let system = InputScope::System(InputSystem::GameBoyAdvance);
    let game = InputScope::Game(InputGameKey::new(InputSystem::GameBoyAdvance, [4; 32]));
    let source = GameplayBindingSource::Keyboard;
    let bindings = alternatives();
    settings
        .set_binding_set(&global, a(), source, Some(bindings.clone()))
        .unwrap();
    settings
        .set_binding_set(&system, a(), source, Some(bindings.clone()))
        .unwrap();
    assert_eq!(settings.binding_set(&game, a(), source).origin, system);
    assert_eq!(
        settings.binding_set(&game, a(), source).value,
        Some(bindings.clone())
    );
    assert!(settings.binding(&game, a(), source).value.is_none());
    settings
        .set_binding(&global, a(), source, key(KeyCode::KeyC))
        .unwrap();
    assert_eq!(
        settings.binding_set(&game, a(), source).value,
        Some(bindings.clone())
    );
    settings.set_binding_set(&game, a(), source, None).unwrap();
    let resolved = settings.resolve_gameplay_input(&game);
    assert!(resolved.has_typed_binding(a(), source));
    assert!(resolved.binding_set(a(), source).is_none());
    assert!(resolved.keyboard_binding(a()).is_none());
    assert_eq!(resolved.typed_binding_sets().count(), 0);
    settings.inherit_binding(&game, a(), source);
    assert_eq!(
        settings.binding_set(&game, a(), source).value,
        Some(bindings)
    );
    settings.inherit_binding(&system, a(), source);
    assert!(!settings.binding_set(&game, a(), source).typed);
    assert_eq!(
        settings.binding(&game, a(), source).value,
        key(KeyCode::KeyC)
    );
}

#[test]
fn legacy_overrides_shadow_parent_typed_sets_and_inherit_removes_both_local_records() {
    let mut settings = Settings::default();
    let system = InputScope::System(InputSystem::GameBoyAdvance);
    let source = GameplayBindingSource::Keyboard;
    settings
        .set_binding_set(&InputScope::Global, a(), source, Some(alternatives()))
        .unwrap();
    settings
        .set_binding(&system, a(), source, key(KeyCode::KeyC))
        .unwrap();
    assert!(!settings.binding_set(&system, a(), source).typed);
    assert_eq!(
        settings.binding(&system, a(), source).value,
        key(KeyCode::KeyC)
    );
    settings
        .set_binding_set(&system, a(), source, Some(alternatives()))
        .unwrap();
    assert!(settings.binding_set(&system, a(), source).typed);
    settings.inherit_binding(&system, a(), source);
    assert_eq!(
        settings.binding_set(&system, a(), source).origin,
        InputScope::Global
    );
    assert!(settings.binding_set(&system, a(), source).typed);
}

#[test]
fn absent_typed_fields_keep_every_legacy_single_binding_available_unchanged() {
    let mut settings = Settings {
        input_overrides: serde_json::from_str(
            r#"{"global_keyboard_unbound":[],"systems":{},"games":{}}"#,
        )
        .unwrap(),
        ..Default::default()
    };
    settings.key_bindings.a = KeyCode::KeyQ;
    settings.key_bindings.b = KeyCode::KeyQ;
    settings
        .gamepad_bindings
        .set_for_player(BindingAction::A, 1, "North");
    settings
        .gamepad_bindings
        .set_for_player(BindingAction::B, 1, "North");
    let resolved = settings.resolve_gameplay_input(&InputScope::Global);
    for target in BindingTarget::all() {
        for source in [
            GameplayBindingSource::Keyboard,
            GameplayBindingSource::Gamepad,
        ] {
            if target.valid(source) {
                assert!(!resolved.has_typed_binding(target, source));
                assert_eq!(
                    resolved
                        .binding_set(target, source)
                        .and_then(BindingSet::as_single_physical_binding),
                    settings.global_binding(target, source)
                );
            }
        }
    }
    assert_eq!(
        resolved.gamepad.map_button_name_for_player("North", 1),
        settings
            .gamepad_bindings
            .map_button_name_for_player("North", 1)
    );
}

#[test]
fn typed_profile_copy_preserves_complete_sets_future_fields_and_shortcut_ownership() {
    let mut settings = Settings::default();
    let system = InputScope::System(InputSystem::GameBoyAdvance);
    let game = InputScope::Game(InputGameKey::new(InputSystem::GameBoyAdvance, [5; 32]));
    let source = GameplayBindingSource::Keyboard;
    let record: BindingSetOverride = serde_json::from_value(serde_json::json!({
        "state":"set", "future_record": {"keep":true}, "value": {
            "future_set":9, "alternatives":[
                {"id":3,"expression":{"kind":"keyboard","key":"KeyX","future_key":4}},
                {"id":8,"expression":{"kind":"future_control","data":[1,2,3]}}
            ]
        }
    }))
    .unwrap();
    settings
        .input_overrides
        .binding_sets_mut(&system)
        .insert(source.key(a()), record.clone());
    let profile = settings.capture_controller_profile(&game);
    assert_eq!(profile.binding_sets[&source.key(a())], record);
    let json = serde_json::to_string(&profile).unwrap();
    let profile: InputProfileBindings = serde_json::from_str(&json).unwrap();
    let shortcuts = settings.shortcut_bindings.clone();
    let speedup = settings.speedup_key.clone();
    let devices = settings.input_devices.clone();
    let emulation = settings.emulation.clone();
    settings
        .apply_controller_profile(&InputScope::Global, &profile)
        .unwrap();
    assert_eq!(
        settings.input_overrides.global_binding_sets[&source.key(a())],
        record
    );
    assert_eq!(settings.shortcut_bindings, shortcuts);
    assert_eq!(settings.speedup_key, speedup);
    assert_eq!(settings.input_devices, devices);
    assert_eq!(settings.emulation, emulation);
    assert_eq!(
        settings
            .binding_set(&InputScope::Global, a(), source)
            .value
            .unwrap()
            .alternatives
            .len(),
        2
    );
}

#[test]
fn typed_reset_is_local_and_invalid_sets_do_not_mutate_settings() {
    let mut settings = Settings::default();
    let global = InputScope::Global;
    let system = InputScope::System(InputSystem::GameBoyAdvance);
    let source = GameplayBindingSource::Keyboard;
    settings
        .set_binding_set(&global, a(), source, Some(alternatives()))
        .unwrap();
    settings
        .set_binding_set(&system, a(), source, None)
        .unwrap();
    let before = settings.clone();
    assert!(
        settings
            .set_binding_set(&system, a(), source, Some(BindingSet::default()))
            .is_err()
    );
    assert_eq!(settings, before);
    settings.reset_input_scope(&global);
    assert!(!settings.binding_set(&global, a(), source).typed);
    assert!(settings.binding_set(&system, a(), source).typed);
    assert!(settings.binding_set(&system, a(), source).value.is_none());
    settings.reset_input_scope(&system);
    assert!(!settings.binding_set(&system, a(), source).typed);
}

#[test]
fn sparse_inheritance_preserves_same_value_pins_and_explicit_unbound() {
    let mut settings = Settings::default();
    let system = InputScope::System(InputSystem::GameBoyAdvance);
    let game = InputScope::Game(InputGameKey::new(InputSystem::GameBoyAdvance, [7; 32]));
    let source = GameplayBindingSource::Keyboard;
    settings
        .set_binding(&system, a(), source, key(KeyCode::KeyX))
        .unwrap();
    assert_eq!(settings.binding(&game, a(), source).origin, system);
    settings
        .set_binding(&InputScope::Global, a(), source, key(KeyCode::KeyQ))
        .unwrap();
    assert_eq!(
        settings.binding(&game, a(), source).value,
        key(KeyCode::KeyX)
    );
    settings.set_binding(&game, a(), source, None).unwrap();
    assert_eq!(settings.binding(&game, a(), source).value, None);
    assert_eq!(settings.binding(&game, a(), source).origin, game);
    settings.inherit_binding(&game, a(), source);
    assert_eq!(
        settings.binding(&game, a(), source).value,
        key(KeyCode::KeyX)
    );
    settings.inherit_binding(&system, a(), source);
    assert_eq!(
        settings.binding(&game, a(), source).value,
        key(KeyCode::KeyQ)
    );
    assert_eq!(
        settings.binding(&game, a(), source).origin,
        InputScope::Global
    );
}

#[test]
fn game_and_system_keys_never_share_scope_by_name_or_prefix() {
    let mut settings = Settings::default();
    let gba = InputGameKey::new(InputSystem::GameBoyAdvance, [3; 32]);
    let gb = InputGameKey::new(InputSystem::GameBoy, [3; 32]);
    let other = InputGameKey::new(InputSystem::GameBoyAdvance, [4; 32]);
    assert!(
        gba.storage_key()
            .starts_with("effective-content-sha256-v1:gba:")
    );
    assert_ne!(gba.storage_key(), gb.storage_key());
    assert_ne!(gba.storage_key(), other.storage_key());
    settings
        .set_binding(
            &InputScope::Game(gba.clone()),
            a(),
            GameplayBindingSource::Keyboard,
            None,
        )
        .unwrap();
    assert!(
        settings
            .binding(&InputScope::Game(gba), a(), GameplayBindingSource::Keyboard)
            .value
            .is_none()
    );
    for game in [gb, other] {
        assert_eq!(
            settings
                .binding(
                    &InputScope::Game(game),
                    a(),
                    GameplayBindingSource::Keyboard
                )
                .value,
            key(KeyCode::KeyX)
        );
    }
}

#[test]
fn global_unbound_is_typed_and_survives_full_profile_roundtrip() {
    let mut settings = Settings::default();
    settings
        .set_binding(
            &InputScope::Global,
            a(),
            GameplayBindingSource::Keyboard,
            None,
        )
        .unwrap();
    assert_eq!(settings.key_bindings.a, KeyCode::KeyX);
    assert_eq!(
        settings
            .resolve_gameplay_input(&InputScope::Global)
            .keyboard_binding(a()),
        None
    );
    let profile = settings.capture_input_profile();
    let json = serde_json::to_string(&profile).unwrap();
    let decoded: InputProfileBindings = serde_json::from_str(&json).unwrap();
    let mut restored = Settings::default();
    restored.apply_input_profile(&decoded);
    assert_eq!(
        restored
            .resolve_gameplay_input(&InputScope::Global)
            .keyboard_binding(a()),
        None
    );
    restored
        .set_binding(
            &InputScope::Global,
            a(),
            GameplayBindingSource::Keyboard,
            key(KeyCode::KeyZ),
        )
        .unwrap();
    assert_eq!(
        restored
            .resolve_gameplay_input(&InputScope::Global)
            .keyboard_binding(a()),
        Some(KeyCode::KeyZ)
    );
}

#[test]
fn controller_profile_load_copies_gameplay_without_changing_global_commands_or_devices() {
    let mut source = Settings::default();
    source.key_bindings.a = KeyCode::KeyQ;
    source.speedup_key = "KeyW".into();
    source
        .gamepad_bindings
        .set_action(GamepadAction::Pause, "West");
    let profile = source.capture_input_profile();
    let mut settings = Settings::default();
    settings.input_devices.players[0] = super::super::GamepadAssignment::Disabled;
    settings
        .gamepad_bindings
        .set_action(GamepadAction::Pause, "North");
    let before = settings.capture_input_profile();
    let scope = InputScope::System(InputSystem::GameBoyAdvance);
    settings.apply_controller_profile(&scope, &profile).unwrap();
    assert_eq!(settings.capture_input_profile(), before);
    assert_eq!(
        settings.input_devices.players[0],
        super::super::GamepadAssignment::Disabled
    );
    assert_eq!(
        settings
            .resolve_gameplay_input(&scope)
            .keyboard_binding(a()),
        Some(KeyCode::KeyQ)
    );
    assert_eq!(
        settings
            .resolve_gameplay_input(&scope)
            .gamepad
            .get_action(GamepadAction::Pause),
        "North"
    );
    settings.apply_profile_shortcuts(&profile);
    assert_eq!(settings.speedup_key, "KeyW");
    assert_eq!(
        settings.gamepad_bindings.get_action(GamepadAction::Pause),
        "West"
    );
}

#[test]
fn transforms_inherit_per_field_and_scope_reset_leaves_global_and_other_scopes() {
    let mut settings = Settings::default();
    let scope = InputScope::System(InputSystem::GameBoyAdvance);
    let game = InputScope::Game(InputGameKey::new(InputSystem::GameBoyAdvance, [2; 32]));
    settings
        .set_transform(
            &scope,
            GameplayTransform::Deadzone,
            TransformValue::Float(0.4),
        )
        .unwrap();
    settings
        .set_transform(
            &game,
            GameplayTransform::InvertX,
            TransformValue::Bool(true),
        )
        .unwrap();
    assert_eq!(settings.resolve_gameplay_input(&game).tilt.deadzone, 0.4);
    assert!(settings.resolve_gameplay_input(&game).tilt.invert_x);
    assert_eq!(
        settings
            .transform(&game, GameplayTransform::Deadzone)
            .origin,
        scope
    );
    settings.reset_input_scope(&game);
    assert!(!settings.resolve_gameplay_input(&game).tilt.invert_x);
    assert_eq!(settings.resolve_gameplay_input(&game).tilt.deadzone, 0.4);
    assert_ne!(settings.tilt.deadzone, 0.4);
    assert!(
        settings
            .set_transform(
                &scope,
                GameplayTransform::Deadzone,
                TransformValue::Float(f32::NAN)
            )
            .is_err()
    );
    assert_eq!(settings.resolve_gameplay_input(&scope).tilt.deadzone, 0.4);
}

#[test]
fn source_mismatch_and_invalid_player_do_not_create_overrides() {
    let mut settings = Settings::default();
    let before = settings.clone();
    let scope = InputScope::System(InputSystem::GameBoyAdvance);
    assert!(
        settings
            .set_binding(
                &scope,
                a(),
                GameplayBindingSource::Gamepad,
                key(KeyCode::KeyQ)
            )
            .is_err()
    );
    assert!(
        settings
            .set_binding(
                &scope,
                BindingTarget::Joypad {
                    player: 0,
                    action: BindingAction::A
                },
                GameplayBindingSource::Keyboard,
                None
            )
            .is_err()
    );
    assert_eq!(settings, before);
}

#[test]
fn autofire_inherits_by_scope_and_profiles_copy_effective_values() {
    let mut settings = Settings::default();
    let target = AutofireTarget {
        player: 2,
        action: BindingAction::B,
    };
    let system = InputScope::System(InputSystem::GameBoyAdvance);
    let game = InputScope::Game(InputGameKey::new(InputSystem::GameBoyAdvance, [8; 32]));
    let pattern = AutofirePattern {
        period_frames: 5,
        on_frames: 3,
    };
    settings
        .set_autofire(
            &InputScope::Global,
            target,
            AutofireOverride::enabled(pattern),
        )
        .unwrap();
    assert_eq!(settings.autofire(&game, target).value, Some(pattern));
    assert_eq!(settings.autofire(&game, target).origin, InputScope::Global);
    settings
        .set_autofire(&system, target, AutofireOverride::disabled())
        .unwrap();
    assert_eq!(settings.autofire(&game, target).value, None);
    assert_eq!(settings.autofire(&game, target).origin, system);
    settings.inherit_autofire(&system, target);
    let profile = settings.capture_controller_profile(&game);
    assert_eq!(profile.autofire[&target.key()].value(), Some(pattern));

    let mut restored = Settings::default();
    restored
        .apply_controller_profile(&system, &profile)
        .unwrap();
    assert_eq!(restored.autofire(&game, target).value, Some(pattern));
    assert_eq!(
        restored.autofire(&game, target).origin,
        InputScope::System(InputSystem::GameBoyAdvance)
    );
}

#[test]
fn autofire_rejects_invalid_patterns_and_ignores_future_targets() {
    let mut settings = Settings::default();
    let target = AutofireTarget {
        player: 1,
        action: BindingAction::A,
    };
    let before = settings.clone();
    assert!(
        settings
            .set_autofire(
                &InputScope::Global,
                target,
                AutofireOverride::enabled(AutofirePattern {
                    period_frames: 3,
                    on_frames: 4,
                }),
            )
            .is_err()
    );
    assert_eq!(settings, before);
    settings.input_overrides.global_autofire.insert(
        "p1.future".into(),
        serde_json::from_str(r#"{"state":"wave","keep":true}"#).unwrap(),
    );
    settings.input_overrides.validate().unwrap();
}
