use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

const RECEIVE_IDLE: u8 = 0x20;
const INPUT_OUTPUT_IDLE: u8 = 0x38;
// Covers counter seed/reload and a sample edge forced through every baud selector.
pub(super) const OUTPUT_IDLE_SETTLED_TICKS: u64 = (1 + 256 + 32 * 100 + 1) * 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputChange {
    Buttons,
    ExtraButtons,
    ControllerKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputFault {
    pub port: ControllerPort,
    pub at: u64,
    pub change: InputChange,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum History {
    #[default]
    LegacyUnknown,
    KnownIdleNoTx,
    PriorTx,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct UartIdle {
    pub history: [History; CONTROLLER_PORT_COUNT],
    pub fault: Option<InputFault>,
}

impl UartIdle {
    pub fn cold() -> Self {
        Self {
            history: [History::KnownIdleNoTx; CONTROLLER_PORT_COUNT],
            fault: None,
        }
    }

    pub fn write_state(&self, writer: &mut StateWriter) {
        for history in self.history {
            writer.write_u8(match history {
                History::LegacyUnknown => 0,
                History::KnownIdleNoTx => 1,
                History::PriorTx => 2,
            });
        }
    }

    pub fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        let mut result = Self::default();
        for history in &mut result.history {
            *history = match reader.read_u8()? {
                0 => History::LegacyUnknown,
                1 => History::KnownIdleNoTx,
                2 => History::PriorTx,
                _ => anyhow::bail!("invalid MD UART history"),
            };
        }
        Ok(result)
    }
}

impl Input {
    pub(crate) fn cold() -> Self {
        let mut result = Self::default();
        result.power_on();
        result
    }

    pub fn fault(&self) -> Option<InputFault> {
        self.uart_idle.fault
    }

    pub(crate) fn uart_active(&self, port: usize) -> bool {
        port < CONTROLLER_PORT_COUNT
            && matches!(self.serial_control[port], RECEIVE_IDLE | INPUT_OUTPUT_IDLE)
    }

    pub(super) fn uart_output_active(&self, port: usize) -> bool {
        port < CONTROLLER_PORT_COUNT && self.serial_control[port] == INPUT_OUTPUT_IDLE
    }

    pub(super) fn physical_tr(&self, port: usize, data: u8, control: u8) -> bool {
        let th = control & IO_TH == 0 || data & IO_TH != 0;
        // Pad handshake phases affect bits0..3; TR selects C or Start through TH.
        self.pads[port].buttons & (if th { 0x20 } else { 0x80 }) != 0
    }

    fn resolved_tr(&self, port: usize) -> bool {
        if self.uart_active(port) || self.control[port] & RECEIVE_IDLE == 0 {
            self.physical_tr(port, self.data[port], self.control[port])
        } else {
            self.data[port] & RECEIVE_IDLE != 0
        }
    }

    fn physical_tl(&self, port: usize, data: u8, control: u8) -> bool {
        let th = control & IO_TH == 0 || data & IO_TH != 0;
        self.pads[port].buttons & (if th { 0x10 } else { 0x40 }) != 0
    }

    fn parallel_tl_high(&self, port: usize) -> bool {
        if self.control[port] & 0x10 == 0 {
            self.physical_tl(port, self.data[port], self.control[port])
        } else {
            self.data[port] & 0x10 != 0
        }
    }

    fn release_output_allowed(&self, port: usize, value: u8) -> bool {
        !self.uart_output_active(port)
            || value == INPUT_OUTPUT_IDLE
            || self.physical_tl(port, self.data[port], self.control[port])
                && self.parallel_tl_high(port)
    }

    pub(super) fn allow_parallel_update(&self, port: usize, data: u8, control: u8) -> bool {
        self.fault().is_none()
            && (!self.uart_tx.used(port)
                || data == self.data[port] && control == self.control[port])
            && (!self.uart_active(port)
                || self.physical_tr(port, self.data[port], self.control[port])
                    && self.physical_tr(port, data, control))
            && (!self.uart_output_active(port)
                || self.physical_tl(port, self.data[port], self.control[port])
                    && self.physical_tl(port, data, control))
    }

    pub(crate) fn serial_read_allowed(&self, register: usize) -> bool {
        self.fault().is_none()
            && !self.uart_tx.used(register / IO_SERIAL_REGISTERS_PER_PORT)
            && (!self.uart_active(register / IO_SERIAL_REGISTERS_PER_PORT)
                || register % IO_SERIAL_REGISTERS_PER_PORT == 2)
    }

    pub(super) fn allow_serial_control(&self, port: usize, value: u8, now: u64) -> bool {
        if self.fault().is_some() {
            return false;
        }
        let value = value & serial::CONTROL_MASK;
        if self.uart_tx.used(port) {
            return value == INPUT_OUTPUT_IDLE
                && self.serial_control[port] == INPUT_OUTPUT_IDLE
                && self.uart_idle.history[port] == History::PriorTx
                && self.uart_tx.receive_high(port)
                && self.physical_tr(port, self.data[port], self.control[port]);
        }
        if !self.release_output_allowed(port, value) {
            return false;
        }
        if value & IO_SERIAL_MODE_MASK == 0 {
            return !self.uart_active(port)
                || self.physical_tr(port, self.data[port], self.control[port]);
        }
        port < CONTROLLER_PORT_COUNT
            && matches!(value, RECEIVE_IDLE | INPUT_OUTPUT_IDLE)
            && self.uart_idle.history[port] == History::KnownIdleNoTx
            && self.resolved_tr(port)
            && self.physical_tr(port, self.data[port], self.control[port])
            && (value != INPUT_OUTPUT_IDLE
                || now >= OUTPUT_IDLE_SETTLED_TICKS
                    && (self.uart_output_active(port) || self.parallel_tl_high(port))
                    && self.physical_tl(port, self.data[port], self.control[port]))
    }

    pub(super) fn note_transmit(&mut self, port: usize) {
        if port < CONTROLLER_PORT_COUNT {
            self.uart_idle.history[port] = History::PriorTx;
            self.uart_tx.retire_port(port);
        }
    }

    pub(crate) fn uart_reset_blocked(&self) -> bool {
        self.fault().is_some() || (0..CONTROLLER_PORT_COUNT).any(|port| self.uart_active(port))
    }

    pub(crate) fn retire_uart_reset(&mut self) {
        self.uart_idle.history.fill(History::LegacyUnknown);
        self.uart_tx.retire();
    }

    pub(crate) fn set_controller_raw_at(&mut self, port: ControllerPort, value: u8, now: u64) {
        if self.pads[port as usize].buttons == value {
            return;
        }
        self.pads[port as usize].buttons = value;
        self.check_host_change(port, now, InputChange::Buttons);
    }

    pub(crate) fn set_controller_extra_at(&mut self, port: ControllerPort, value: u8, now: u64) {
        if self.pads[port as usize].extra == value & 0x0f {
            return;
        }
        self.pads[port as usize].set_extra(value);
        self.check_host_change(port, now, InputChange::ExtraButtons);
    }

    pub(crate) fn set_controller_kind_at(
        &mut self,
        port: ControllerPort,
        kind: ControllerKind,
        now: u64,
    ) {
        if self.pads[port as usize].kind == kind {
            return;
        }
        self.pads[port as usize].set_kind(kind);
        self.check_host_change(port, now, InputChange::ControllerKind);
    }

    fn check_host_change(&mut self, port: ControllerPort, now: u64, change: InputChange) {
        let index = port as usize;
        if self.uart_idle.fault.is_some() {
            return;
        }
        if self.uart_tx.any_used()
            && !self.uart_tx.used(index)
            && self.serial_control[index] & IO_SERIAL_MODE_MASK == 0
        {
            self.uart_tx.retire_port(index);
            self.uart_idle.history[index] = History::LegacyUnknown;
            return;
        }
        if self.uart_tx.any_used()
            || self.uart_active(index)
                && !self.physical_tr(index, self.data[index], self.control[index])
            || self.uart_output_active(index)
                && !self.physical_tl(index, self.data[index], self.control[index])
        {
            self.uart_idle.fault = Some(InputFault {
                port,
                at: now,
                change,
            });
        }
    }

    pub(super) fn validate_uart(&self, now: u64) -> Result<()> {
        self.uart_tx.validate(now, None)?;
        ensure!(
            self.fault().is_none(),
            "MD pending unsupported input transition"
        );
        for port in 0..CONTROLLER_PORT_COUNT {
            ensure!(
                self.uart_tx.latch_matches(port, self.transmit[port]),
                "MD UART transmit latch mismatch"
            );
            ensure!(
                !self.uart_tx.used(port)
                    || self.serial_control[port] == INPUT_OUTPUT_IDLE
                        && self.uart_idle.history[port] == History::PriorTx
                        && self.uart_tx.receive_high(port),
                "invalid MD UART active owner"
            );
            ensure!(
                self.uart_idle.history[port] != History::KnownIdleNoTx
                    || self.transmit[port] == IO_SERIAL_TX_RESET,
                "MD UART no-TX latch mismatch"
            );
            ensure!(
                self.allow_serial_control(port, self.serial_control[port], now),
                "MD unqualified UART state"
            );
        }
        Ok(())
    }
}
