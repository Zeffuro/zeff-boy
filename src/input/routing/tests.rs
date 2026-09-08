use super::*;
use crate::input::HostButton;
use crate::settings::{BindingAction, GamepadAction};

fn fingerprint(name: &str) -> GamepadFingerprint {
    GamepadFingerprint {
        name: name.into(),
        uuid: name.into(),
    }
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
