use crate::Region;
#[cfg(test)]
use crate::constants::IO_SERIAL_CONTROL_MASK;
use crate::constants::{
    CONTROLLER_PORT_COUNT, IO_DATA_LATCH, IO_PIN_MASK, IO_PORT_COUNT, IO_SERIAL_MODE_MASK,
    IO_SERIAL_REGISTERS_PER_PORT, IO_SERIAL_TX_RESET, IO_TH,
};

mod pad;
mod serial;
mod state;
mod uart_idle;
mod uart_tx;
pub use pad::ControllerKind;
use pad::Pad;
pub use uart_idle::{InputChange, InputFault};

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
    serial: serial::Serial,
    uart_idle: uart_idle::UartIdle,
    uart_tx: uart_tx::UartTx,
}

impl Default for Input {
    fn default() -> Self {
        Self {
            pads: [Pad::default(); CONTROLLER_PORT_COUNT],
            data: [IO_PIN_MASK; IO_PORT_COUNT],
            control: [0; IO_PORT_COUNT],
            transmit: [IO_SERIAL_TX_RESET; IO_PORT_COUNT],
            serial_control: [0; IO_PORT_COUNT],
            serial: serial::Serial::default(),
            uart_idle: uart_idle::UartIdle::default(),
            uart_tx: uart_tx::UartTx::default(),
        }
    }
}

impl Input {
    pub(crate) fn rollback_parallel_only(&self) -> bool {
        self.serial_control == [0; IO_PORT_COUNT]
            && self.transmit == [IO_SERIAL_TX_RESET; IO_PORT_COUNT]
            && self.serial == serial::Serial::default()
            && self.uart_idle == uart_idle::UartIdle::cold()
            && self.uart_tx == uart_tx::UartTx::cold()
    }

    /// Active-low Start, A, C, B, Right, Left, Down, Up bits.
    #[cfg(test)]
    pub(crate) fn set_controller_raw(&mut self, port: ControllerPort, value: u8) {
        self.set_controller_raw_at(port, value, 0);
    }

    pub fn controller_raw(&self, port: ControllerPort) -> u8 {
        self.pads[port as usize].buttons
    }

    /// Active-low Mode, X, Y, Z in bits 3 through 0.
    #[cfg(test)]
    pub(crate) fn set_controller_extra_raw(&mut self, port: ControllerPort, value: u8) {
        self.set_controller_extra_at(port, value, 0);
    }

    pub fn controller_extra_raw(&self, port: ControllerPort) -> u8 {
        self.pads[port as usize].extra
    }

    #[cfg(test)]
    pub(crate) fn set_controller_kind(&mut self, port: ControllerPort, kind: ControllerKind) {
        self.set_controller_kind_at(port, kind, 0);
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
        self.serial = serial::Serial::default();
        self.uart_idle = uart_idle::UartIdle::cold();
        self.uart_tx = uart_tx::UartTx::cold();
    }

    fn th(&self, port: usize) -> bool {
        self.control[port] & IO_TH == 0 || self.data[port] & IO_TH != 0
    }

    pub(crate) fn read_data(&self, port: usize, now: u64, region: Region) -> u8 {
        let serial_output = if self.uart_output_active(port) {
            0x10
        } else {
            0
        };
        let mut direction = self.control[port] & IO_PIN_MASK;
        if self.uart_active(port) {
            direction &= !0x20;
        }
        direction |= serial_output;
        let pins = self.pads.get(port).map_or(IO_PIN_MASK, |pad| {
            pad.read(self.th(port), now, region.pad_timeout_ticks())
        });
        (pins & !direction) | ((self.data[port] | serial_output) & (direction | IO_DATA_LATCH))
    }

    pub(crate) fn data_read_allowed(&self, port: usize) -> bool {
        port < IO_PORT_COUNT && self.fault().is_none() && !self.uart_tx.used(port)
    }

    pub(crate) fn write_data(&mut self, port: usize, value: u8, now: u64, region: Region) -> bool {
        if !self.allow_parallel_update(port, value, self.control[port]) {
            return false;
        }
        let before = self.th(port);
        self.data[port] = value;
        self.transition(port, before, now, region);
        true
    }

    pub(crate) fn read_control(&self, port: usize) -> u8 {
        self.control[port]
    }

