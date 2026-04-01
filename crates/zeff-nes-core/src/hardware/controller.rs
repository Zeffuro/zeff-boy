use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    A,
    B,
    Select,
    Start,
    Up,
    Down,
    Left,
    Right,
}

pub enum ControllerType {
    Standard,
    Zapper { trigger: bool, hit: bool },
}

pub struct Controller {
    buttons: u8,
    shift_register: u8,
    strobe: bool,
    controller_type: ControllerType,
}

impl Default for Controller {
    fn default() -> Self {
        Self::new()
    }
}

impl Controller {
    pub fn new() -> Self {
        Self {
            buttons: 0,
            shift_register: 0,
            strobe: false,
            controller_type: ControllerType::Standard,
        }
    }

    pub fn set_type(&mut self, controller_type: ControllerType) {
        self.controller_type = controller_type;
    }

    pub fn set_buttons(&mut self, state: u8) {
        self.buttons = state;
    }

    pub fn press(&mut self, button: Button) {
        self.buttons |= Self::button_mask(button);
    }

    pub fn release(&mut self, button: Button) {
        self.buttons &= !Self::button_mask(button);
    }

    pub fn write(&mut self, val: u8) {
        let new_strobe = val & 0x01 != 0;
        if self.strobe && !new_strobe {
            self.shift_register = self.buttons;
        }
        self.strobe = new_strobe;
    }

    pub fn read(&mut self) -> u8 {
        match &self.controller_type {
            ControllerType::Standard => self.read_standard(),
            ControllerType::Zapper { trigger, hit } => self.read_zapper(*trigger, *hit),
        }
    }

    fn read_standard(&mut self) -> u8 {
        if self.strobe {
            return self.buttons & 0x01;
        }
        let bit = self.shift_register & 0x01;
        self.shift_register >>= 1;
        self.shift_register |= 0x80;
        bit
    }

    fn read_zapper(&mut self, trigger: bool, hit: bool) -> u8 {
        if self.strobe {
            let mut result = self.buttons & 0x01;
            if trigger {
                result |= 0x02;
            }
            if !hit {
                result |= 0x04;
            }
            result |= 0x08;
            return result;
        }
        let bit = self.shift_register & 0x01;
        self.shift_register >>= 1;
        self.shift_register |= 0x80;
        bit
    }

    fn button_mask(button: Button) -> u8 {
        match button {
            Button::A => 0x01,
            Button::B => 0x02,
            Button::Select => 0x04,
            Button::Start => 0x08,
            Button::Up => 0x10,
            Button::Down => 0x20,
            Button::Left => 0x40,
            Button::Right => 0x80,
        }
    }

    pub fn write_state(&self, w: &mut crate::save_state::StateWriter) {
        w.write_u8(self.buttons);
        w.write_u8(self.shift_register);
        w.write_bool(self.strobe);
        match &self.controller_type {
            ControllerType::Standard => {
                w.write_u8(0);
            }
            ControllerType::Zapper { trigger, hit } => {
                w.write_u8(1);
                w.write_bool(*trigger);
                w.write_bool(*hit);
            }
        }
    }

    pub fn read_state(&mut self, r: &mut crate::save_state::StateReader) -> anyhow::Result<()> {
        self.buttons = r.read_u8()?;
        self.shift_register = r.read_u8()?;
        self.strobe = r.read_bool()?;
        let type_tag = r.read_u8()?;
        match type_tag {
            0 => self.controller_type = ControllerType::Standard,
            1 => {
                let trigger = r.read_bool()?;
                let hit = r.read_bool()?;
                self.controller_type = ControllerType::Zapper { trigger, hit };
            }
            _ => self.controller_type = ControllerType::Standard,
        }
        Ok(())
    }
}

impl fmt::Debug for Controller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Controller")
            .field("buttons", &format_args!("{:#04X}", self.buttons))
            .field("strobe", &self.strobe)
            .finish()
    }
}
