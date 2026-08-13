use std::fmt;
use std::io::{self, Write};

use super::bus::{GameBoyLinkAction, GameBoyLinkReply, GameBoyLinkState};
use crate::hardware::types::hardware_mode::HardwareMode;
use crate::save_state::{StateReader, StateReaderGbExt, StateWriter, StateWriterGbExt};
use anyhow::Result;

pub trait SerialDevice {
    fn exchange_byte(&mut self, byte: u8) -> u8;
}

#[allow(dead_code)]
pub struct DisconnectedDevice;

impl SerialDevice for DisconnectedDevice {
    fn exchange_byte(&mut self, _byte: u8) -> u8 {
        0xFF
    }
}

pub(super) struct Serial {
    sb: u8,
    sc: u8,
    cycles: u64,
    mode: HardwareMode,
    output_log: Vec<u8>,
    link_peer_present: bool,
    pending_link_byte: Option<u8>,
    pending_link_response: Option<u8>,
    pending_link_completion_ready: bool,
    queued_link_action: Option<GameBoyLinkAction>,
    serial_generation: u64,
}

impl fmt::Debug for Serial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Serial")
            .field("sb", &format_args!("{:#04X}", self.sb))
            .field("sc", &format_args!("{:#04X}", self.sc))
            .field("cycles", &self.cycles)
            .field("mode", &self.mode)
            .field("output_log_len", &self.output_log.len())
            .field("link_peer_present", &self.link_peer_present)
            .field("pending_link_byte", &self.pending_link_byte)
            .field("pending_link_response", &self.pending_link_response)
            .field(
                "pending_link_completion_ready",
                &self.pending_link_completion_ready,
            )
            .field("queued_link_action", &self.queued_link_action)
            .field("serial_generation", &self.serial_generation)
            .finish()
    }
}

impl Serial {
    pub(super) fn new() -> Self {
        Self {
            sb: 0,
            sc: 0,
            cycles: 0,
            mode: HardwareMode::DMG,
            output_log: Vec::new(),
            link_peer_present: false,
            pending_link_byte: None,
            pending_link_response: None,
            pending_link_completion_ready: false,
            queued_link_action: None,
            serial_generation: 0,
        }
    }

    fn transfer_period(&self) -> u64 {
        let cgb_mode = matches!(self.mode, HardwareMode::CGBNormal | HardwareMode::CGBDouble);
        let fast_serial = cgb_mode && (self.sc & 0x02) != 0;
        match (self.mode, fast_serial) {
            (HardwareMode::CGBDouble, false) => 2048,
            (HardwareMode::CGBDouble, true) => 64,
            (_, false) => 4096,
            (_, true) => 128,
        }
    }

    pub(super) fn output_bytes(&self) -> &[u8] {
        &self.output_log
    }

    pub(super) fn sb(&self) -> u8 {
        self.sb
    }

    pub(super) fn sc(&self) -> u8 {
        let read_mask = if matches!(self.mode, HardwareMode::CGBNormal | HardwareMode::CGBDouble) {
            0x7C
        } else {
            0x7E
        };
        self.sc | read_mask
    }

    pub(super) fn write_sb(&mut self, value: u8) {
        self.sb = value;
        self.bump_serial_generation();
    }

    pub(super) fn write_sc(&mut self, value: u8) {
        let was_internal_active = (self.sc & 0x81) == 0x81;
        self.sc = value;
        self.bump_serial_generation();
        if (self.sc & 0x80) == 0 {
            self.pending_link_byte = None;
            self.pending_link_response = None;
            self.pending_link_completion_ready = false;
            self.queued_link_action = None;
            return;
        }

        if self.link_peer_present
            && !was_internal_active
            && self.pending_link_byte.is_none()
            && (self.sc & 0x81) == 0x81
        {
            let action = GameBoyLinkAction {
                out_byte: self.sb,
                clock_period_t_cycles: self.transfer_period(),
                serial_generation: self.serial_generation,
            };
            self.pending_link_byte = Some(action.out_byte);
            self.pending_link_response = None;
            self.pending_link_completion_ready = false;
            self.queued_link_action = Some(action);
        }
    }

    pub(super) fn set_clock_phase(&mut self, cycles: u64) {
        self.cycles = cycles % self.transfer_period();
    }

    pub(super) fn set_mode(&mut self, mode: HardwareMode) {
        self.mode = mode;
    }

    pub(super) fn reset_cycles(&mut self) {
        self.cycles = 0;
    }

    pub(super) fn set_link_peer_present(&mut self, present: bool) {
        let was_present = self.link_peer_present;
        self.link_peer_present = present;
        if !present {
            self.pending_link_byte = None;
            self.pending_link_response = None;
            self.pending_link_completion_ready = false;
            self.queued_link_action = None;
        } else if !was_present && self.pending_link_byte.is_none() && (self.sc & 0x81) == 0x81 {
            let action = GameBoyLinkAction {
                out_byte: self.sb,
                clock_period_t_cycles: self.transfer_period(),
                serial_generation: self.serial_generation,
            };
            self.pending_link_byte = Some(action.out_byte);
            self.pending_link_response = None;
            self.pending_link_completion_ready = false;
            self.queued_link_action = Some(action);
        }
    }

