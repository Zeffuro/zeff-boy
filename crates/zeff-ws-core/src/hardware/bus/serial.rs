use super::*;
use std::collections::VecDeque;

use crate::hardware::constants::CPU_CLOCK_HZ;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WonderSwanTxEvent {
    pub completed_cycle: u64,
    pub byte: u8,
    pub baud_bps: u32,
    pub generation: u64,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Uart {
    rx_data: u8,
    rx_ready: bool,
    overrun: bool,
    tx_data: u8,
    tx_pending: bool,
    tx_cycles_remaining: u32,
    completed_tx_events: VecDeque<WonderSwanTxEvent>,
    tx_generation: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct UartSaveState {
    pub rx_data: u8,
    pub rx_ready: bool,
    pub overrun: bool,
    pub tx_data: u8,
    pub tx_pending: bool,
    pub tx_cycles_remaining: u32,
    pub completed_tx_events: Vec<WonderSwanTxEvent>,
    pub tx_generation: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UartDebugSnapshot {
    pub rx_data: u8,
    pub tx_data: u8,
    pub status: u8,
    pub control: u8,
    pub baud_bps: u32,
    pub tx_cycles_remaining: u32,
    pub completed_tx: Option<u8>,
    pub completed_tx_count: usize,
}

impl Uart {
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    pub(super) fn read_data(&mut self) -> u8 {
        let value = self.rx_data;
        self.rx_ready = false;
        value
    }

    pub(super) fn peek_data(&self) -> u8 {
        self.rx_data
    }

    pub(super) fn write_data(&mut self, value: u8, control: u8) -> bool {
        if control & SERIAL_CONTROL_ENABLE == 0 || self.tx_pending {
            return false;
        }

        self.tx_data = value;
        self.tx_pending = true;
        self.tx_cycles_remaining = byte_cycles(control);
        self.tx_generation = self.tx_generation.wrapping_add(1);
        true
    }

    pub(super) fn write_control(&mut self, value: u8) -> u8 {
        if value & SERIAL_CONTROL_RESET_OVERRUN != 0 {
            self.overrun = false;
        }
        value & (SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD)
    }

    pub(super) fn status(&self, control: u8) -> u8 {
        let mut status = control & (SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD);
        if self.rx_ready {
            status |= SERIAL_STATUS_RX_READY;
        }
        if self.overrun {
            status |= SERIAL_STATUS_OVERRUN;
        }
        if !self.tx_pending {
            status |= SERIAL_STATUS_TX_EMPTY;
        }
        status
    }

    pub(super) fn step_cycles(&mut self, cycles: u32, control: u8, completed_cycle_end: u64) {
        if control & SERIAL_CONTROL_ENABLE == 0 || !self.tx_pending {
            return;
        }
        if cycles < self.tx_cycles_remaining {
            self.tx_cycles_remaining -= cycles;
            return;
        }
        let overshoot_cycles = cycles - self.tx_cycles_remaining;
        self.tx_pending = false;
        self.tx_cycles_remaining = 0;
        self.completed_tx_events.push_back(WonderSwanTxEvent {
            completed_cycle: completed_cycle_end.saturating_sub(u64::from(overshoot_cycles)),
            byte: self.tx_data,
            baud_bps: baud_bps(control),
            generation: self.tx_generation,
        });
    }

    pub(super) fn receive_byte(&mut self, value: u8, control: u8) {
        if control & SERIAL_CONTROL_ENABLE == 0 {
            return;
        }
        if self.rx_ready {
            self.overrun = true;
            return;
        }
        self.rx_data = value;
        self.rx_ready = true;
    }

    pub(super) fn take_completed_tx(&mut self) -> Option<WonderSwanTxEvent> {
        self.completed_tx_events.pop_front()
    }

    pub(super) fn debug_snapshot(&self, control: u8) -> UartDebugSnapshot {
        UartDebugSnapshot {
            rx_data: self.rx_data,
            tx_data: self.tx_data,
            status: self.status(control),
            control: control & (SERIAL_CONTROL_ENABLE | SERIAL_CONTROL_FAST_BAUD),
            baud_bps: baud_bps(control),
            tx_cycles_remaining: self.tx_cycles_remaining,
            completed_tx: self.completed_tx_events.front().map(|event| event.byte),
            completed_tx_count: self.completed_tx_events.len(),
        }
    }

    pub(super) fn save_state(&self) -> UartSaveState {
        UartSaveState {
            rx_data: self.rx_data,
            rx_ready: self.rx_ready,
            overrun: self.overrun,
            tx_data: self.tx_data,
            tx_pending: self.tx_pending,
            tx_cycles_remaining: self.tx_cycles_remaining,
            completed_tx_events: self.completed_tx_events.iter().copied().collect(),
            tx_generation: self.tx_generation,
        }
    }

    pub(super) fn load_state(&mut self, state: UartSaveState) {
        self.rx_data = state.rx_data;
        self.rx_ready = state.rx_ready;
        self.overrun = state.overrun;
        self.tx_data = state.tx_data;
        self.tx_pending = state.tx_pending;
        self.tx_cycles_remaining = state.tx_cycles_remaining;
        self.completed_tx_events = state.completed_tx_events.into();
        self.tx_generation = state.tx_generation;
    }
}

fn baud_bps(control: u8) -> u32 {
    if control & SERIAL_CONTROL_FAST_BAUD != 0 {
        38_400
    } else {
        9_600
    }
}

fn byte_cycles(control: u8) -> u32 {
    CPU_CLOCK_HZ / baud_bps(control) * 10
}
