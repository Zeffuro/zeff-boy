use crate::hardware::joypad::JoypadKey;
use crate::settings::TiltBindingAction;

#[derive(Default)]
pub(super) struct HostInputState {
    keyboard_pressed: u8,
    gamepad_pressed: u8,
    gamepad_stick_dpad_pressed: u8,
    tilt_keyboard_pressed: u8,
}

impl HostInputState {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn set_keyboard(&mut self, key: JoypadKey, pressed: bool) {
        Self::set_mask_bit(&mut self.keyboard_pressed, key, pressed);
    }

    pub(super) fn set_gamepad(&mut self, key: JoypadKey, pressed: bool) {
        Self::set_mask_bit(&mut self.gamepad_pressed, key, pressed);
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
        (self.keyboard_pressed | self.gamepad_pressed | self.gamepad_stick_dpad_pressed) & 0x0F
    }

    pub(super) fn buttons_pressed(&self) -> u8 {
        ((self.keyboard_pressed | self.gamepad_pressed) >> 4) & 0x0F
    }

    fn set_mask_bit(mask: &mut u8, key: JoypadKey, pressed: bool) {
        let bit = joypad_host_bit(key);
        if pressed {
            *mask |= bit;
        } else {
            *mask &= !bit;
        }
    }
}

fn joypad_host_bit(key: JoypadKey) -> u8 {
    match key {
        JoypadKey::Right => 1 << 0,
        JoypadKey::Left => 1 << 1,
        JoypadKey::Up => 1 << 2,
        JoypadKey::Down => 1 << 3,
        JoypadKey::A => 1 << 4,
        JoypadKey::B => 1 << 5,
        JoypadKey::Select => 1 << 6,
        JoypadKey::Start => 1 << 7,
    }
}
