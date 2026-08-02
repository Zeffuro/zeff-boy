#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keypad {
    select: u8,
    x_buttons: u8,
    y_buttons: u8,
    ab_start: u8,
}

impl Default for Keypad {
    fn default() -> Self {
        Self::new()
    }
}

impl Keypad {
    pub fn new() -> Self {
        Self {
            select: 0,
            x_buttons: 0,
            y_buttons: 0,
            ab_start: 0,
        }
    }

    pub fn set_host_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) {
        self.x_buttons = dpad_pressed & 0x0F;
        self.ab_start = buttons_pressed & 0x07;
        self.y_buttons = (buttons_pressed >> 4) & 0x0F;
    }

    pub fn read(&self) -> u8 {
        let mut value = 0x0F;
        if self.select & 0x10 != 0 {
            value &= !self.y_buttons;
        }
        if self.select & 0x20 != 0 {
            value &= !self.x_buttons;
        }
        if self.select & 0x40 != 0 {
            value &= !self.ab_start;
        }
        (self.select & 0x70) | (value & 0x0F)
    }

    pub fn write(&mut self, value: u8) {
        self.select = value & 0x70;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_rows_read_as_active_low_nibbles() {
        let mut keypad = Keypad::new();
        keypad.set_host_input(0b0011_0011, 0b0101);
        keypad.write(0x20);
        assert_eq!(keypad.read() & 0x0F, 0b1010);
        keypad.write(0x40);
        assert_eq!(keypad.read() & 0x0F, 0b1100);
        keypad.write(0x10);
        assert_eq!(keypad.read() & 0x0F, 0b1100);
    }
}
