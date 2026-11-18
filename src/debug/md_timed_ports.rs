use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::debug::DebugUiActions;
use crate::emu_thread::{MdTimedPortReply, MdTimedPortRequest};
use zeff_md_core::emulator::DebugPortWrite;

mod view;
pub(super) use view::draw;

static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MdTimedPortView {
    pub(crate) can_submit: bool,
    pub(crate) sound_requested: bool,
    pub(crate) sound_granted: bool,
    pub(crate) sound_reset_asserted: bool,
    pub(crate) save: Option<zeff_md_core::emulator::DebugSavePorts>,
}

#[derive(Default)]
pub(crate) struct MdTimedPortUi {
    pub(super) data: String,
    pub(super) control: String,
    pub(super) psg: String,
    pub(super) sound_address: String,
    pub(super) sound_data: String,
    pub(super) uart_address: String,
    pub(super) uart_data: String,
    pub(super) save_address: String,
    pub(super) save_data: String,
    pub(super) request_sound: bool,
    pub(super) release_reset: bool,
    pub(super) draft: Vec<DebugPortWrite>,
    pub(super) notice: Option<String>,
    queued: Option<MdTimedPortRequest>,
    in_flight: Option<u64>,
    reply: Option<MdTimedPortReply>,
    deferred: VecDeque<DebugUiActions>,
    ordinary_flush: bool,
    ordinary_in_flight: bool,
}

impl MdTimedPortUi {
    pub(crate) fn busy(&self) -> bool {
        self.queued.is_some() || self.in_flight.is_some()
    }

    pub(crate) fn can_queue(&self) -> bool {
        !self.busy() && self.deferred.is_empty() && !self.ordinary_flush && !self.ordinary_in_flight
    }

    pub(super) fn word(&mut self, address: u32, text: &str) -> Result<(), String> {
        let value = crate::debug::console::parse_hex(text.trim())
            .and_then(|value| u16::try_from(value).ok())
            .ok_or_else(|| "Enter a full hexadecimal word from 0000 to FFFF".to_string())?;
        self.draft.push(DebugPortWrite::Word { address, value });
        Ok(())
    }

