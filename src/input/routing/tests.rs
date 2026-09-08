use super::*;
use crate::input::HostButton;
use crate::settings::{BindingAction, GamepadAction};

fn fingerprint(name: &str) -> GamepadFingerprint {
    GamepadFingerprint {
        name: name.into(),
        uuid: name.into(),
    }
}

fn typed_target(player: u8, action: BindingAction) -> BindingTarget {
    BindingTarget::Joypad { player, action }
}

fn bind_typed(
    settings: &mut crate::settings::Settings,
    target: BindingTarget,
    bindings: BindingSet,
) {
    settings
        .set_binding_set(
            &crate::settings::InputScope::Global,
            target,
            GameplayBindingSource::Gamepad,
            Some(bindings),
        )
        .unwrap();
}

fn typed_axis(press: f32, release: f32) -> BindingExpression {
    let mut axis = crate::settings::AxisBinding::new(InputAxis::RightX, AxisDirection::Positive);
    axis.press_threshold = press;
    axis.release_threshold = release;
    BindingExpression::axis(axis)
}

fn resolve_typed(
    router: &mut GamepadRouter,
    settings: &crate::settings::Settings,
    capture: bool,
) -> EffectiveGamepads {
    let resolved = settings.resolve_gameplay_input(&crate::settings::InputScope::Global);
    router.configure_typed(&resolved);
    router.resolve(
        &settings.input_devices,
        &resolved.gamepad,
        capture,
        resolved.tilt.deadzone,
    )
}

fn typed_router(settings: &crate::settings::Settings, pads: usize) -> GamepadRouter {
    let mut router = GamepadRouter::default();
    resolve_typed(&mut router, settings, false);
    for index in 1..=pads {
        connect(&mut router, index as u64, "pad");
    }
    resolve_typed(&mut router, settings, false);
    router
}

#[test]
fn typed_button_alternatives_release_only_after_the_last_held_control() {
    let mut settings = crate::settings::Settings::default();
    let mut alternatives = BindingSet::new(BindingExpression::gamepad_button("South"));
    alternatives
        .add_expression(BindingExpression::gamepad_button("East"))
        .unwrap();
    bind_typed(
        &mut settings,
        typed_target(1, BindingAction::A),
        alternatives,
    );
    let mut router = typed_router(&settings, 1);
    let id = RuntimeGamepadId(1);
    let a = HostButton::A.host_mask_bit();
    router.button(id, "South", true);
    assert_ne!(
        resolve_typed(&mut router, &settings, false).players[0].buttons & a,
        0
    );
    router.button(id, "East", true);
    router.button(id, "South", false);
    assert_ne!(
        resolve_typed(&mut router, &settings, false).players[0].buttons & a,
        0
    );
    router.button(id, "East", false);
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
}

#[test]
fn typed_singleton_is_removed_from_legacy_first_match_routing() {
    let mut settings = crate::settings::Settings::default();
    settings
        .gamepad_bindings
        .set_for_player(BindingAction::B, 1, "South");
    let mut router = typed_router(&settings, 1);
    let id = RuntimeGamepadId(1);
    router.button(id, "South", true);
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
    bind_typed(
        &mut settings,
        typed_target(1, BindingAction::A),
        BindingSet::new(BindingExpression::gamepad_button("South")),
    );
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
    router.button(id, "South", false);
    resolve_typed(&mut router, &settings, false);
    router.button(id, "South", true);
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        HostButton::A.host_mask_bit() | HostButton::B.host_mask_bit()
    );
}

