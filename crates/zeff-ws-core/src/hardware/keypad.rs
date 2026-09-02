#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keypad {
    select: u8,
    x_buttons: u8,
    y_buttons: u8,
    ab_start: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct KeypadSaveState {
    pub select: u8,
    pub x_buttons: u8,
    pub y_buttons: u8,
    pub ab_start: u8,
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

    pub fn set_host_input(&mut self, buttons_pressed: u8, dpad_pressed: u8) -> bool {
        let was_pressed = self.any_pressed();
        self.x_buttons = dpad_pressed & 0x0F;
        self.ab_start = map_host_buttons_to_ws_row(buttons_pressed);
        self.y_buttons = (buttons_pressed >> 4) & 0x0F;
        !was_pressed && self.any_pressed()
    }

    pub fn any_pressed(&self) -> bool {
        (self.x_buttons | self.y_buttons | self.ab_start) != 0
    }

    pub fn read(&self) -> u8 {
        let mut value = 0;
        if self.select & 0x10 != 0 {
            value |= self.y_buttons;
        }
        if self.select & 0x20 != 0 {
            value |= self.x_buttons;
        }
        if self.select & 0x40 != 0 {
            value |= self.ab_start;
        }
        (self.select & 0x70) | (value & 0x0F)
    }

    pub fn write(&mut self, value: u8) {
        self.select = value & 0x70;
    }

    pub(crate) fn save_state(&self) -> KeypadSaveState {
        KeypadSaveState {
            select: self.select,
            x_buttons: self.x_buttons,
            y_buttons: self.y_buttons,
            ab_start: self.ab_start,
        }
    }

    pub(crate) fn load_state(&mut self, state: KeypadSaveState) -> anyhow::Result<()> {
        if state.select & !0x70 != 0
            || state.x_buttons & !0x0F != 0
            || state.y_buttons & !0x0F != 0
            || state.ab_start & !0x0F != 0
        {
            anyhow::bail!("invalid WonderSwan keypad state");
        }
        self.select = state.select;
        self.x_buttons = state.x_buttons;
        self.y_buttons = state.y_buttons;
        self.ab_start = state.ab_start;
        Ok(())
    }
}

fn map_host_buttons_to_ws_row(buttons_pressed: u8) -> u8 {
    let mut row = 0u8;
    if buttons_pressed & (1 << 0) != 0 {
        row |= 1 << 2;
    }
    if buttons_pressed & (1 << 1) != 0 {
        row |= 1 << 3;
    }
    if buttons_pressed & (1 << 3) != 0 {
        row |= 1 << 1;
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_rows_read_as_active_high_nibbles() {
        let mut keypad = Keypad::new();
        keypad.set_host_input(0b0011_0011, 0b0101);
        keypad.write(0x20);
        assert_eq!(keypad.read() & 0x0F, 0b0101);
        keypad.write(0x40);
        assert_eq!(keypad.read() & 0x0F, 0b1100);
        keypad.write(0x10);
        assert_eq!(keypad.read() & 0x0F, 0b0011);
    }

    #[test]
    fn maps_generic_a_b_start_to_wonderswan_button_row_bits() {
        let mut keypad = Keypad::new();
        keypad.set_host_input(0b0000_1011, 0);
        keypad.write(0x40);
        assert_eq!(keypad.read() & 0x0F, 0b1110);
    }
}
