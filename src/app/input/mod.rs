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
    keyboard_p3_pressed: u16,
    gamepad_p3_pressed: u16,
    remote_p3_pressed: u16,
    keyboard_p4_pressed: u16,
    gamepad_p4_pressed: u16,
    remote_p4_pressed: u16,
    keyboard_p5_pressed: u16,
    gamepad_p5_pressed: u16,
    remote_p5_pressed: u16,
    coleco_keyboard_keypad_pressed: u16,
    coleco_keyboard_keypad_p2_pressed: u16,
    coleco_remote_keypad_pressed: u16,
    coleco_remote_keypad_p2_pressed: u16,
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

    pub(super) fn clear_keyboard(&mut self) {
        self.keyboard_pressed = 0;
        self.keyboard_p2_pressed = 0;
        self.keyboard_p3_pressed = 0;
        self.keyboard_p4_pressed = 0;
        self.keyboard_p5_pressed = 0;
        self.coleco_keyboard_keypad_pressed = 0;
        self.coleco_keyboard_keypad_p2_pressed = 0;
        self.tilt_keyboard_pressed = 0;
        self.ws_keyboard_x_pressed = 0;
        self.ws_keyboard_y_pressed = 0;
        self.ws_keyboard_button_pressed = 0;
    }

    pub(super) fn set_keyboard_p3(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.keyboard_p3_pressed, key, pressed);
    }

    pub(super) fn set_gamepad_p3(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.gamepad_p3_pressed, key, pressed);
    }

    pub(super) fn set_remote_p3(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.remote_p3_pressed, key, pressed);
    }

    pub(super) fn set_keyboard_p4(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.keyboard_p4_pressed, key, pressed);
    }

    pub(super) fn set_gamepad_p4(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.gamepad_p4_pressed, key, pressed);
    }

    pub(super) fn set_remote_p4(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.remote_p4_pressed, key, pressed);
    }

    pub(super) fn set_keyboard_p5(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.keyboard_p5_pressed, key, pressed);
    }

    pub(super) fn set_gamepad_p5(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.gamepad_p5_pressed, key, pressed);
    }

    pub(super) fn set_remote_p5(&mut self, key: HostButton, pressed: bool) {
        Self::set_mask_bit(&mut self.remote_p5_pressed, key, pressed);
    }

    pub(super) fn set_coleco_keyboard_keypad(&mut self, player: u8, key: u8, pressed: bool) {
        let mask = match player {
            1 => &mut self.coleco_keyboard_keypad_pressed,
            2 => &mut self.coleco_keyboard_keypad_p2_pressed,
            _ => return,
        };
        set_coleco_keypad_bit(mask, key, pressed);
    }

    pub(super) fn set_coleco_remote_keypad(&mut self, player: u8, key: u8, pressed: bool) {
        let mask = match player {
            1 => &mut self.coleco_remote_keypad_pressed,
            2 => &mut self.coleco_remote_keypad_p2_pressed,
            _ => return,
        };
        set_coleco_keypad_bit(mask, key, pressed);
    }

    pub(super) fn coleco_keypad_pressed(&self, player: u8) -> Option<u8> {
        let keypad_pressed = self.coleco_keypad_mask(player)?;
        (keypad_pressed != 0).then(|| keypad_pressed.trailing_zeros() as u8)
    }

    pub(super) fn coleco_tas_controllers(
        &self,
    ) -> anyhow::Result<[crate::tas_project::TasColecoControllerInput; 2]> {
        Ok([
            self.coleco_tas_controller(1)?,
            self.coleco_tas_controller(2)?,
        ])
    }

    fn coleco_tas_controller(
        &self,
        player: u8,
    ) -> anyhow::Result<crate::tas_project::TasColecoControllerInput> {
        let keypad_mask = self.coleco_keypad_mask(player).unwrap_or_default();
        anyhow::ensure!(
            keypad_mask.count_ones() <= 1,
            "ColecoVision player {player} has multiple keypad keys held"
        );
        let buttons = match player {
            1 => self.buttons_pressed(),
            2 => self.buttons_p2_pressed(),
            _ => 0,
        };
        anyhow::ensure!(
            buttons & !0x0F == 0,
            "ColecoVision player {player} has unsupported host buttons held"
        );
        let fallback_key_count = (buttons & 0x0C).count_ones();
        anyhow::ensure!(
            fallback_key_count <= 1 && !(keypad_mask != 0 && fallback_key_count != 0),
            "ColecoVision player {player} has ambiguous keypad input"
        );
        let keypad = (keypad_mask != 0)
            .then(|| coleco_tas_keypad(keypad_mask.trailing_zeros() as u8))
            .flatten();
        let dpad = match player {
            1 => self.dpad_pressed(),
            2 => self.dpad_p2_pressed(),
            _ => 0,
        };
        Ok(crate::emu_backend::coleco::tas_controller_from_host_input(
            buttons, dpad, keypad,
        ))
    }

    fn coleco_keypad_mask(&self, player: u8) -> Option<u16> {
        match player {
            1 => Some(self.coleco_keyboard_keypad_pressed | self.coleco_remote_keypad_pressed),
            2 => {
                Some(self.coleco_keyboard_keypad_p2_pressed | self.coleco_remote_keypad_p2_pressed)
            }
            _ => None,
        }
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

    pub(super) fn coleco_dpad_pressed(&self, player: u8) -> u8 {
        let dpad = match player {
            1 => self.dpad_pressed(),
            2 => self.dpad_p2_pressed(),
            _ => return 0,
        };
        self.coleco_keypad_pressed(player)
            .map_or(dpad, |key| dpad | ((key + 1) << 4))
    }

    pub(super) fn dpad_p3_pressed(&self) -> u8 {
        ((self.keyboard_p3_pressed | self.gamepad_p3_pressed | self.remote_p3_pressed) & 0x0F) as u8
    }

    pub(super) fn buttons_p3_pressed(&self) -> u8 {
        ((self.keyboard_p3_pressed | self.gamepad_p3_pressed | self.remote_p3_pressed) >> 4) as u8
    }

    pub(super) fn dpad_p4_pressed(&self) -> u8 {
        ((self.keyboard_p4_pressed | self.gamepad_p4_pressed | self.remote_p4_pressed) & 0x0F) as u8
    }

    pub(super) fn buttons_p4_pressed(&self) -> u8 {
        ((self.keyboard_p4_pressed | self.gamepad_p4_pressed | self.remote_p4_pressed) >> 4) as u8
    }

    pub(super) fn dpad_p5_pressed(&self) -> u8 {
        ((self.keyboard_p5_pressed | self.gamepad_p5_pressed | self.remote_p5_pressed) & 0x0F) as u8
    }

    pub(super) fn buttons_p5_pressed(&self) -> u8 {
        ((self.keyboard_p5_pressed | self.gamepad_p5_pressed | self.remote_p5_pressed) >> 4) as u8
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

fn set_coleco_keypad_bit(mask: &mut u16, key: u8, pressed: bool) {
    if key >= 12 {
        return;
    }
    let Some(bit) = 1u16.checked_shl(u32::from(key)) else {
        return;
    };
    if pressed {
        *mask |= bit;
    } else {
        *mask &= !bit;
    }
}

fn coleco_tas_keypad(key: u8) -> Option<crate::tas_project::TasColecoKeypadKey> {
    use crate::tas_project::TasColecoKeypadKey;

    Some(match key {
        0 => TasColecoKeypadKey::Zero,
        1 => TasColecoKeypadKey::One,
        2 => TasColecoKeypadKey::Two,
        3 => TasColecoKeypadKey::Three,
        4 => TasColecoKeypadKey::Four,
        5 => TasColecoKeypadKey::Five,
        6 => TasColecoKeypadKey::Six,
        7 => TasColecoKeypadKey::Seven,
        8 => TasColecoKeypadKey::Eight,
        9 => TasColecoKeypadKey::Nine,
        10 => TasColecoKeypadKey::Star,
        11 => TasColecoKeypadKey::Pound,
        _ => return None,
    })
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

    pub(super) fn current_host_joypad_p3_input(&self) -> (u8, u8) {
        self.host_joypad_multitap_input(3)
    }

    pub(super) fn current_host_joypad_p4_input(&self) -> (u8, u8) {
        self.host_joypad_multitap_input(4)
    }

    pub(super) fn current_host_joypad_p5_input(&self) -> (u8, u8) {
        self.host_joypad_multitap_input(5)
    }

    fn host_joypad_multitap_input(&self, player: u8) -> (u8, u8) {
        if self.active_system != ActiveSystem::Pce {
            return (0, 0);
        }
        match player {
            3 => (
                self.host_input.buttons_p3_pressed(),
                self.host_input.dpad_p3_pressed(),
            ),
            4 => (
                self.host_input.buttons_p4_pressed(),
                self.host_input.dpad_p4_pressed(),
            ),
            5 => (
                self.host_input.buttons_p5_pressed(),
                self.host_input.dpad_p5_pressed(),
            ),
            _ => (0, 0),
        }
    }

    pub(super) fn host_joypad_input_for_system(&self, system: ActiveSystem) -> (u8, u8) {
        if system == ActiveSystem::WonderSwan {
            (
                self.host_input.ws_buttons_pressed(self.ws_display_rotated),
                self.host_input.ws_dpad_pressed(self.ws_display_rotated),
            )
        } else if system == ActiveSystem::Coleco {
            (
                self.host_input.buttons_pressed(),
                self.host_input.coleco_dpad_pressed(1),
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
        } else if system == ActiveSystem::Coleco {
            (
                self.host_input.buttons_p2_pressed(),
                self.host_input.coleco_dpad_pressed(2),
            )
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
