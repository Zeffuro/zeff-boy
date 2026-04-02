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
            // SGB uses P14/P15 high reads to expose current joypad index when multiplayer mode is active.
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
        let was_both_high = !self.select_buttons && !self.select_dpad;

        self.select_buttons = value & 0x20 == 0;
        self.select_dpad = value & 0x10 == 0;

        let is_both_high = !self.select_buttons && !self.select_dpad;
        
        if was_both_high && !is_both_high && self.sgb_joypad_count > 1 {
            self.sgb_current_joypad = (self.sgb_current_joypad + 1) % self.sgb_joypad_count;
        }
    }

    pub fn set_sgb_multiplayer_mode(&mut self, mode: u8) {
        self.sgb_joypad_count = match mode & 0x03 {
            0x00 => 1,
            0x01 => 2,
            0x03 => 4,
            _ => 1,
        };
        self.sgb_current_joypad = 0;
    }

    pub fn key_down(&mut self, key: JoypadKey) -> bool {
        self.set_key_state(key, true)
    }

    pub fn key_up(&mut self, key: JoypadKey) {
        let _ = self.set_key_state(key, false);
    }

    pub fn apply_pressed_masks(&mut self, buttons_pressed: u8, dpad_pressed: u8) -> bool {
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

    #[test]
    fn repeated_0x30_does_not_cycle_id() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x01);
        jp.write(0x30);
        jp.write(0x30);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0F, "ID should still be player 1");
    }

    #[test]
    fn falling_edge_0x30_to_0x00_cycles() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x01);
        jp.write(0x00);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0E, "ID should be player 2 after $30→$00 fall");
    }

    #[test]
    fn falling_edge_0x30_to_0x20_cycles() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x01);
        jp.write(0x20);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0E, "ID should be player 2 after $30→$20 fall");
    }

    #[test]
    fn falling_edge_0x30_to_0x10_cycles() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x01);
        jp.write(0x10);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0E, "ID should be player 2 after $30→$10 fall");
    }

    #[test]
    fn rising_edge_does_not_cycle() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x01);
        jp.write(0x20);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0E, "Rising edge should NOT cycle (still player 2)");
    }

    #[test]
    fn non_both_high_transitions_do_not_cycle() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x01);
        jp.write(0x20);
        jp.write(0x10);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0E, "Non-both-high transitions should not cycle");
    }

    #[test]
    fn post_mlt_req_packet_stop_bit_cycles_once() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x01);
        
        jp.write(0x10);

        let mut jp = Joypad::new();
        jp.write(0x10);
        jp.set_sgb_multiplayer_mode(0x01);
        
        jp.write(0x30);
        jp.write(0x20);
        jp.write(0x30);

        assert_eq!(read_sgb_id(&jp), 0x0E, "Stop bit should cause exactly 1 falling-edge cycle");
    }

    #[test]
    fn vblank_joypad_poll_cycles_once() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x01);
        jp.write(0x20);
        jp.write(0x10);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0E, "One VBlank poll should cycle to player 2");
    }

    #[test]
    fn pokemon_red_sgb_detection_sequence() {
        let mut jp = Joypad::new();
        
        jp.write(0x10);
        jp.set_sgb_multiplayer_mode(0x01);

        jp.write(0x30);
        jp.write(0x20);
        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0E, "After post-packet stop bit: player 2");

        let expected_after_vblank = [0x0F, 0x0E, 0x0F, 0x0E];
        for (i, &expected) in expected_after_vblank.iter().enumerate() {
            jp.write(0x20);
            jp.write(0x10);
            jp.write(0x30);
            assert_eq!(read_sgb_id(&jp), expected, "VBlank {} mismatch", i + 1);
        }
        
        assert_eq!(read_sgb_id(&jp), 0x0E, "After 4 VBlanks, should be player 2");

        jp.write(0x30);
        assert_eq!(read_sgb_id(&jp), 0x0E, "Detection read should see player 2");

        let p1_val = jp.read();
        assert_ne!(p1_val & 0x03, 0x03, "Game should detect SGB");
    }

    #[test]
    fn four_player_cycling() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x03);
        
        for expected_id in [0x0E, 0x0D, 0x0C, 0x0F] {
            jp.write(0x00);
            jp.write(0x30);
            assert_eq!(read_sgb_id(&jp), expected_id);
        }
    }

    #[test]
    fn single_player_mode_ignores_cycling() {
        let mut jp = Joypad::new();
        jp.set_sgb_multiplayer_mode(0x00); // 1-player
        jp.write(0x00); // would be falling edge, but count=1
        jp.write(0x30);
        assert_eq!(jp.read() & 0x0F, 0x0F);
    }
}