    pub(crate) fn write_control(
        &mut self,
        port: usize,
        value: u8,
        now: u64,
        region: Region,
    ) -> bool {
        if !self.allow_parallel_update(port, self.data[port], value) {
            return false;
        }
        let before = self.th(port);
        self.control[port] = value;
        self.transition(port, before, now, region);
        true
    }

    pub(crate) fn read_serial(&self, register: usize, now: u64, region: Region) -> u8 {
        let port = register / IO_SERIAL_REGISTERS_PER_PORT;
        match register % IO_SERIAL_REGISTERS_PER_PORT {
            0 => self.transmit[port],
            1 => 0,
            _ => {
                self.serial_control[port]
                    | if port == IO_PORT_COUNT - 1 {
                        self.serial.status(now, self.serial_control[port], region)
                    } else {
                        0
                    }
            }
        }
    }

    pub(crate) fn write_serial(
        &mut self,
        register: usize,
        value: u8,
        now: u64,
        region: Region,
    ) -> bool {
        let port = register / IO_SERIAL_REGISTERS_PER_PORT;
        if self.fault().is_some()
            || port == IO_PORT_COUNT - 1 && self.uart_tx.any_used()
            || self.uart_active(port) && register % IO_SERIAL_REGISTERS_PER_PORT != 2
        {
            return false;
        }
        match register % IO_SERIAL_REGISTERS_PER_PORT {
            0 => {
                self.note_transmit(port);
                if port == IO_PORT_COUNT - 1 {
                    self.serial
                        .transmit(value, now, self.serial_control[port], region);
                }
                self.transmit[port] = value;
            }
            1 => {}
            _ => {
                let mask = if port == IO_PORT_COUNT - 1 {
                    self.serial
                        .control(self.serial_control[port], value, now, region);
                    serial::CONTROL_MASK
                } else if !self.allow_serial_control(port, value, now) {
                    return false;
                } else {
                    serial::CONTROL_MASK
                };
                self.serial_control[port] = value & mask;
                if port < CONTROLLER_PORT_COUNT && value & 0xc0 != 0 {
                    self.uart_tx.retire_port(port);
                }
            }
        }
        true
    }

    pub(crate) fn read_serial_at_range(
        &self,
        register: usize,
        begin: u64,
        end: u64,
        region: Region,
    ) -> Option<u8> {
        if register >= IO_PORT_COUNT * IO_SERIAL_REGISTERS_PER_PORT
            || self.fault().is_some()
            || begin > end
        {
            return None;
        }
        let port = register / IO_SERIAL_REGISTERS_PER_PORT;
        if self.uart_tx.used(port) {
            if register % IO_SERIAL_REGISTERS_PER_PORT != 2
                || self.serial_control[port] != 0x38
                || !self.uart_tx.receive_high(port)
            {
                return None;
            }
            return self
                .uart_tx
                .status(port, begin, end, region)
                .map(|status| self.serial_control[port] | status);
        }
        self.serial_read_allowed(register)
            .then(|| self.read_serial(register, end, region))
    }

    pub(crate) fn write_serial_at_range(
        &mut self,
        register: usize,
        value: u8,
        begin: u64,
        end: u64,
        region: Region,
    ) -> bool {
        if register >= IO_PORT_COUNT * IO_SERIAL_REGISTERS_PER_PORT
            || self.fault().is_some()
            || begin > end
        {
            return false;
        }
        let port = register / IO_SERIAL_REGISTERS_PER_PORT;
        if port < CONTROLLER_PORT_COUNT
            && register.is_multiple_of(IO_SERIAL_REGISTERS_PER_PORT)
            && self.serial_control[port] == 0x38
        {
            if !self.physical_tr(port, self.data[port], self.control[port])
                || !(self.uart_idle.history[port] == uart_idle::History::KnownIdleNoTx
                    || self.uart_tx.receive_high(port))
                || !self
                    .serial
                    .inactive_empty(end, self.serial_control[2], region)
                || !self.uart_tx.transmit(port, value, begin, end, region)
            {
                return false;
            }
            self.note_transmit(port);
            self.transmit[port] = value;
            return true;
        }
        self.write_serial(register, value, end, region)
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
