use super::*;
use crate::settings::WonderSwanButton;
use zeff_gb_core::hardware::joypad::JoypadKey;

#[test]
fn keyboard_a_button_in_buttons_pressed() {
    let mut state = HostInputState::new();
    state.set_keyboard(JoypadKey::A, true);
    assert_eq!(state.buttons_pressed(), 0x01);
    assert_eq!(state.dpad_pressed(), 0x00);
}

#[test]
fn keyboard_dpad_right_in_dpad_pressed() {
    let mut state = HostInputState::new();
    state.set_keyboard(JoypadKey::Right, true);
    assert_eq!(state.dpad_pressed(), 0x01);
    assert_eq!(state.buttons_pressed(), 0x00);
}

#[test]
fn keyboard_and_gamepad_merge_via_or() {
    let mut state = HostInputState::new();
    state.set_keyboard(JoypadKey::A, true);
    state.set_gamepad(JoypadKey::B, true);
    assert_eq!(state.buttons_pressed(), 0x03);
}

#[test]
fn remote_input_merges_with_keyboard_and_gamepad() {
    let mut state = HostInputState::new();
    state.set_keyboard(JoypadKey::A, true);
    state.set_gamepad(JoypadKey::B, true);
    state.set_remote(JoypadKey::Start, true);
    state.set_remote(JoypadKey::Right, true);
    assert_eq!(state.buttons_pressed(), 0x0B);
    assert_eq!(state.dpad_pressed(), 0x01);
}

#[test]
fn release_clears_bit() {
    let mut state = HostInputState::new();
    state.set_keyboard(JoypadKey::A, true);
    assert_eq!(state.buttons_pressed(), 0x01);
    state.set_keyboard(JoypadKey::A, false);
    assert_eq!(state.buttons_pressed(), 0x00);
}

#[test]
fn all_buttons_and_dpad() {
    let mut state = HostInputState::new();
    state.set_keyboard(JoypadKey::A, true);
    state.set_keyboard(JoypadKey::B, true);
    state.set_keyboard(JoypadKey::Select, true);
    state.set_keyboard(JoypadKey::Start, true);
    state.set_keyboard(JoypadKey::Up, true);
    state.set_keyboard(JoypadKey::Down, true);
    state.set_keyboard(JoypadKey::Left, true);
    state.set_keyboard(JoypadKey::Right, true);
    assert_eq!(state.buttons_pressed(), 0x0F);
    assert_eq!(state.dpad_pressed(), 0x0F);
}

#[test]
fn wonderswan_keyboard_packs_exact_x_y_and_button_rows() {
    let mut state = HostInputState::new();
    state.set_ws_keyboard(WonderSwanButton::X1, true);
    state.set_ws_keyboard(WonderSwanButton::X4, true);
    state.set_ws_keyboard(WonderSwanButton::Y2, true);
    state.set_ws_keyboard(WonderSwanButton::Y3, true);
    state.set_ws_keyboard(WonderSwanButton::A, true);
    state.set_ws_keyboard(WonderSwanButton::B, true);
    state.set_ws_keyboard(WonderSwanButton::Start, true);

    assert_eq!(state.ws_dpad_pressed(false), 0b1001);
    assert_eq!(state.ws_buttons_pressed(false), 0b0110_1011);
}

#[test]
fn wonderswan_direct_gamepad_packs_separately_from_keyboard() {
    let mut state = HostInputState::new();
    state.set_ws_keyboard(WonderSwanButton::X1, true);
    state.set_ws_gamepad(WonderSwanButton::X4, true);
    state.set_ws_gamepad(WonderSwanButton::Y2, true);
    state.set_ws_gamepad(WonderSwanButton::A, true);

    assert_eq!(state.ws_dpad_pressed(false), 0b1001);
    assert_eq!(state.ws_buttons_pressed(false), 0b0010_0001);

    state.set_ws_gamepad(WonderSwanButton::X4, false);

    assert_eq!(
        state.ws_dpad_pressed(false) & 0b0001,
        0b0001,
        "keyboard-held X1 must survive gamepad X4 release"
    );
    assert_eq!(state.ws_dpad_pressed(false) & 0b1000, 0);
}

#[test]
fn wonderswan_generic_dpad_follows_x_buttons_when_horizontal() {
    let mut state = HostInputState::new();
    state.set_gamepad(JoypadKey::Up, true);
    state.set_gamepad(JoypadKey::Right, true);

    assert_eq!(state.ws_dpad_pressed(false), 0b0011);
    assert_eq!(state.ws_buttons_pressed(false) >> 4, 0);
}