    pub(super) fn psg_byte(&mut self, text: &str) -> Result<(), String> {
        let value = crate::debug::console::parse_hex(text.trim())
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| "Enter a hexadecimal byte from 00 to FF".to_string())?;
        self.draft.push(DebugPortWrite::Byte {
            address: 0xc00011,
            value,
        });
        Ok(())
    }

    pub(super) fn sound_byte(&mut self, address: &str, text: &str) -> Result<(), String> {
        let address = crate::debug::console::parse_hex(address.trim())
            .and_then(|address| u32::try_from(address).ok())
            .filter(
                |&address| matches!(address, 0xa00000..=0xa01fff | 0xa04000..=0xa04003 | 0xa06000),
            )
            .ok_or_else(|| "Use A00000–A01FFF RAM, A04000–A04003 YM, or A06000 bank".to_string())?;
        let value = crate::debug::console::parse_hex(text.trim())
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| "Enter a hexadecimal byte from 00 to FF".to_string())?;
        self.draft.push(DebugPortWrite::Byte { address, value });
        Ok(())
    }

    pub(super) fn uart_byte(&mut self, address: &str, text: &str) -> Result<(), String> {
        let address = crate::debug::console::parse_hex(address.trim())
            .and_then(|address| u32::try_from(address).ok())
            .filter(|&address| matches!(address, 0xa1000f | 0xa10013 | 0xa10015 | 0xa10019))
            .ok_or_else(|| "Use A1000F/A10015 TX or A10013/A10019 control".to_string())?;
        let value = crate::debug::console::parse_hex(text.trim())
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| "Enter a hexadecimal byte from 00 to FF".to_string())?;
        self.draft.push(DebugPortWrite::Byte { address, value });
        Ok(())
    }

    pub(super) fn save_byte(
        &mut self,
        ports: Option<zeff_md_core::emulator::DebugSavePorts>,
        address: &str,
        text: &str,
    ) -> Result<(), String> {
        let ports =
            ports.ok_or_else(|| "This cartridge has no admitted timed SRAM owner".to_string())?;
        let address = crate::debug::console::parse_hex(address.trim())
            .and_then(|value| u32::try_from(value).ok())
            .filter(|&address| ports.accepts_byte(address))
            .ok_or_else(|| {
                "Use the displayed SRAM byte window or its canonical control byte".to_string()
            })?;
        let value = crate::debug::console::parse_hex(text.trim())
            .and_then(|value| u8::try_from(value).ok())
            .ok_or_else(|| "Enter a hexadecimal byte from 00 to FF".to_string())?;
        self.draft.push(DebugPortWrite::Byte { address, value });
        Ok(())
    }

    pub(super) fn prepare(&self) -> Result<MdTimedPortRequest, String> {
        if !self.can_queue() || self.draft.is_empty() {
            return Err("Finish the pending request before submitting another batch".into());
        }
        let id = NEXT_REQUEST
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .map_err(|_| "Timed-port request identifiers are exhausted".to_string())?;
        Ok(MdTimedPortRequest {
            id,
            writes: self.draft.clone(),
        })
    }

    pub(crate) fn queue(&mut self, request: MdTimedPortRequest) -> Result<(), String> {
        if !self.can_queue() {
            return Err("A timed-port request or deferred action is already pending".into());
        }
        self.queued = Some(request);
        self.ordinary_flush = true;
        self.reply = None;
        self.notice = None;
        Ok(())
    }

    pub(crate) fn queued(&self) -> Option<MdTimedPortRequest> {
        self.queued.clone()
    }

    pub(crate) fn sent(&mut self, id: u64) {
        if self.queued.as_ref().is_some_and(|request| request.id == id) {
            self.queued = None;
            self.in_flight = Some(id);
        }
    }

    pub(crate) fn receive(&mut self, reply: MdTimedPortReply) -> bool {
        if self.in_flight != Some(reply.id) {
            return false;
        }
        self.in_flight = None;
        self.reply = Some(reply);
        self.notice = None;
        true
    }

    pub(crate) fn send_failed(&mut self, reason: String) {
        self.queued = None;
        self.in_flight = None;
        self.notice = Some(reason);
    }

    pub(crate) fn refused(&mut self, reason: String) {
        self.notice = Some(reason);
    }

    pub(crate) fn cancel_session(&mut self, reason: &str) {
        self.queued = None;
        self.in_flight = None;
        self.reply = None;
        self.deferred.clear();
        self.ordinary_flush = false;
        self.ordinary_in_flight = false;
        self.draft.clear();
        self.notice = Some(reason.into());
    }

    pub(crate) fn defer(&mut self, actions: DebugUiActions) {
        self.deferred.push_back(actions);
    }

    pub(crate) fn take_deferred(&mut self) -> Option<DebugUiActions> {
        if self.busy() || self.ordinary_flush || self.ordinary_in_flight {
            return None;
        }
        let actions = self.deferred.pop_front()?;
        self.ordinary_flush = !self.deferred.is_empty();
        Some(actions)
    }

    pub(crate) fn ordinary_flush_required(&self) -> bool {
        self.ordinary_flush && !self.busy()
    }

    pub(crate) fn ordinary_flush_sent(&mut self) {
        if self.ordinary_flush_required() {
            self.ordinary_flush = false;
            self.ordinary_in_flight = true;
        }
    }

    pub(crate) fn ordinary_delivery_pending(&self) -> bool {
        self.ordinary_in_flight
    }

    pub(crate) fn ordinary_delivery(&mut self) {
        self.ordinary_in_flight = false;
    }

    pub(crate) fn ordering_pending(&self) -> bool {
        self.busy() || self.ordinary_flush || self.ordinary_in_flight || self.has_deferred()
    }

    pub(crate) fn has_deferred(&self) -> bool {
        !self.deferred.is_empty()
    }

    pub(super) fn status(&self) -> String {
        if let Some(request) = &self.queued {
            return format!("Request #{} queued", request.id);
        }
        if let Some(id) = self.in_flight {
            return format!("Request #{id} pending");
        }
        if let Some(reply) = &self.reply {
            let mut status = match &reply.result {
                Ok(outcome) => format!(
                    "#{}: {} writes, {} CPU clocks, {} MCLK, {} instructions, {} fields; {} PCM samples delivered",
                    reply.id,
                    outcome.writes,
                    outcome.cpu_clocks,
                    outcome.master_ticks,
                    outcome.instructions,
                    outcome.fields_crossed,
                    reply.delivered_pcm_samples,
                ),
                Err(error) => format!(
                    "#{} {:?}{}: {}",
                    reply.id,
                    error.kind,
                    error
                        .index
                        .map_or(String::new(), |index| format!(" at write {index}")),
                    error.reason
                ),
            };
            if self.ordinary_flush || self.ordinary_in_flight {
                status.push_str("; waiting for ordinary delivery");
            }
            return status;
        }
        self.notice
            .clone()
            .unwrap_or_else(|| "No timed-port request submitted".into())
    }
}

#[cfg(test)]
mod tests;