    pub(super) fn pending_link_byte(&self) -> Option<u8> {
        self.pending_link_byte
    }

    pub(super) fn pending_link_response(&self) -> Option<u8> {
        self.pending_link_response
    }

    pub(super) fn waiting_at_link_completion_boundary(&self) -> bool {
        self.pending_link_byte.is_some()
            && self.pending_link_completion_ready
            && self.pending_link_response.is_none()
    }

    pub(super) fn take_link_action(&mut self) -> Option<GameBoyLinkAction> {
        self.queued_link_action.take()
    }

    pub(super) fn link_state(&self) -> GameBoyLinkState {
        GameBoyLinkState {
            pending_master_byte: self.pending_link_byte,
            external_clock_byte: self
                .external_clock_transfer_active()
                .then_some(self.link_transfer_byte()),
            output_byte: self.link_transfer_byte(),
        }
    }

    pub(super) fn external_clock_transfer_active(&self) -> bool {
        self.pending_link_byte.is_none() && (self.sc & 0x81) == 0x80
    }

    pub(super) fn link_transfer_byte(&self) -> u8 {
        self.sb
    }

    pub(super) fn complete_link_transfer(&mut self, response: u8) -> bool {
        if self.pending_link_byte.take().is_none() && !self.external_clock_transfer_active() {
            return false;
        }

        self.sb = response;
        self.sc &= !0x80;
        self.pending_link_response = None;
        self.pending_link_completion_ready = false;
        self.queued_link_action = None;
        self.bump_serial_generation();
        true
    }

    pub(super) fn apply_link_reply(&mut self, reply: GameBoyLinkReply) -> bool {
        if self.pending_link_byte.is_none() {
            return false;
        }

        if self.pending_link_completion_ready {
            return self.complete_link_transfer(reply.out_byte);
        }

        self.pending_link_response = Some(reply.out_byte);
        true
    }

    pub(super) fn reply_to_master_start(&self) -> GameBoyLinkReply {
        GameBoyLinkReply {
            out_byte: self.link_transfer_byte(),
            passive: self.external_clock_transfer_active(),
            serial_generation: self.serial_generation,
        }
    }

    pub(super) fn complete_external_from_master(&mut self, peer_byte: u8) -> bool {
        if !self.external_clock_transfer_active() {
            return false;
        }

        self.complete_link_transfer(peer_byte)
    }

    pub(super) fn apply_remote_link_peer_state(
        &mut self,
        peer: GameBoyLinkState,
        idle_master_response: Option<u8>,
    ) -> bool {
        if let Some(local_byte) = self.pending_link_byte() {
            let response = peer
                .pending_master_byte
                .or(peer.external_clock_byte)
                .unwrap_or_else(|| idle_master_response.unwrap_or(peer.output_byte));
            debug_assert_eq!(self.pending_link_byte(), Some(local_byte));
            return self.complete_link_transfer(response);
        }

        if self.external_clock_transfer_active()
            && let Some(peer_byte) = peer.pending_master_byte
        {
            return self.complete_link_transfer(peer_byte);
        }

        false
    }

    pub(super) fn step(&mut self, cycles: u64, device: &mut dyn SerialDevice) -> bool {
        let transfer_period = self.transfer_period();
        let active = self.sc & 0x81 == 0x81;
        let previous_cycles = self.cycles;
        self.cycles = self.cycles.wrapping_add(cycles) % transfer_period;
        let crossed_period = previous_cycles + cycles >= transfer_period;

        if active && crossed_period {
            if let Some(response) = self.pending_link_response {
                return self.complete_link_transfer(response);
            }

            if self.link_peer_present && self.pending_link_byte.is_some() {
                self.pending_link_completion_ready = true;
                return false;
            }

            let response = device.exchange_byte(self.sb);
            self.output_log.push(self.sb);
            print!("{}", self.sb as char);
            let _ = io::stdout().flush();

            self.sb = response;
            self.sc &= !0x80;
            self.bump_serial_generation();
            return true;
        }

        false
    }

    fn bump_serial_generation(&mut self) {
        self.serial_generation = self.serial_generation.wrapping_add(1);
    }

    pub(super) fn write_state(&self, writer: &mut StateWriter) {
        writer.write_u8(self.sb);
        writer.write_u8(self.sc);
        writer.write_u64(self.cycles);
        writer.write_hardware_mode(self.mode);
    }

    pub(super) fn read_state(reader: &mut StateReader<'_>) -> Result<Self> {
        Ok(Self {
            sb: reader.read_u8()?,
            sc: reader.read_u8()?,
            cycles: reader.read_u64()?,
            mode: reader.read_hardware_mode()?,
            output_log: Vec::new(),
            link_peer_present: false,
            pending_link_byte: None,
            pending_link_response: None,
            pending_link_completion_ready: false,
            queued_link_action: None,
            serial_generation: 0,
        })
    }
}

#[cfg(test)]
mod tests;