#[test]
fn wonderswan_generic_dpad_follows_y_buttons_when_rotated() {
    let mut state = HostInputState::new();
    state.set_gamepad(JoypadKey::Down, true);
    state.set_gamepad(JoypadKey::Left, true);

    assert_eq!(state.ws_dpad_pressed(true), 0);
    assert_eq!(state.ws_buttons_pressed(true) >> 4, 0b1100);
}

#[test]
fn stick_dpad_right() {
    let mut state = HostInputState::new();
    state.set_gamepad_stick_dpad((0.8, 0.0), 0.3);
    assert_eq!(state.dpad_pressed(), 0x01);
}

#[test]
fn stick_dpad_left() {
    let mut state = HostInputState::new();
    state.set_gamepad_stick_dpad((-0.8, 0.0), 0.3);
    assert_eq!(state.dpad_pressed(), 0x02);
}

#[test]
fn stick_dpad_up() {
    let mut state = HostInputState::new();
    state.set_gamepad_stick_dpad((0.0, 0.8), 0.3);
    assert_eq!(state.dpad_pressed(), 0x04);
}

#[test]
fn stick_dpad_down() {
    let mut state = HostInputState::new();
    state.set_gamepad_stick_dpad((0.0, -0.8), 0.3);
    assert_eq!(state.dpad_pressed(), 0x08);
}

#[test]
fn stick_dpad_below_deadzone_is_zero() {
    let mut state = HostInputState::new();
    state.set_gamepad_stick_dpad((0.1, 0.1), 0.3);
    assert_eq!(state.dpad_pressed(), 0x00);
}

#[test]
fn stick_dpad_diagonal() {
    let mut state = HostInputState::new();
    state.set_gamepad_stick_dpad((0.7, 0.7), 0.3);
    let d = state.dpad_pressed();
    assert!(d & 0x01 != 0, "Right expected");
    assert!(d & 0x04 != 0, "Up expected");
}

#[test]
fn stick_dpad_cardinal_snap_suppresses_minor_axis() {
    let mut state = HostInputState::new();
    state.set_gamepad_stick_dpad((0.9, 0.05), 0.3);
    let d = state.dpad_pressed();
    assert!(d & 0x01 != 0, "Right expected");
    assert_eq!(d & 0x04, 0, "Up should be snapped out");
}

#[test]
fn stick_dpad_merges_with_keyboard() {
    let mut state = HostInputState::new();
    state.set_keyboard(JoypadKey::Up, true);
    state.set_gamepad_stick_dpad((0.8, 0.0), 0.3);
    let d = state.dpad_pressed();
    assert!(d & 0x01 != 0, "Right from stick");
    assert!(d & 0x04 != 0, "Up from keyboard");
}

#[test]
fn clear_stick_dpad() {
    let mut state = HostInputState::new();
    state.set_gamepad_stick_dpad((0.8, 0.0), 0.3);
    assert_ne!(state.dpad_pressed(), 0);
    state.clear_gamepad_stick_dpad();
    assert_eq!(state.dpad_pressed(), 0);
}

#[test]
fn tilt_vector_all_directions() {
    let mut state = HostInputState::new();
    state.set_tilt_keyboard(TiltBindingAction::Right, true);
    assert_eq!(state.tilt_vector(), (1.0, 0.0));

    state.set_tilt_keyboard(TiltBindingAction::Right, false);
    state.set_tilt_keyboard(TiltBindingAction::Left, true);
    assert_eq!(state.tilt_vector(), (-1.0, 0.0));

    state.set_tilt_keyboard(TiltBindingAction::Left, false);
    state.set_tilt_keyboard(TiltBindingAction::Up, true);
    assert_eq!(state.tilt_vector(), (0.0, 1.0));

    state.set_tilt_keyboard(TiltBindingAction::Up, false);
    state.set_tilt_keyboard(TiltBindingAction::Down, true);
    assert_eq!(state.tilt_vector(), (0.0, -1.0));
}

#[test]
fn tilt_vector_opposing_cancel() {
    let mut state = HostInputState::new();
    state.set_tilt_keyboard(TiltBindingAction::Right, true);
    state.set_tilt_keyboard(TiltBindingAction::Left, true);
    assert_eq!(state.tilt_vector(), (0.0, 0.0));
}

#[test]
fn tilt_vector_diagonal() {
    let mut state = HostInputState::new();
    state.set_tilt_keyboard(TiltBindingAction::Right, true);
    state.set_tilt_keyboard(TiltBindingAction::Up, true);
    assert_eq!(state.tilt_vector(), (1.0, 1.0));
}
