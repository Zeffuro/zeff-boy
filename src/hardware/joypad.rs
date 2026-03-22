use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;

#[derive(Clone, Copy, Debug)]
pub(crate) enum JoypadKey {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

pub(crate) struct Joypad {
    // Active-low: 1 = released, 0 = pressed.
    buttons: u8,
    dpad: u8,
    select_buttons: bool,
    select_dpad: bool,
}

impl Joypad {
    pub(crate) fn new() -> Self {
        Self {
            buttons: 0x0F,
            dpad: 0x0F,
            select_buttons: false,
            select_dpad: false,
        }
    }

    pub(crate) fn read(&self) -> u8 {
        let mut value = 0xC0;

        if self.select_buttons {
            value &= !0x20;
        } else {
            value |= 0x20;
        }

        if self.select_dpad {
            value &= !0x10;
        } else {
            value |= 0x10;
        }

        let mut lines = 0x0F;
        if self.select_buttons {
            lines &= self.buttons;
        }
        if self.select_dpad {
            lines &= self.dpad;
        }

        value | lines
    }

    pub(crate) fn write(&mut self, value: u8) {
        self.select_buttons = value & 0x20 == 0;
        self.select_dpad = value & 0x10 == 0;
    }

    pub(crate) fn key_down(&mut self, key: JoypadKey) -> bool {
        self.set_key_state(key, true)
    }

    pub(crate) fn key_up(&mut self, key: JoypadKey) {
        let _ = self.set_key_state(key, false);
    }
    
    pub(crate) fn apply_pressed_masks(&mut self, buttons_pressed: u8, dpad_pressed: u8) -> bool {
        let old_buttons = self.buttons;
        let old_dpad = self.dpad;

        self.buttons = (!buttons_pressed) & 0x0F;
        self.dpad = (!dpad_pressed) & 0x0F;

        let newly_pressed_buttons = old_buttons & !self.buttons;
        let newly_pressed_dpad = old_dpad & !self.dpad;
        (newly_pressed_buttons | newly_pressed_dpad) != 0
    }

    fn set_key_state(&mut self, key: JoypadKey, pressed: bool) -> bool {
        let (group, bit) = match key {
            JoypadKey::Right => (&mut self.dpad, 0),
            JoypadKey::Left => (&mut self.dpad, 1),
            JoypadKey::Up => (&mut self.dpad, 2),
            JoypadKey::Down => (&mut self.dpad, 3),
            JoypadKey::A => (&mut self.buttons, 0),
            JoypadKey::B => (&mut self.buttons, 1),
            JoypadKey::Select => (&mut self.buttons, 2),
            JoypadKey::Start => (&mut self.buttons, 3),
        };

        let mask = 1u8 << bit;
        let was_released = (*group & mask) != 0;

        if pressed {
            *group &= !mask;
            was_released
        } else {
            *group |= mask;
            false
        }
    }

    pub(crate) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.buttons);
        writer.write_u8(self.dpad);
        writer.write_bool(self.select_buttons);
        writer.write_bool(self.select_dpad);
    }

    pub(crate) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            buttons: reader.read_u8()?,
            dpad: reader.read_u8()?,
            select_buttons: reader.read_bool()?,
            select_dpad: reader.read_bool()?,
        })
    }
}
