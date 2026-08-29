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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerType {
    Standard,
    Zapper { trigger: bool, hit: bool },
    VsZapper { trigger: bool, hit: bool },
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExpansionDevice {
    #[default]
    None,
    HyperShot(HyperShot),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HyperShot {
    buttons: u8,
    shift_register: u8,
    strobe: bool,
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

    pub const fn controller_type(&self) -> ControllerType {
        self.controller_type
    }

    pub fn set_zapper_hit(&mut self, hit: bool) {
        match &mut self.controller_type {
            ControllerType::Zapper {
                hit: current_hit, ..
            }
            | ControllerType::VsZapper {
                hit: current_hit, ..
            } => {
                *current_hit = hit;
            }
            ControllerType::Standard => {}
        }
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
            self.shift_register = self.serial_report();
        }
        self.strobe = new_strobe;
    }

    pub fn read(&mut self) -> u8 {
        match &self.controller_type {
            ControllerType::Standard => self.read_standard(),
            ControllerType::Zapper { trigger, hit } => Self::read_zapper(*trigger, *hit),
            ControllerType::VsZapper { trigger, hit } => self.read_vs_zapper(*trigger, *hit),
        }
    }

    fn read_standard(&mut self) -> u8 {
        self.read_serial(self.buttons)
    }

    fn read_serial(&mut self, serial_report: u8) -> u8 {
        if self.strobe {
            return serial_report & 0x01;
        }
        let bit = self.shift_register & 0x01;
        self.shift_register >>= 1;
        self.shift_register |= 0x80;
        bit
    }

    fn read_zapper(trigger: bool, hit: bool) -> u8 {
        let no_light_reflected = if hit { 0 } else { 0x08 };
        let trigger = if trigger { 0x10 } else { 0 };
        no_light_reflected | trigger
    }

    fn read_vs_zapper(&mut self, trigger: bool, hit: bool) -> u8 {
        self.read_serial_zero_fill(Self::vs_zapper_serial_report(trigger, hit))
    }

    fn serial_report(&self) -> u8 {
        match self.controller_type {
            ControllerType::Standard | ControllerType::Zapper { .. } => self.buttons,
            ControllerType::VsZapper { trigger, hit } => {
                Self::vs_zapper_serial_report(trigger, hit)
            }
        }
    }

    fn vs_zapper_serial_report(trigger: bool, hit: bool) -> u8 {
        let up_always_pressed = 0x10;
        let light_sensed = if hit { 0x40 } else { 0 };
        let trigger = if trigger { 0x80 } else { 0 };
        up_always_pressed | light_sensed | trigger
    }

    fn read_serial_zero_fill(&mut self, serial_report: u8) -> u8 {
        if self.strobe {
            return serial_report & 0x01;
        }
        let bit = self.shift_register & 0x01;
        self.shift_register >>= 1;
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
            ControllerType::VsZapper { trigger, hit } => {
                w.write_u8(2);
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
            2 => {
                let trigger = r.read_bool()?;
                let hit = r.read_bool()?;
                self.controller_type = ControllerType::VsZapper { trigger, hit };
            }
            _ => self.controller_type = ControllerType::Standard,
        }
        Ok(())
    }
}

impl ExpansionDevice {
    pub fn set_buttons(&mut self, state: u8) {
        if let Self::HyperShot(device) = self {
            device.set_buttons(state);
        }
    }

    pub fn write(&mut self, val: u8) {
        if let Self::HyperShot(device) = self {
            device.write(val);
        }
    }

    pub fn read_4016(&mut self) -> u8 {
        match self {
            Self::None => 0,
            Self::HyperShot(device) => device.read_4016(),
        }
    }

    pub fn read_4017(&self) -> u8 {
        match self {
            Self::None => 0,
            Self::HyperShot(device) => device.read_4017(),
        }
    }
}

impl HyperShot {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_buttons(&mut self, state: u8) {
        self.buttons = state;
    }

    pub fn write(&mut self, val: u8) {
        let new_strobe = val & 0x01 != 0;
        if self.strobe && !new_strobe {
            self.shift_register = self.serial_buttons();
        }
        self.strobe = new_strobe;
    }

    pub fn read_4016(&mut self) -> u8 {
        let bit = if self.strobe {
            self.serial_buttons() & 0x01
        } else {
            let bit = self.shift_register & 0x01;
            self.shift_register >>= 1;
            self.shift_register |= 0x80;
            bit
        };
        bit << 1
    }

    pub fn read_4017(&self) -> u8 {
        let no_light_reflected = 0x08;
        let trigger = if self.buttons & Self::trigger_button_mask() != 0 {
            0x10
        } else {
            0
        };
        no_light_reflected | trigger
    }

    fn serial_buttons(&self) -> u8 {
        self.buttons & !Self::trigger_button_mask()
    }

    fn trigger_button_mask() -> u8 {
        Self::button_mask(Button::A)
    }

    fn button_mask(button: Button) -> u8 {
        Controller::button_mask(button)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hyper_shot_outputs_serial_controller_bits_on_4016_bit_one() {
        let mut device = HyperShot::new();
        device.set_buttons(
            Controller::button_mask(Button::Start) | Controller::button_mask(Button::Right),
        );

        device.write(1);
        device.write(0);

        assert_eq!(device.read_4016(), 0);
        assert_eq!(device.read_4016(), 0);
        assert_eq!(device.read_4016(), 0);
        assert_eq!(device.read_4016(), 0x02);
        assert_eq!(device.read_4016(), 0);
        assert_eq!(device.read_4016(), 0);
        assert_eq!(device.read_4016(), 0);
        assert_eq!(device.read_4016(), 0x02);
    }

    #[test]
    fn hyper_shot_outputs_trigger_and_light_bits_on_4017() {
        let mut device = HyperShot::new();

        assert_eq!(device.read_4017(), 0x08);

        device.set_buttons(Controller::button_mask(Button::A));
        assert_eq!(device.read_4017(), 0x18);
    }

    #[test]
    fn hyper_shot_trigger_is_not_part_of_serial_controller_data() {
        let mut device = HyperShot::new();
        device.set_buttons(
            Controller::button_mask(Button::A) | Controller::button_mask(Button::Start),
        );

        device.write(1);
        assert_eq!(device.read_4016(), 0);

        device.write(0);
        assert_eq!(device.read_4016(), 0);
        assert_eq!(device.read_4016(), 0);
        assert_eq!(device.read_4016(), 0);
        assert_eq!(device.read_4016(), 0x02);
    }

    #[test]
    fn zapper_outputs_light_and_trigger_bits_on_4017() {
        let mut controller = Controller::new();

        controller.set_type(ControllerType::Zapper {
            trigger: false,
            hit: false,
        });
        assert_eq!(controller.read(), 0x08);

        controller.set_type(ControllerType::Zapper {
            trigger: true,
            hit: false,
        });
        assert_eq!(controller.read(), 0x18);

        controller.set_type(ControllerType::Zapper {
            trigger: true,
            hit: true,
        });
        assert_eq!(controller.read(), 0x10);
    }

    #[test]
    fn zapper_read_does_not_shift_serial_controller_state() {
        let mut controller = Controller::new();
        controller.set_buttons(Controller::button_mask(Button::A));
        controller.write(1);
        controller.write(0);

        controller.set_type(ControllerType::Zapper {
            trigger: false,
            hit: false,
        });
        assert_eq!(controller.read(), 0x08);
        assert_eq!(controller.read(), 0x08);
    }

    #[test]
    fn vs_zapper_outputs_serial_report_on_bit_zero() {
        let mut controller = Controller::new();
        controller.set_type(ControllerType::VsZapper {
            trigger: true,
            hit: true,
        });

        controller.write(1);
        controller.write(0);

        let bits: Vec<u8> = (0..8).map(|_| controller.read() & 0x01).collect();

        assert_eq!(bits, [0, 0, 0, 0, 1, 0, 1, 1]);
        assert_eq!(controller.read() & 0x01, 0);
    }

    #[test]
    fn vs_zapper_light_bit_is_clear_when_not_detecting() {
        let mut controller = Controller::new();
        controller.set_type(ControllerType::VsZapper {
            trigger: false,
            hit: false,
        });

        controller.write(1);
        controller.write(0);

        let bits: Vec<u8> = (0..8).map(|_| controller.read() & 0x01).collect();

        assert_eq!(bits, [0, 0, 0, 0, 1, 0, 0, 0]);
    }
}
