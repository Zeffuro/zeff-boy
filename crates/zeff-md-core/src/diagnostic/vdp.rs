use super::{Callback, observe};
use crate::constants::{
    VDP_CONTROL_END, VDP_CONTROL_START, VDP_DECODE_MASK, VDP_DECODE_VALUE, VDP_PORT_MASK,
};
use std::marker::PhantomData;
use std::rc::Rc;
use zeff_m68k::bus::ByteLanes;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisterPending {
    pub index: u8,
    pub value: u8,
    pub publish_at: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VdpContext {
    pub ticks: u64,
    pub register_ticks: u64,
    pub publication_epoch: u64,
    pub registers: [Option<u8>; 24],
    pub pending: Option<RegisterPending>,
    pub read_phase: Option<bool>,
    pub fifo_len: usize,
    pub control_pending_word: Option<u16>,
    pub dma_active: bool,
    pub timing_history: bool,
    pub normal_interlace_history: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectionSite {
    PortReadClock,
    ReadPhase,
    Fifo,
    UnsupportedMode,
    InterlaceHistory,
    ClockInterlace,
    ClockPublicationWindow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RegisterRejection {
    pub site: RejectionSite,
    pub index: u8,
    pub value: u8,
    pub attempted_publish_at: Option<u64>,
    pub context: VdpContext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ControlWriteContext {
    pub word: u16,
    pub before: VdpContext,
    pub rejection: Option<RegisterRejection>,
}

pub(crate) struct ControlScope {
    generation: u64,
    sequence: u64,
    _owner_thread: PhantomData<Rc<()>>,
}

impl ControlScope {
    pub(crate) fn begin(word: u16, before: VdpContext) -> Option<Self> {
        observe(|recorder| {
            let record = recorder.in_flight.as_mut()?;
            let access = record.access?;
            let service = record.service?;
            if record.callback != Callback::Write
                || access.bus_address & VDP_DECODE_MASK != VDP_DECODE_VALUE
                || !(VDP_CONTROL_START..=VDP_CONTROL_END)
                    .contains(&(access.bus_address & VDP_PORT_MASK))
                || service.vdp_ticks != before.ticks
            {
                return None;
            }
            let [high, low] = record.write_value?.to_be_bytes();
            let expected = match access.lanes {
                ByteLanes::Both => u16::from_be_bytes([high, low]),
                ByteLanes::Upper => u16::from_be_bytes([high; 2]),
                ByteLanes::Lower => u16::from_be_bytes([low; 2]),
            };
            if word != expected {
                recorder.complete = false;
                return None;
            }
            if recorder.vdp_scope.is_some() || record.vdp_control.is_some() {
                recorder.complete = false;
                return None;
            }
            record.vdp_control = Some(ControlWriteContext {
                word,
                before,
                rejection: None,
            });
            recorder.vdp_scope = Some(record.sequence);
            Some(Self {
                generation: recorder.generation,
                sequence: record.sequence,
                _owner_thread: PhantomData,
            })
        })
        .flatten()
    }

    pub(crate) fn reject(rejection: RegisterRejection) {
        observe(|recorder| {
            let Some(sequence) = recorder.vdp_scope else {
                return;
            };
            let Some(record) = recorder
                .in_flight
                .as_mut()
                .filter(|record| record.sequence == sequence)
            else {
                recorder.complete = false;
                return;
            };
            let Some(control) = record.vdp_control.as_mut() else {
                recorder.complete = false;
                return;
            };
            if control.word & 0xc000 != 0x8000
                || ((control.word >> 8) & 0x1f) != u16::from(rejection.index)
                || control.word as u8 != rejection.value
            {
                recorder.complete = false;
                return;
            }
            if control.rejection.replace(rejection).is_some() {
                recorder.complete = false;
            }
        });
    }
}

impl Drop for ControlScope {
    fn drop(&mut self) {
        observe(|recorder| {
            if recorder.generation != self.generation {
                return;
            }
            if recorder.vdp_scope != Some(self.sequence) {
                recorder.complete = false;
            } else {
                recorder.vdp_scope = None;
            }
        });
    }
}
