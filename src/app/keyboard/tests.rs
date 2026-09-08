use super::*;
use crate::app::tas_control::tests::harness::app_with_worker;
use crate::emu_backend::{EmuBackend, PceBackend};
use crate::emu_thread::EmuThread;
use crate::settings::{
    BindingAction, BindingExpression, BindingSet, BindingTarget, GameplayBindingSource, InputScope,
};

fn app_with_r_binding() -> App {
    let path = std::path::PathBuf::from("keyboard-test.pce");
    let mut rom = vec![0xEA; 0x2000];
    rom[..4].copy_from_slice(&[0xD4, 0xEA, 0x80, 0xFD]);
    rom[0x1FFE..].copy_from_slice(&0xE000_u16.to_le_bytes());
    let backend = EmuBackend::from_pce(PceBackend::new(rom, path.clone()).unwrap());
    let mut app = app_with_worker(EmuThread::spawn(backend, false), 1, ActiveSystem::Pce, path);
    app.settings
        .set_binding_set(
            &InputScope::Global,
            BindingTarget::Joypad {
                player: 1,
                action: BindingAction::A,
            },
            GameplayBindingSource::Keyboard,
            Some(BindingSet::new(BindingExpression::keyboard(KeyCode::KeyR))),
        )
        .unwrap();
    app.sync_effective_input();
    app
}

fn admit_key(app: &mut App, key: KeyCode) {
    app.observe_physical_keyboard_key(key, true);
    app.pressed_keyboard_targets.press_expression_key(key);
    app.refresh_keyboard_expressions();
}

fn release_key(app: &mut App, key: KeyCode) {
    app.observe_physical_keyboard_key(key, false);
    app.release_pressed_keyboard_key(key);
}

#[test]
fn consumed_modifier_release_cannot_reactivate_gameplay_until_key_is_released() {
    let mut app = app_with_r_binding();
    admit_key(&mut app, KeyCode::KeyR);
    assert_ne!(app.host_input.buttons_pressed(), 0);
    admit_key(&mut app, KeyCode::ControlLeft);
    assert_eq!(app.host_input.buttons_pressed(), 0);

    app.egui_wants_keyboard = true;
    release_key(&mut app, KeyCode::ControlLeft);
    assert_eq!(app.host_input.buttons_pressed(), 0);
    assert!(app.pressed_keyboard_targets.gameplay_blocked(KeyCode::KeyR));

    app.egui_wants_keyboard = false;
    app.refresh_keyboard_expressions();
    admit_key(&mut app, KeyCode::KeyR);
    assert_eq!(app.host_input.buttons_pressed(), 0);
    release_key(&mut app, KeyCode::KeyR);
    admit_key(&mut app, KeyCode::KeyR);
    assert_ne!(app.host_input.buttons_pressed(), 0);
}

#[test]
fn every_keyboard_ownership_gate_blocks_reactivation_on_modifier_release() {
    let mut app = app_with_r_binding();
    for gate in 0..3 {
        admit_key(&mut app, KeyCode::KeyR);
        admit_key(&mut app, KeyCode::ControlRight);
        match gate {
            0 => app.keyboard_capture_active = true,
            1 => app.game_view_focused = false,
            _ => app.game_window_focused = false,
        }
        release_key(&mut app, KeyCode::ControlRight);
        assert_eq!(app.host_input.buttons_pressed(), 0);
        assert!(app.pressed_keyboard_targets.gameplay_blocked(KeyCode::KeyR));
        app.keyboard_capture_active = false;
        app.game_view_focused = true;
        app.game_window_focused = true;
        app.refresh_keyboard_expressions();
        assert_eq!(app.host_input.buttons_pressed(), 0);
        release_key(&mut app, KeyCode::KeyR);
    }
}

#[test]
fn releasing_one_modifier_side_keeps_the_other_side_active() {
    let mut app = app_with_r_binding();
    for (left, right, index) in [
        (KeyCode::ControlLeft, KeyCode::ControlRight, 0),
        (KeyCode::AltLeft, KeyCode::AltRight, 1),
        (KeyCode::ShiftLeft, KeyCode::ShiftRight, 2),
    ] {
        for (first, last) in [(left, right), (right, left)] {
            app.observe_physical_keyboard_key(first, true);
            app.observe_physical_keyboard_key(last, true);
            app.observe_physical_keyboard_key(last, true);
            app.observe_physical_keyboard_key(first, false);
            assert!([app.modifiers.ctrl, app.modifiers.alt, app.modifiers.shift][index]);
            app.observe_physical_keyboard_key(last, false);
            assert!(![app.modifiers.ctrl, app.modifiers.alt, app.modifiers.shift][index]);
            assert!(
                app.debug_windows
                    .settings_ui
                    .binding_keyboard_down
                    .is_empty()
            );
        }
    }
}
