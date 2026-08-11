use crate::emu_backend::ActiveSystem;
use crate::input::HostButton;
use crate::settings::{TiltBindingAction, WonderSwanButton};

#[derive(Default)]
pub(super) struct HostInputState {
    keyboard_pressed: u16,
    gamepad_pressed: u16,
    remote_pressed: u16,
    keyboard_p2_pressed: u16,
    gamepad_p2_pressed: u16,
    remote_p2_pressed: u16,
    gamepad_stick_dpad_pressed: u8,
    tilt_keyboard_pressed: u8,
    ws_keyboard_x_pressed: u8,
    ws_keyboard_y_pressed: u8,
    ws_keyboard_button_pressed: u8,
    ws_gamepad_x_pressed: u8,
    ws_gamepad_y_pressed: u8,
    ws_gamepad_button_pressed: u8,
}

impl HostInputState {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn set_keyboard(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.keyboard_pressed, key, pressed);
    }

    pub(super) fn set_keyboard_p2(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.keyboard_p2_pressed, key, pressed);
    }

    pub(super) fn set_gamepad(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.gamepad_pressed, key, pressed);
    }

    pub(super) fn set_gamepad_p2(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.gamepad_p2_pressed, key, pressed);
    }

    pub(super) fn set_remote(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.remote_pressed, key, pressed);
    }

    pub(super) fn set_remote_p2(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.remote_p2_pressed, key, pressed);
    }

    pub(super) fn set_tilt_keyboard(&mut self, key: TiltBindingAction, pressed: bool) {
        let bit = match key {
            TiltBindingAction::Right => 1 << 0,
            TiltBindingAction::Left => 1 << 1,
            TiltBindingAction::Up => 1 << 2,
            TiltBindingAction::Down => 1 << 3,
        };
        if pressed {
            self.tilt_keyboard_pressed |= bit;
        } else {
            self.tilt_keyboard_pressed &= !bit;
        }
    }

    pub(super) fn set_ws_keyboard(&mut self, key: WonderSwanButton, pressed: bool) {
        Self::set_ws_button_masks(
            &mut self.ws_keyboard_x_pressed,
            &mut self.ws_keyboard_y_pressed,
            &mut self.ws_keyboard_button_pressed,
            key,
            pressed,
        );
    }

    pub(super) fn set_ws_gamepad(&mut self, key: WonderSwanButton, pressed: bool) {
        Self::set_ws_button_masks(
            &mut self.ws_gamepad_x_pressed,
            &mut self.ws_gamepad_y_pressed,
            &mut self.ws_gamepad_button_pressed,
            key,
            pressed,
        );
    }

    fn set_ws_button_masks(
        x_pressed: &mut u8,
        y_pressed: &mut u8,
        button_pressed: &mut u8,
        key: WonderSwanButton,
        pressed: bool,
    ) {
        let (mask, bit) = match key {
            WonderSwanButton::X1 => (x_pressed, 1 << 0),
            WonderSwanButton::X2 => (x_pressed, 1 << 1),
            WonderSwanButton::X3 => (x_pressed, 1 << 2),
            WonderSwanButton::X4 => (x_pressed, 1 << 3),
            WonderSwanButton::Y1 => (y_pressed, 1 << 0),
            WonderSwanButton::Y2 => (y_pressed, 1 << 1),
            WonderSwanButton::Y3 => (y_pressed, 1 << 2),
            WonderSwanButton::Y4 => (y_pressed, 1 << 3),
            WonderSwanButton::A => (button_pressed, 1 << 0),
            WonderSwanButton::B => (button_pressed, 1 << 1),
            WonderSwanButton::Start => (button_pressed, 1 << 3),
        };

        if pressed {
            *mask |= bit;
        } else {
            *mask &= !bit;
        }
    }

    pub(super) fn set_gamepad_stick_dpad(&mut self, left_stick: (f32, f32), deadzone: f32) {
        let (x, y) = left_stick;
        let ax = x.abs();
        let ay = y.abs();

        let mut use_x = ax >= deadzone;
        let mut use_y = ay >= deadzone;

        const CARDINAL_SNAP: f32 = 0.18; // ~tan(10deg)
        if use_x && use_y {
            if ay < ax * CARDINAL_SNAP {
                use_y = false;
            } else if ax < ay * CARDINAL_SNAP {
                use_x = false;
            }
        }

        let mut mask = 0u8;
        if use_x {
            if x >= deadzone {
                mask |= 1 << 0;
            }
            if x <= -deadzone {
                mask |= 1 << 1;
            }
        }
        if use_y {
            if y >= deadzone {
                mask |= 1 << 2;
            }
            if y <= -deadzone {
                mask |= 1 << 3;
            }
        }
        self.gamepad_stick_dpad_pressed = mask;
    }

    pub(super) fn clear_gamepad_stick_dpad(&mut self) {
        self.gamepad_stick_dpad_pressed = 0;
    }

    pub(super) fn tilt_vector(&self) -> (f32, f32) {
        let mut x = 0.0;
        let mut y = 0.0;
        if self.tilt_keyboard_pressed & (1 << 0) != 0 {
            x += 1.0;
        }
        if self.tilt_keyboard_pressed & (1 << 1) != 0 {
            x -= 1.0;
        }
        if self.tilt_keyboard_pressed & (1 << 2) != 0 {
            y += 1.0;
        }
        if self.tilt_keyboard_pressed & (1 << 3) != 0 {
            y -= 1.0;
        }
        (x, y)
    }

    pub(super) fn dpad_pressed(&self) -> u8 {
        ((self.keyboard_pressed
            | self.gamepad_pressed
            | self.remote_pressed
            | u16::from(self.gamepad_stick_dpad_pressed))
            & 0x0F) as u8
    }

    pub(super) fn buttons_pressed(&self) -> u8 {
        ((self.keyboard_pressed | self.gamepad_pressed | self.remote_pressed) >> 4) as u8
    }

    pub(super) fn dpad_p2_pressed(&self) -> u8 {
        ((self.keyboard_p2_pressed | self.gamepad_p2_pressed | self.remote_p2_pressed) & 0x0F) as u8
    }

    pub(super) fn buttons_p2_pressed(&self) -> u8 {
        ((self.keyboard_p2_pressed | self.gamepad_p2_pressed | self.remote_p2_pressed) >> 4) as u8
    }

    pub(super) fn ws_buttons_pressed(&self, display_rotated: bool) -> u8 {
        let mut y_buttons = (self.ws_keyboard_y_pressed | self.ws_gamepad_y_pressed) & 0x0F;
        if display_rotated {
            y_buttons |= host_dpad_to_ws_diamond(self.dpad_pressed());
        }

        self.ws_keyboard_button_pressed
            | self.ws_gamepad_button_pressed
            | (self.buttons_pressed() & 0x0F)
            | (y_buttons << 4)
    }

    pub(super) fn ws_dpad_pressed(&self, display_rotated: bool) -> u8 {
        let mut x_buttons = (self.ws_keyboard_x_pressed | self.ws_gamepad_x_pressed) & 0x0F;
        if !display_rotated {
            x_buttons |= host_dpad_to_ws_diamond(self.dpad_pressed());
        }
        x_buttons & 0x0F
    }

    fn set_mask_bit(mask: &mut u16, key: HostButton, pressed: bool) {
        let bit = key.host_mask_bit();
        if pressed {
            *mask |= bit;
        } else {
            *mask &= !bit;
        }
    }
}

