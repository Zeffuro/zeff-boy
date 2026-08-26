use crate::save_state::{StateReader, StateWriter};
use anyhow::Result;
use std::fmt;

#[derive(Clone, Copy, Debug)]
pub enum JoypadKey {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

pub struct Joypad {
    // Active-low: 1 = released, 0 = pressed.
    buttons: u8,
    dpad: u8,
    select_buttons: bool,
    select_dpad: bool,
    sgb_joypad_count: u8,
    sgb_current_joypad: u8,
}

impl fmt::Debug for Joypad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Joypad")
            .field("buttons", &format_args!("{:#04X}", self.buttons))
            .field("dpad", &format_args!("{:#04X}", self.dpad))
            .field("select_buttons", &self.select_buttons)
            .field("select_dpad", &self.select_dpad)
            .field("sgb_joypad_count", &self.sgb_joypad_count)
            .field("sgb_current_joypad", &self.sgb_current_joypad)
            .finish()
    }
}

impl Default for Joypad {
    fn default() -> Self {
        Self::new()
    }
}

impl Joypad {
    pub fn new() -> Self {
        Self {
            buttons: 0x0F,
            dpad: 0x0F,
            select_buttons: false,
            select_dpad: false,
            sgb_joypad_count: 1,
            sgb_current_joypad: 0,
        }
    }

    pub fn read(&self) -> u8 {
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
        if !self.select_buttons && !self.select_dpad && self.sgb_joypad_count > 1 {
            // SGB uses P14/P15 high reads to expose the current joypad index when multiplayer mode is active.
            lines = 0x0Fu8.saturating_sub(self.sgb_current_joypad & 0x03);
        } else {
            if self.select_buttons {
                lines &= self.buttons;
            }
            if self.select_dpad {
                lines &= self.dpad;
            }
        }

        value | lines
    }

    pub fn write(&mut self, value: u8) {
        let was_buttons_selected = self.select_buttons;

        self.select_buttons = value & 0x20 == 0;
        self.select_dpad = value & 0x10 == 0;

        if was_buttons_selected && !self.select_buttons && self.sgb_controller_switching_enabled() {
            self.sgb_current_joypad = (self.sgb_current_joypad + 1) & (self.sgb_joypad_count - 1);
        }
    }

    pub fn set_sgb_multiplayer_mode(&mut self, mode: u8) {
        match mode & 0x03 {
            0x00 => {
                self.sgb_joypad_count = 1;
                self.sgb_current_joypad = 0;
            }
            0x01 => {
                self.sgb_joypad_count = 2;
                self.sgb_current_joypad &= 0x01;
            }
            0x02 => {
                // MLT_REQ 2 is invalid, but hardware exposes a glitched 3-player state:
                // the selected player is forced to player 1 or player 3, and ordinary
                // multiplayer switching is disabled while this mode is active.
                self.sgb_joypad_count = 3;
                self.sgb_current_joypad = self.sgb_current_joypad.wrapping_add(1) & 0x02;
            }
            _ => {
                self.sgb_joypad_count = 4;
                self.sgb_current_joypad &= 0x03;
            }
        };
    }

    pub fn key_down(&mut self, key: JoypadKey) -> bool {
        self.set_key_state(key, true)
    }

    pub fn key_up(&mut self, key: JoypadKey) {
        let _ = self.set_key_state(key, false);
    }