#[test]
fn typed_axis_chord_maintains_axis_state_when_its_button_is_released() {
    let mut settings = crate::settings::Settings::default();
    let chord = BindingExpression::chord(vec![
        BindingExpression::gamepad_button("South"),
        typed_axis(0.75, 0.25),
    ]);
    bind_typed(
        &mut settings,
        typed_target(1, BindingAction::A),
        BindingSet::new(chord),
    );
    let mut router = typed_router(&settings, 1);
    let id = RuntimeGamepadId(1);
    router.right_stick(id, (0.8, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
    router.button(id, "South", true);
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
    router.right_stick(id, (0.5, 0.0));
    router.button(id, "South", false);
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
    router.button(id, "South", true);
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
    router.right_stick(id, (0.25, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
    router.right_stick(id, (0.5, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
    router.right_stick(id, (0.75, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
}

#[test]
fn hysteresis_is_separate_for_alternatives_players_and_devices() {
    let mut settings = crate::settings::Settings::default();
    let mut alternatives = BindingSet::new(typed_axis(0.8, 0.2));
    alternatives.add_expression(typed_axis(0.5, 0.4)).unwrap();
    bind_typed(
        &mut settings,
        typed_target(1, BindingAction::A),
        alternatives.clone(),
    );
    bind_typed(
        &mut settings,
        typed_target(2, BindingAction::A),
        alternatives,
    );
    let mut router = typed_router(&settings, 2);
    router.right_stick(RuntimeGamepadId(1), (0.6, 0.0));
    let first = resolve_typed(&mut router, &settings, false);
    assert_eq!(first.players[0].buttons, HostButton::A.host_mask_bit());
    assert_eq!(first.players[1].buttons, 0);
    router.right_stick(RuntimeGamepadId(1), (0.3, 0.0));
    router.right_stick(RuntimeGamepadId(2), (0.3, 0.0));
    let next = resolve_typed(&mut router, &settings, false);
    assert_eq!(next.players[0].buttons, 0);
    assert_eq!(next.players[1].buttons, 0);
    router.right_stick(RuntimeGamepadId(1), (0.9, 0.0));
    resolve_typed(&mut router, &settings, false);
    router.right_stick(RuntimeGamepadId(1), (0.3, 0.0));
    let held = resolve_typed(&mut router, &settings, false);
    assert_eq!(held.players[0].buttons, HostButton::A.host_mask_bit());
    assert_eq!(held.players[1].buttons, 0);
}

#[test]
fn rebind_below_global_deadzone_still_requires_axis_release_threshold() {
    let mut settings = crate::settings::Settings::default();
    settings.tilt.deadzone = 0.3;
    bind_typed(
        &mut settings,
        typed_target(1, BindingAction::A),
        BindingSet::new(typed_axis(0.1, 0.05)),
    );
    let mut router = typed_router(&settings, 1);
    let id = RuntimeGamepadId(1);
    router.right_stick(id, (0.2, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
    bind_typed(
        &mut settings,
        typed_target(1, BindingAction::A),
        BindingSet::new(typed_axis(0.15, 0.05)),
    );
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
    assert!(router.snapshot.devices[0].waiting_for_neutral);
    router.right_stick(id, (0.04, 0.0));
    resolve_typed(&mut router, &settings, false);
    assert!(!router.snapshot.devices[0].waiting_for_neutral);
    router.right_stick(id, (0.2, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
}

#[test]
fn capture_neutrality_includes_explicit_right_axes_only_for_their_player() {
    let mut settings = crate::settings::Settings::default();
    bind_typed(
        &mut settings,
        typed_target(1, BindingAction::A),
        BindingSet::new(typed_axis(0.75, 0.25)),
    );
    let mut router = typed_router(&settings, 2);
    let id = RuntimeGamepadId(1);
    router.right_stick(id, (0.8, 0.0));
    router.right_stick(RuntimeGamepadId(2), (0.9, 0.9));
    assert_eq!(
        resolve_typed(&mut router, &settings, true),
        EffectiveGamepads::default()
    );
    assert!(!router.snapshot.capture_ready);
    router.right_stick(id, (0.0, 0.0));
    resolve_typed(&mut router, &settings, true);
    assert!(router.snapshot.capture_ready);
    router.right_stick(id, (0.8, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
    router.right_stick(id, (0.0, 0.0));
    resolve_typed(&mut router, &settings, false);
    router.right_stick(id, (0.8, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
}

#[test]
fn disconnect_and_reassignment_cannot_transfer_axis_latches() {
    let mut settings = crate::settings::Settings::default();
    for player in [1, 2] {
        bind_typed(
            &mut settings,
            typed_target(player, BindingAction::A),
            BindingSet::new(typed_axis(0.75, 0.25)),
        );
    }
    let mut router = typed_router(&settings, 1);
    router.right_stick(RuntimeGamepadId(1), (0.8, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
    router.disconnect(RuntimeGamepadId(1));
    assert_eq!(
        resolve_typed(&mut router, &settings, false),
        EffectiveGamepads::default()
    );
    connect(&mut router, 2, "pad");
    router.right_stick(RuntimeGamepadId(2), (0.5, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
    router.right_stick(RuntimeGamepadId(2), (0.8, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
    settings.input_devices.players[0] = GamepadAssignment::Disabled;
    let moved = resolve_typed(&mut router, &settings, false);
    assert_eq!(moved.players[0].buttons, 0);
    assert_eq!(moved.players[1].buttons, 0);
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[1].buttons,
        0
    );
    router.right_stick(RuntimeGamepadId(2), (0.0, 0.0));
    resolve_typed(&mut router, &settings, false);
    router.right_stick(RuntimeGamepadId(2), (0.8, 0.0));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[1].buttons,
        HostButton::A.host_mask_bit()
    );
}

#[test]
fn typed_inputs_preserve_quick_frontend_pause_edges() {
    let mut settings = crate::settings::Settings::default();
    settings
        .gamepad_bindings
        .set_action(GamepadAction::Pause, "South");
    bind_typed(
        &mut settings,
        typed_target(1, BindingAction::A),
        BindingSet::new(BindingExpression::gamepad_button("South")),
    );
    let mut router = typed_router(&settings, 1);
    router.button(RuntimeGamepadId(1), "South", true);
    router.button(RuntimeGamepadId(1), "South", false);
    assert_eq!(
        resolve_typed(&mut router, &settings, false).players[0].buttons,
        0
    );
    assert_eq!(router.take_pause_presses(), 1);
    assert_eq!(router.take_pause_presses(), 0);
}

#[test]
fn wonderswan_typed_right_axis_uses_only_player_one_and_keeps_raw_diagnostics() {
    let mut settings = crate::settings::Settings::default();
    let axis = crate::settings::AxisBinding::new(InputAxis::RightY, AxisDirection::Negative);
    bind_typed(
        &mut settings,
        BindingTarget::WonderSwan(crate::settings::WonderSwanButton::X1),
        BindingSet::new(BindingExpression::axis(axis)),
    );
    let mut router = typed_router(&settings, 2);
    router.right_stick(RuntimeGamepadId(2), (0.0, -0.8));
    assert_eq!(resolve_typed(&mut router, &settings, false).ws, 0);
    router.right_stick(RuntimeGamepadId(1), (0.0, -0.8));
    assert_eq!(
        resolve_typed(&mut router, &settings, false).ws,
        ws_bit(crate::settings::WonderSwanButton::X1)
    );
    assert_eq!(router.snapshot.devices[0].right_stick, (0.0, -0.8));
}

#[test]
fn action_rebind_preserves_held_gameplay_and_waits_for_a_new_press() {
    let mut router = GamepadRouter::default();
    let mut bindings = GamepadBindings::default();
    let preferences = InputDeviceSettings::default();
    connect(&mut router, 1, "pad");
    router.resolve(&preferences, &bindings, false, 0.3);
    router.button(RuntimeGamepadId(1), "South", true);
    let before = router.resolve(&preferences, &bindings, false, 0.3);
    assert_ne!(before.players[0].buttons, 0);
    bindings.set_action(GamepadAction::Pause, "South");
    let after = router.resolve(&preferences, &bindings, false, 0.3);
    assert_eq!(after.players[0], before.players[0]);
    assert_eq!(after.actions, 0);
    assert_eq!(router.take_pause_presses(), 0);
    router.resolve(&preferences, &bindings, false, 0.3);
    assert_eq!(router.take_pause_presses(), 0);
    router.button(RuntimeGamepadId(1), "South", false);
    router.resolve(&preferences, &bindings, false, 0.3);
    router.button(RuntimeGamepadId(1), "South", true);
    assert_eq!(router.take_pause_presses(), 1);
    assert_eq!(
        router.resolve(&preferences, &bindings, false, 0.3).players[0],
        before.players[0]
    );
}

#[test]
fn explicit_barrier_releases_unchanged_mapping_until_physical_neutral() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "pad");
    resolve(&mut router);
    router.button(RuntimeGamepadId(1), "South", true);
    assert_ne!(resolve(&mut router).players[0].buttons, 0);
    router.apply_command(GamepadCommand::Neutralize);
    assert_eq!(resolve(&mut router).players[0].buttons, 0);
    assert_eq!(resolve(&mut router).players[0].buttons, 0);
    router.button(RuntimeGamepadId(1), "South", false);
    resolve(&mut router);
    router.button(RuntimeGamepadId(1), "South", true);
    assert_ne!(resolve(&mut router).players[0].buttons, 0);
}

#[test]
fn right_stick_diagnostics_do_not_change_gameplay_or_capture_readiness() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "first");
    connect(&mut router, 2, "second");
    resolve(&mut router);
    router.right_stick(RuntimeGamepadId(1), (0.8, -0.6));
    router.right_stick(RuntimeGamepadId(2), (-0.4, 0.2));
    router.stick(RuntimeGamepadId(1), (0.5, 0.0));
    router.button(RuntimeGamepadId(1), "South", true);
    let effective = resolve(&mut router);
    assert_eq!(effective.players[0].stick, (0.5, 0.0));
    assert_eq!(effective.players[0].buttons, HostButton::A.host_mask_bit());
    assert_eq!(effective.players[1], EffectivePlayer::default());
    assert_eq!(router.snapshot.devices[0].right_stick, (0.8, -0.6));
    assert_eq!(router.snapshot.devices[1].right_stick, (-0.4, 0.2));

    router.stick(RuntimeGamepadId(1), (0.0, 0.0));
    router.button(RuntimeGamepadId(1), "South", false);
    let captured = router.resolve(
        &InputDeviceSettings::default(),
        &GamepadBindings::default(),
        true,
        0.3,
    );
    assert!(router.capture_accepts_press());
    assert_eq!(captured, EffectiveGamepads::default());
    assert_eq!(router.snapshot.devices[0].right_stick, (0.8, -0.6));
    router.disconnect(RuntimeGamepadId(1));
    resolve(&mut router);
    assert_eq!(router.snapshot.devices.len(), 1);
    assert_eq!(router.snapshot.devices[0].right_stick, (-0.4, 0.2));
}

#[test]
fn right_stick_diagnostics_normalize_invalid_axes() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "pad");
    router.right_stick(RuntimeGamepadId(1), (f32::NAN, f32::INFINITY));
    resolve(&mut router);
    assert_eq!(router.snapshot.devices[0].right_stick, (0.0, 0.0));
    router.right_stick(RuntimeGamepadId(1), (-2.0, 2.0));
    resolve(&mut router);
    assert_eq!(router.snapshot.devices[0].right_stick, (-1.0, 1.0));
}

#[test]
fn model_calibration_preserves_raw_diagnostics_and_maps_both_sticks() {
    let mut router = GamepadRouter::default();
    let mut preferences = InputDeviceSettings::default();
    let fingerprint = fingerprint("pad");
    preferences.set_calibration(
        fingerprint.clone(),
        crate::settings::GamepadCalibration {
            left: crate::settings::StickCalibration {
                x: crate::settings::AxisCalibration {
                    min: -0.8,
                    center: 0.2,
                    max: 0.8,
                },
                y: Default::default(),
            },
            right: crate::settings::StickCalibration {
                x: Default::default(),
                y: crate::settings::AxisCalibration {
                    min: -0.9,
                    center: -0.2,
                    max: 0.6,
                },
            },
        },
    );
    router.connect(RuntimeGamepadId(1), fingerprint);
    // A preference transition establishes a calibrated-neutral rearm barrier.
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    router.stick(RuntimeGamepadId(1), (0.5, -0.4));
    router.right_stick(RuntimeGamepadId(1), (0.25, 0.2));

    let effective = router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(effective.players[0].stick, (0.5, -0.4));
    let device = &router.snapshot.devices[0];
    assert_eq!(device.left_stick, (0.5, -0.4));
    assert_eq!(device.calibrated_left_stick, (0.5, -0.4));
    assert_eq!(device.right_stick, (0.25, 0.2));
    assert_eq!(device.calibrated_right_stick, (0.25, 0.5));
    assert!(router.snapshot.sample_generation > 0);
}

#[test]
fn invalid_model_calibration_falls_back_to_normalized_raw_input() {
    let mut router = GamepadRouter::default();
    let mut preferences = InputDeviceSettings::default();
    let fingerprint = fingerprint("pad");
    preferences.set_calibration(
        fingerprint.clone(),
        crate::settings::GamepadCalibration {
            left: crate::settings::StickCalibration {
                x: crate::settings::AxisCalibration {
                    min: -1.0,
                    center: -0.99,
                    max: 1.0,
                },
                ..Default::default()
            },
            ..Default::default()
        },
    );
    router.connect(RuntimeGamepadId(1), fingerprint);
    router.stick(RuntimeGamepadId(1), (0.4, 0.0));
    assert_eq!(
        router
            .resolve(&preferences, &GamepadBindings::default(), false, 0.3)
            .players[0]
            .stick,
        (0.4, 0.0)
    );
}

#[test]
fn calibration_change_requires_calibrated_neutral_before_rearming() {
    let mut router = GamepadRouter::default();
    let fingerprint = fingerprint("pad");
    let mut preferences = InputDeviceSettings::default();
    router.connect(RuntimeGamepadId(1), fingerprint.clone());
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    router.stick(RuntimeGamepadId(1), (0.4, 0.0));
    assert_eq!(
        router
            .resolve(&preferences, &GamepadBindings::default(), false, 0.3)
            .players[0]
            .stick,
        (0.4, 0.0)
    );

    preferences.set_calibration(
        fingerprint,
        crate::settings::GamepadCalibration {
            left: crate::settings::StickCalibration {
                x: crate::settings::AxisCalibration {
                    min: -0.8,
                    center: 0.4,
                    max: 0.8,
                },
                ..Default::default()
            },
            ..Default::default()
        },
    );
    assert_eq!(
        router.resolve(&preferences, &GamepadBindings::default(), false, 0.3),
        EffectiveGamepads::default()
    );
    assert_eq!(
        router.resolve(&preferences, &GamepadBindings::default(), false, 0.3),
        EffectiveGamepads::default()
    );
    assert_eq!(
        router
            .resolve(&preferences, &GamepadBindings::default(), false, 0.3)
            .players[0]
            .stick,
        (0.0, 0.0)
    );

    router.resolve(&preferences, &GamepadBindings::default(), true, 0.3);
    assert!(router.capture_accepts_press());
}

#[test]
fn duplicate_reservations_require_identification_after_staggered_reversed_reconnect() {
    let mut router = GamepadRouter::default();
    let mut preferences = InputDeviceSettings::default();
    preferences.players[0] = GamepadAssignment::Reserved(fingerprint("same"));
    preferences.players[1] = GamepadAssignment::Reserved(fingerprint("same"));
    connect(&mut router, 1, "same");
    connect(&mut router, 2, "same");
    router.apply_command(GamepadCommand::Identify {
        player: 1,
        device: RuntimeGamepadId(1),
    });
    router.apply_command(GamepadCommand::Identify {
        player: 2,
        device: RuntimeGamepadId(2),
    });
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(
        router.assigned[..2],
        [Some(RuntimeGamepadId(1)), Some(RuntimeGamepadId(2))]
    );
    router.disconnect(RuntimeGamepadId(1));
    router.disconnect(RuntimeGamepadId(2));
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    // The former P2 returns first under a new connection ID. It must not become P1.
    connect(&mut router, 3, "same");
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(router.assigned[..2], [None, None]);
    assert_eq!(
        router.snapshot.players[0].status,
        GamepadAssignmentStatus::Ambiguous
    );
    connect(&mut router, 4, "same");
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(router.assigned[..2], [None, None]);
    router.apply_command(GamepadCommand::Identify {
        player: 1,
        device: RuntimeGamepadId(4),
    });
    router.apply_command(GamepadCommand::Identify {
        player: 2,
        device: RuntimeGamepadId(3),
    });
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(
        router.assigned[..2],
        [Some(RuntimeGamepadId(4)), Some(RuntimeGamepadId(3))]
    );
}

#[test]
fn capture_waits_for_preexisting_buttons_to_release_before_accepting_new_candidate() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "pad");
    resolve(&mut router);
    router.button(RuntimeGamepadId(1), "South", true);
    router.resolve(
        &InputDeviceSettings::default(),
        &GamepadBindings::default(),
        true,
        0.3,
    );
    assert!(!router.capture_accepts_press());
    router.button(RuntimeGamepadId(1), "East", true);
    assert!(!router.capture_accepts_press());
    router.button(RuntimeGamepadId(1), "South", false);
    assert!(!router.capture_accepts_press());
    router.button(RuntimeGamepadId(1), "East", false);
    assert!(router.capture_accepts_press());
    assert!(router.button(RuntimeGamepadId(1), "East", true));
    assert_eq!(
        router.resolve(
            &InputDeviceSettings::default(),
            &GamepadBindings::default(),
            true,
            0.3
        ),
        EffectiveGamepads::default()
    );
}

#[test]
fn pause_tap_survives_one_poll_but_capture_and_rearm_cannot_trigger_it() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "pad");
    let mut bindings = GamepadBindings::default();
    bindings.set_action(GamepadAction::Pause, "Mode");
    let preferences = InputDeviceSettings::default();
    router.resolve(&preferences, &bindings, false, 0.3);
    router.button(RuntimeGamepadId(1), "Mode", true);
    router.button(RuntimeGamepadId(1), "Mode", false);
    assert_eq!(
        router.resolve(&preferences, &bindings, false, 0.3).actions,
        0
    );
    assert_eq!(router.take_pause_presses(), 1);
    assert_eq!(router.take_pause_presses(), 0);
    router.resolve(&preferences, &bindings, true, 0.3);
    router.button(RuntimeGamepadId(1), "Mode", true);
    router.button(RuntimeGamepadId(1), "Mode", false);
    router.resolve(&preferences, &bindings, true, 0.3);
    assert_eq!(router.take_pause_presses(), 0);
    // Releasing the capture and pressing before a neutral rearm poll stays suppressed.
    router.button(RuntimeGamepadId(1), "Mode", true);
    router.resolve(&preferences, &bindings, false, 0.3);
    router.button(RuntimeGamepadId(1), "Mode", false);
    assert_eq!(router.take_pause_presses(), 0);
    router.resolve(&preferences, &bindings, false, 0.3);
    router.button(RuntimeGamepadId(1), "Mode", true);
    router.button(RuntimeGamepadId(1), "Mode", false);
    router.resolve(&preferences, &bindings, false, 0.3);
    assert_eq!(router.take_pause_presses(), 1);
}

fn connect(router: &mut GamepadRouter, id: u64, name: &str) {
    router.connect(RuntimeGamepadId(id), fingerprint(name));
}

fn resolve(router: &mut GamepadRouter) -> EffectiveGamepads {
    router.resolve(
        &InputDeviceSettings::default(),
        &GamepadBindings::default(),
        false,
        0.3,
    )
}

#[test]
fn extra_device_never_falls_back_to_player_one() {
    let mut router = GamepadRouter::default();
    for id in 1..=6 {
        connect(&mut router, id, &format!("pad{id}"));
    }
    resolve(&mut router);
    router.button(RuntimeGamepadId(1), "South", true);
    router.button(RuntimeGamepadId(6), "East", true);
    let effective = resolve(&mut router);
    assert_eq!(effective.players[0].buttons, HostButton::A.host_mask_bit());
    router.button(RuntimeGamepadId(6), "South", true);
    router.button(RuntimeGamepadId(6), "South", false);
    assert_eq!(
        resolve(&mut router).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
    assert!(
        router
            .snapshot
            .players
            .iter()
            .all(|player| player.device != Some(RuntimeGamepadId(6)))
    );
}

#[test]
fn disconnect_releases_buttons_actions_ws_and_stick_without_touching_other_seats() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "one");
    connect(&mut router, 2, "two");
    let mut bindings = GamepadBindings::default();
    bindings.set_action(GamepadAction::SpeedUp, "South");
    router.resolve(&InputDeviceSettings::default(), &bindings, false, 0.3);
    router.button(RuntimeGamepadId(1), "South", true);
    router.button(RuntimeGamepadId(2), "East", true);
    router.stick(RuntimeGamepadId(1), (0.8, -0.7));
    let held = router.resolve(&InputDeviceSettings::default(), &bindings, false, 0.3);
    assert_ne!(held.actions, 0);
    assert_ne!(held.ws, 0);
    router.disconnect(RuntimeGamepadId(1));
    let released = router.resolve(&InputDeviceSettings::default(), &bindings, false, 0.3);
    assert_eq!(released.players[0], EffectivePlayer::default());
    assert_eq!(released.actions, 0);
    assert_eq!(released.ws, 0);
    assert_eq!(released.players[1].buttons, HostButton::B.host_mask_bit());
    assert_eq!(router.assigned_p1(), None);
}

#[test]
fn reserved_device_ignores_connection_order_and_auto_does_not_steal_it() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "other");
    connect(&mut router, 2, "reserved");
    let mut preferences = InputDeviceSettings::default();
    preferences.players[1] = GamepadAssignment::Reserved(fingerprint("reserved"));
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(router.assigned[0], Some(RuntimeGamepadId(1)));
    assert_eq!(router.assigned[1], Some(RuntimeGamepadId(2)));
    router.disconnect(RuntimeGamepadId(2));
    connect(&mut router, 20, "reserved");
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(router.assigned[1], Some(RuntimeGamepadId(20)));
}

#[test]
fn identical_models_are_ambiguous_until_identified_and_reconnect_does_not_reuse_identity() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "same");
    connect(&mut router, 2, "same");
    let mut preferences = InputDeviceSettings::default();
    preferences.players[0] = GamepadAssignment::Reserved(fingerprint("same"));
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(
        router.snapshot.players[0].status,
        GamepadAssignmentStatus::Ambiguous
    );
    assert!(router.assigned.iter().all(Option::is_none));
    router.apply_command(GamepadCommand::Identify {
        player: 1,
        device: RuntimeGamepadId(2),
    });
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(router.assigned_p1(), Some(RuntimeGamepadId(2)));
    router.disconnect(RuntimeGamepadId(2));
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(
        router.snapshot.players[0].status,
        GamepadAssignmentStatus::Ambiguous,
        "the other identical pad must not inherit the disconnected player's reservation"
    );
    assert_eq!(router.assigned_p1(), None);
    connect(&mut router, 3, "same");
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(
        router.snapshot.players[0].status,
        GamepadAssignmentStatus::Ambiguous
    );
    router.apply_command(GamepadCommand::Identify {
        player: 1,
        device: RuntimeGamepadId(2),
    });
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert_eq!(router.assigned_p1(), None);
}

#[test]
fn reassignment_releases_old_seat_and_waits_for_neutral_before_new_seat() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "pad");
    resolve(&mut router);
    router.button(RuntimeGamepadId(1), "South", true);
    assert_ne!(resolve(&mut router).players[0].buttons, 0);
    let mut preferences = InputDeviceSettings::default();
    preferences.players[0] = GamepadAssignment::Disabled;
    let effective = router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    assert!(effective.players.iter().all(|player| player.buttons == 0));
    assert_eq!(router.assigned[1], Some(RuntimeGamepadId(1)));
    router.button(RuntimeGamepadId(1), "South", false);
    router.resolve(&preferences, &GamepadBindings::default(), false, 0.3);
    router.button(RuntimeGamepadId(1), "South", true);
    assert_eq!(
        router
            .resolve(&preferences, &GamepadBindings::default(), false, 0.3)
            .players[1]
            .buttons,
        HostButton::A.host_mask_bit()
    );
}

#[test]
fn capture_processes_releases_and_blocks_buttons_and_axes_until_neutral() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "pad");
    resolve(&mut router);
    router.button(RuntimeGamepadId(1), "South", true);
    assert_ne!(resolve(&mut router).players[0].buttons, 0);
    let captured = router.resolve(
        &InputDeviceSettings::default(),
        &GamepadBindings::default(),
        true,
        0.3,
    );
    assert_eq!(captured, EffectiveGamepads::default());
    router.button(RuntimeGamepadId(1), "South", false);
    router.stick(RuntimeGamepadId(1), (0.9, 0.0));
    assert!(router.button(RuntimeGamepadId(1), "East", true));
    assert!(!router.button(RuntimeGamepadId(1), "East", true));
    assert_eq!(resolve(&mut router), EffectiveGamepads::default());
    router.button(RuntimeGamepadId(1), "East", false);
    assert_eq!(resolve(&mut router), EffectiveGamepads::default());
    router.stick(RuntimeGamepadId(1), (0.0, 0.0));
    resolve(&mut router);
    router.button(RuntimeGamepadId(1), "East", true);
    assert_eq!(
        resolve(&mut router).players[0].buttons,
        HostButton::B.host_mask_bit()
    );
}

#[test]
fn binding_change_while_held_releases_old_target_and_does_not_press_new_target() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "pad");
    resolve(&mut router);
    router.button(RuntimeGamepadId(1), "South", true);
    assert_eq!(
        resolve(&mut router).players[0].buttons,
        HostButton::A.host_mask_bit()
    );
    let mut bindings = GamepadBindings::default();
    bindings.set(BindingAction::A, "West");
    bindings.set(BindingAction::B, "South");
    assert_eq!(
        router.resolve(&InputDeviceSettings::default(), &bindings, false, 0.3),
        EffectiveGamepads::default()
    );
    router.button(RuntimeGamepadId(1), "South", false);
    router.resolve(&InputDeviceSettings::default(), &bindings, false, 0.3);
    router.button(RuntimeGamepadId(1), "South", true);
    assert_eq!(
        router
            .resolve(&InputDeviceSettings::default(), &bindings, false, 0.3)
            .players[0]
            .buttons,
        HostButton::B.host_mask_bit()
    );
}

#[test]
fn duplicate_legacy_bindings_keep_first_match_and_axes_are_per_device() {
    let mut router = GamepadRouter::default();
    connect(&mut router, 1, "one");
    connect(&mut router, 2, "two");
    let mut bindings = GamepadBindings::default();
    bindings.set(BindingAction::Up, "South");
    router.resolve(&InputDeviceSettings::default(), &bindings, false, 0.3);
    router.button(RuntimeGamepadId(1), "South", true);
    router.stick(RuntimeGamepadId(1), (0.5, 0.0));
    router.stick(RuntimeGamepadId(2), (f32::NAN, -2.0));
    let effective = router.resolve(&InputDeviceSettings::default(), &bindings, false, 0.3);
    assert_eq!(effective.players[0].buttons, HostButton::A.host_mask_bit());
    assert_eq!(effective.players[0].stick, (0.5, 0.0));
    assert_eq!(effective.players[1].stick, (0.0, -1.0));
}