fn host_dpad_to_ws_diamond(mask: u8) -> u8 {
    let mut ws = 0u8;
    if mask & (1 << 2) != 0 {
        ws |= 1 << 0;
    }
    if mask & (1 << 0) != 0 {
        ws |= 1 << 1;
    }
    if mask & (1 << 3) != 0 {
        ws |= 1 << 2;
    }
    if mask & (1 << 1) != 0 {
        ws |= 1 << 3;
    }
    ws
}

impl super::App {
    pub(super) fn current_host_joypad_input(&self) -> (u8, u8) {
        self.host_joypad_input_for_system(self.active_system)
    }

    pub(super) fn current_host_joypad_p2_input(&self) -> (u8, u8) {
        self.host_joypad_p2_input_for_system(self.active_system)
    }

    pub(super) fn host_joypad_input_for_system(&self, system: ActiveSystem) -> (u8, u8) {
        if system == ActiveSystem::WonderSwan {
            (
                self.host_input.ws_buttons_pressed(self.ws_display_rotated),
                self.host_input.ws_dpad_pressed(self.ws_display_rotated),
            )
        } else {
            (
                self.host_input.buttons_pressed(),
                self.host_input.dpad_pressed(),
            )
        }
    }

    pub(super) fn host_joypad_p2_input_for_system(&self, system: ActiveSystem) -> (u8, u8) {
        if system == ActiveSystem::WonderSwan {
            (0, 0)
        } else {
            (
                self.host_input.buttons_p2_pressed(),
                self.host_input.dpad_p2_pressed(),
            )
        }
    }
}

#[cfg(test)]
mod tests;