    pub fn apply_pressed_masks(&mut self, buttons_pressed: u8, dpad_pressed: u8) -> bool {
        let old_lines = self.read() & 0x0F;

        self.buttons = (!buttons_pressed) & 0x0F;
        self.dpad = (!dpad_pressed) & 0x0F;

        old_lines & !(self.read() & 0x0F) != 0
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

    fn sgb_controller_switching_enabled(&self) -> bool {
        self.sgb_joypad_count > 1 && self.sgb_joypad_count & 1 == 0
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.buttons);
        writer.write_u8(self.dpad);
        writer.write_bool(self.select_buttons);
        writer.write_bool(self.select_dpad);
        writer.write_u8(self.sgb_joypad_count);
        writer.write_u8(self.sgb_current_joypad);
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let buttons = reader.read_u8()?;
        let dpad = reader.read_u8()?;
        let select_buttons = reader.read_bool()?;
        let select_dpad = reader.read_bool()?;

        let sgb_joypad_count = reader.read_u8().unwrap_or(1);
        let sgb_current_joypad = reader.read_u8().unwrap_or(0);
        Ok(Self {
            buttons,
            dpad,
            select_buttons,
            select_dpad,
            sgb_joypad_count,
            sgb_current_joypad,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_sgb_id(jp: &Joypad) -> u8 {
        jp.read() & 0x0F
    }

    fn write_increment(jp: &mut Joypad) {
        jp.write(0x10);
        jp.write(0x30);
    }

    fn send_mlt_req(jp: &mut Joypad, mode: u8) {
        let mut packet = [0u8; 16];
        packet[0] = (0x11 << 3) | 0x01;
        packet[1] = mode & 0x03;

        jp.write(0x00);
        jp.write(0x30);

        for (byte_index, byte) in packet.iter().copied().enumerate() {
            for bit in 0..8 {
                let value = if (byte >> bit) & 1 == 0 { 0x20 } else { 0x10 };
                jp.write(value);
                if byte_index == 15 && bit == 7 {
                    jp.set_sgb_multiplayer_mode(mode);
                }
                jp.write(0x30);
            }
        }

        jp.write(0x20);
        jp.write(0x30);
    }

    #[test]
    fn pressed_masks_report_only_selected_line_falling_edges() {
        let mut joypad = Joypad::new();

        assert!(!joypad.apply_pressed_masks(0x01, 0));
        joypad.write(0x20);
        assert!(!joypad.apply_pressed_masks(0x03, 0));
        assert!(joypad.apply_pressed_masks(0x03, 0x01));

        joypad.write(0x10);
        assert!(!joypad.apply_pressed_masks(0, 0x01));
        assert!(joypad.apply_pressed_masks(0x01, 0x01));
    }

    #[test]
    fn pressed_masks_do_not_repeat_an_already_low_shared_line() {
        let mut joypad = Joypad::new();
        joypad.write(0x00);

        assert!(joypad.apply_pressed_masks(0x01, 0));
        assert!(!joypad.apply_pressed_masks(0x01, 0x01));
    }

    fn push_p1(out: &mut Vec<u8>, jp: &Joypad) {
        out.push(jp.read());
    }

    #[test]
    fn repeated_0x30_does_not_cycle_id() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x01);
        jp.write(0x30);
        jp.write(0x30);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0F);
    }

