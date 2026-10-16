use super::Emulator;
use crate::hardware::input::Input;

impl Emulator {
    pub fn input(&self) -> &Input {
        &self.bus.input
    }

    pub fn input_mut(&mut self) -> &mut Input {
        &mut self.bus.input
    }
}
