use super::constants::IO_OPEN_BUS_VALUE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerPort {
    One,
    Two,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    controller_1: u8,
    controller_2: u8,
}

impl Input {
    pub fn new() -> Self {
        Self {
            controller_1: IO_OPEN_BUS_VALUE,
            controller_2: IO_OPEN_BUS_VALUE,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::new();
    }

    pub fn read_controller(&self, port: ControllerPort) -> u8 {
        match port {
            ControllerPort::One => self.controller_1,
            ControllerPort::Two => self.controller_2,
        }
    }

    pub fn set_controller_raw(&mut self, port: ControllerPort, value: u8) {
        match port {
            ControllerPort::One => self.controller_1 = value,
            ControllerPort::Two => self.controller_2 = value,
        }
    }
}

impl Default for Input {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controllers_default_to_released_active_low_bits() {
        let input = Input::new();

        assert_eq!(input.read_controller(ControllerPort::One), 0xFF);
        assert_eq!(input.read_controller(ControllerPort::Two), 0xFF);
    }

    #[test]
    fn raw_controller_state_can_be_updated() {
        let mut input = Input::new();

        input.set_controller_raw(ControllerPort::One, 0xF0);

        assert_eq!(input.read_controller(ControllerPort::One), 0xF0);
    }
}