    #[test]
    fn sgb_controller_switches_on_p15_rising_edge() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x01);

        jp.write(0x20);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0F, "P14 rising does not switch");

        jp.write(0x10);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0E, "P15 rising switches");

        jp.write(0x00);
        jp.write(0x30);
        assert_eq!(
            read_sgb_id(&jp),
            0x0F,
            "Both-low to both-high also raises P15 and switches"
        );
    }

    #[test]
    fn sgb_mlt_req_1_increment_patterns_match_samesuite() {
        let mut jp = Joypad::new();
        send_mlt_req(&mut jp, 0x01);

        let mut actual = Vec::new();

        jp.write(0x10);
        jp.write(0x30);
        push_p1(&mut actual, &jp);

        jp.write(0x20);
        jp.write(0x30);
        push_p1(&mut actual, &jp);

        jp.write(0x10);
        jp.write(0x20);
        jp.write(0x30);
        push_p1(&mut actual, &jp);

        jp.write(0x10);
        jp.write(0x20);
        jp.write(0x10);
        jp.write(0x30);
        push_p1(&mut actual, &jp);

        jp.write(0x10);
        jp.write(0x10);
        jp.write(0x30);
        push_p1(&mut actual, &jp);

        jp.write(0x00);
        jp.write(0x10);
        jp.write(0x30);
        push_p1(&mut actual, &jp);

        jp.write(0x10);
        jp.write(0x00);
        jp.write(0x30);
        push_p1(&mut actual, &jp);

        jp.write(0x00);
        jp.write(0x30);
        push_p1(&mut actual, &jp);

        assert_eq!(actual, [0xFE, 0xFE, 0xFF, 0xFF, 0xFE, 0xFF, 0xFE, 0xFF]);
    }

    #[test]
    fn sgb_mlt_req_packet_side_effects_match_samesuite() {
        let mut jp = Joypad::new();
        let mut actual = Vec::new();

        send_mlt_req(&mut jp, 0x01);
        push_p1(&mut actual, &jp);

        write_increment(&mut jp);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x01);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x02);
        push_p1(&mut actual, &jp);

        write_increment(&mut jp);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        push_p1(&mut actual, &jp);

        write_increment(&mut jp);
        push_p1(&mut actual, &jp);
        write_increment(&mut jp);
        push_p1(&mut actual, &jp);
        write_increment(&mut jp);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        send_mlt_req(&mut jp, 0x01);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        write_increment(&mut jp);
        send_mlt_req(&mut jp, 0x01);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        write_increment(&mut jp);
        write_increment(&mut jp);
        send_mlt_req(&mut jp, 0x01);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        write_increment(&mut jp);
        write_increment(&mut jp);
        write_increment(&mut jp);
        send_mlt_req(&mut jp, 0x01);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        push_p1(&mut actual, &jp);
        send_mlt_req(&mut jp, 0x03);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        send_mlt_req(&mut jp, 0x02);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        write_increment(&mut jp);
        send_mlt_req(&mut jp, 0x02);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        write_increment(&mut jp);
        write_increment(&mut jp);
        send_mlt_req(&mut jp, 0x02);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        write_increment(&mut jp);
        write_increment(&mut jp);
        write_increment(&mut jp);
        send_mlt_req(&mut jp, 0x02);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        send_mlt_req(&mut jp, 0x02);
        write_increment(&mut jp);
        push_p1(&mut actual, &jp);
        write_increment(&mut jp);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        write_increment(&mut jp);
        send_mlt_req(&mut jp, 0x02);
        write_increment(&mut jp);
        push_p1(&mut actual, &jp);
        write_increment(&mut jp);
        push_p1(&mut actual, &jp);

        send_mlt_req(&mut jp, 0x00);
        send_mlt_req(&mut jp, 0x03);
        write_increment(&mut jp);
        write_increment(&mut jp);
        send_mlt_req(&mut jp, 0x02);
        write_increment(&mut jp);
        push_p1(&mut actual, &jp);

        assert_eq!(
            actual,
            [
                0xFF, 0xFE, 0xFF, 0xFF, 0xFF, 0xFF, 0xFE, 0xFD, 0xFC, 0xFE, 0xFF, 0xFE, 0xFF, 0xFF,
                0xFD, 0xFD, 0xFD, 0xFF, 0xFF, 0xFD, 0xFD, 0xFD, 0xFD, 0xFF
            ]
        );
    }

    #[test]
    fn four_player_cycling() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x03);

        for expected_id in [0x0E, 0x0D, 0x0C, 0x0F] {
            jp.write(0x10);
            jp.write(0x30);
            assert_eq!(read_sgb_id(&jp), expected_id);
        }
    }

    #[test]
    fn single_player_mode_ignores_cycling() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x00);
        jp.write(0x00);
        jp.write(0x30);
        assert_eq!(jp.read() & 0x0F, 0x0F);
    }

    #[test]
    fn unsupported_mlt_req_2_disables_switching_on_glitched_player_id() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x03);
        write_increment(&mut jp);

        jp.set_sgb_multiplayer_mode(0x02);
        assert_eq!(jp.read(), 0xFD);

        write_increment(&mut jp);
        write_increment(&mut jp);
        assert_eq!(jp.read(), 0xFD);
    }
}
