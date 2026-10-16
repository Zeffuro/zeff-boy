use crate::Region;
use crate::constants::{
    CONTROLLER_PORT_COUNT, IO_DATA_LATCH, IO_PIN_MASK, IO_PORT_COUNT, IO_SERIAL_CONTROL_MASK,
    IO_SERIAL_MODE_MASK, IO_SERIAL_REGISTERS_PER_PORT, IO_SERIAL_TX_RESET, IO_TH,
};

mod pad;
mod state;
pub use pad::ControllerKind;
use pad::Pad;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerPort {
    One,
    Two,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pads: [Pad; CONTROLLER_PORT_COUNT],
    data: [u8; IO_PORT_COUNT],
    control: [u8; IO_PORT_COUNT],
    transmit: [u8; IO_PORT_COUNT],
    serial_control: [u8; IO_PORT_COUNT],
}

impl Default for Input {
    fn default() -> Self {
        Self {
            pads: [Pad::default(); CONTROLLER_PORT_COUNT],
            data: [IO_PIN_MASK; IO_PORT_COUNT],
            control: [0; IO_PORT_COUNT],
            transmit: [IO_SERIAL_TX_RESET; IO_PORT_COUNT],
            serial_control: [0; IO_PORT_COUNT],
        }
    }
}

impl Input {
    /// Active-low Start, A, C, B, Right, Left, Down, Up bits.
    pub fn set_controller_raw(&mut self, port: ControllerPort, value: u8) {
        self.pads[port as usize].buttons = value;
    }

    pub fn controller_raw(&self, port: ControllerPort) -> u8 {
        self.pads[port as usize].buttons
    }

    /// Active-low Mode, X, Y, Z in bits 3 through 0.
    pub fn set_controller_extra_raw(&mut self, port: ControllerPort, value: u8) {
        self.pads[port as usize].set_extra(value);
    }

    pub fn controller_extra_raw(&self, port: ControllerPort) -> u8 {
        self.pads[port as usize].extra
    }

    pub fn set_controller_kind(&mut self, port: ControllerPort, kind: ControllerKind) {
        self.pads[port as usize].set_kind(kind);
    }

    pub fn controller_kind(&self, port: ControllerPort) -> ControllerKind {
        self.pads[port as usize].kind
    }

    pub(crate) fn power_on(&mut self) {
        for pad in &mut self.pads {
            pad.power_on();
        }
        self.data.fill(IO_PIN_MASK);
        self.control.fill(0);
        self.transmit.fill(IO_SERIAL_TX_RESET);
        self.serial_control.fill(0);
    }

    fn th(&self, port: usize) -> bool {
        self.control[port] & IO_TH == 0 || self.data[port] & IO_TH != 0
    }

    pub(crate) fn read_data(&self, port: usize, now: u64, region: Region) -> u8 {
        let direction = self.control[port] & IO_PIN_MASK;
        let pins = self.pads.get(port).map_or(IO_PIN_MASK, |pad| {
            pad.read(self.th(port), now, region.pad_timeout_ticks())
        });
        (pins & !direction) | (self.data[port] & (direction | IO_DATA_LATCH))
    }

    pub(crate) fn write_data(&mut self, port: usize, value: u8, now: u64, region: Region) {
        let before = self.th(port);
        self.data[port] = value;
        self.transition(port, before, now, region);
    }

    pub(crate) fn read_control(&self, port: usize) -> u8 {
        self.control[port]
    }

    pub(crate) fn write_control(&mut self, port: usize, value: u8, now: u64, region: Region) {
        let before = self.th(port);
        self.control[port] = value;
        self.transition(port, before, now, region);
    }

    pub(crate) fn read_serial(&self, register: usize) -> u8 {
        let port = register / IO_SERIAL_REGISTERS_PER_PORT;
        match register % IO_SERIAL_REGISTERS_PER_PORT {
            0 => self.transmit[port],
            1 => 0,
            _ => self.serial_control[port],
        }
    }

    pub(crate) fn write_serial(&mut self, register: usize, value: u8) -> bool {
        let port = register / IO_SERIAL_REGISTERS_PER_PORT;
        match register % IO_SERIAL_REGISTERS_PER_PORT {
            0 => self.transmit[port] = value,
            1 => {}
            _ => {
                // Serial pin modes require a UART device.
                if value & IO_SERIAL_MODE_MASK != 0 {
                    return false;
                }
                self.serial_control[port] = value & IO_SERIAL_CONTROL_MASK;
            }
        }
        true
    }

    fn transition(&mut self, port: usize, before: bool, now: u64, region: Region) {
        let after = self.th(port);
        if before != after
            && let Some(pad) = self.pads.get_mut(port)
        {
            pad.transition(after, now, region.pad_timeout_ticks());
        }
    }
}

#[cfg(test)]
#[path = "input/tests.rs"]
mod tests;
