use super::Emulator;
use crate::hardware::input::{ControllerKind, ControllerPort, Input, InputFault};
use std::ops::Deref;

pub struct InputMut<'a> {
    input: &'a mut Input,
    now: u64,
}

impl Deref for InputMut<'_> {
    type Target = Input;
    fn deref(&self) -> &Self::Target {
        self.input
    }
}

impl InputMut<'_> {
    pub fn set_host_pad(&mut self, port: ControllerPort, buttons: u8, dpad: u8) {
        fn project(value: u8, bits: &[(u8, u8)]) -> u8 {
            bits.iter().fold(0, |mapped, &(source, target)| {
                mapped | ((value >> source & 1) << target)
            })
        }
        let raw = project(buttons, &[(6, 6), (0, 4), (1, 5), (3, 7)])
            | project(dpad, &[(0, 3), (1, 2), (2, 0), (3, 1)]);
        self.set_controller_raw(port, !raw);
        self.set_controller_extra_raw(port, !project(buttons, &[(2, 3), (4, 2), (7, 1), (5, 0)]));
    }

    pub fn set_controller_raw(&mut self, port: ControllerPort, value: u8) {
        self.input.set_controller_raw_at(port, value, self.now);
    }
    pub fn set_controller_extra_raw(&mut self, port: ControllerPort, value: u8) {
        self.input.set_controller_extra_at(port, value, self.now);
    }
    pub fn set_controller_kind(&mut self, port: ControllerPort, kind: ControllerKind) {
        self.input.set_controller_kind_at(port, kind, self.now);
    }
}

impl Emulator {
    pub fn input(&self) -> &Input {
        &self.bus.input
    }

    pub fn input_mut(&mut self) -> InputMut<'_> {
        InputMut {
            now: self
                .bus
                .input_state_tick()
                .expect("validated input frontier"),
            input: &mut self.bus.input,
        }
    }

    pub fn input_fault(&self) -> Option<InputFault> {
        self.bus.input.fault()
    }
}
