pub const KEYINPUT: u32 = 0x0400_0130;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keypad {
    keyinput: u16,
    keycnt: u16,
}

impl Default for Keypad {
    fn default() -> Self {
        Self::new()
    }
}

impl Keypad {
    pub fn new() -> Self {
        Self {
            keyinput: 0x03FF,
            keycnt: 0,
        }
    }

    pub fn set_host_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        let mut pressed = 0u16;
        pressed |= u16::from(buttons_pressed & 0x01); // A
        pressed |= u16::from((buttons_pressed & 0x02) >> 1) << 1; // B
        pressed |= u16::from((buttons_pressed & 0x04) >> 2) << 2; // Select
        pressed |= u16::from((buttons_pressed & 0x08) >> 3) << 3; // Start
        pressed |= u16::from(dpad_pressed & 0x01) << 4; // Right
        pressed |= u16::from((dpad_pressed & 0x02) >> 1) << 5; // Left
        pressed |= u16::from((dpad_pressed & 0x04) >> 2) << 6; // Up
        pressed |= u16::from((dpad_pressed & 0x08) >> 3) << 7; // Down
        pressed |= u16::from((buttons_pressed & 0x10) >> 4) << 8; // L
        pressed |= u16::from((buttons_pressed & 0x20) >> 5) << 9; // R
        self.keyinput = 0x03FF & !pressed;
    }

    pub fn read_keyinput(&self) -> u16 {
        self.keyinput
    }

    pub fn read_keycnt(&self) -> u16 {
        self.keycnt
    }

    pub fn write_keycnt(&mut self, value: u16) {
        self.keycnt = value & 0xC3FF;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_host_input_to_active_low_keyinput() {
        let mut keypad = Keypad::new();
        keypad.set_host_input(0b11_0011, 0b0101);
        assert_eq!(keypad.read_keyinput() & 0x03D3, 0x0080);
    }

    #[test]
    fn maps_host_shoulders_to_l_and_r_bits() {
        let mut keypad = Keypad::new();

        keypad.set_host_input(0x30, 0);

        assert_eq!(keypad.read_keyinput() & 0x0100, 0);
        assert_eq!(keypad.read_keyinput() & 0x0200, 0);
        assert_eq!(keypad.read_keyinput() & 0x00FF, 0x00FF);
    }
}
