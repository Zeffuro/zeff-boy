mod arbiter;
mod grant;
mod instruction_origin;
mod logic;
mod memory;
pub(crate) mod previous;
mod protocol;
mod psg;
mod resolve;
mod sound;
mod sound_source;
use crate::sound_trace::physical::HalfCapture;
#[cfg(test)]
use crate::sound_trace::physical::vdp::raw::{
    Driver as VdpDriver, Event as VdpRaw, Pins as VdpPins,
};
use crate::sound_trace::physical::vdp::{
    Event as VdpEvent, Mux as VdpMux, Slot as VdpSlot, Source as VdpSource,
};
mod state;
#[cfg(test)]
pub(crate) mod work_witness;

use super::domain::{BoardBoundary, BoardInput, BoardView, Effect, Storage};
use super::{main_bus, wiring};
use crate::hardware::vdp::state::{StateReader, StateWriter};
use anyhow::{Result, ensure};
pub(crate) use protocol::{
    Completion, ProtocolCompletion, ProtocolDrive, ProtocolRequest, ProtocolTarget,
};
#[cfg(test)]
pub(crate) use psg::PsgBinding;
pub(crate) use psg::PsgWrite;
pub(crate) use sound::{SoundPins, SoundResponse};
use zeff_m68k::bus::{ByteLanes, FunctionCode};
use zeff_m68k::timing::InstructionByteOrigin;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Companion {
    pub half: u64,
    pub pass: u8,
    pub pins: SoundPins,
    pub bank: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Outputs {
    request_level: bool,
    acknowledge_level: bool,
    grant_acknowledge_level: bool,
    vpa_level: bool,
    cart_enable_level: bool,
    strobe_direction: bool,
    read_direction: bool,
    address_strobe_level: bool,
    upper_level: bool,
    lower_level: bool,
    low_address_direction: bool,
    high_address_direction: bool,
}

impl Outputs {
    const fn zero() -> Self {
        Self {
            request_level: false,
            acknowledge_level: false,
            grant_acknowledge_level: false,
            vpa_level: false,
            cart_enable_level: false,
            strobe_direction: false,
            read_direction: false,
            address_strobe_level: false,
            upper_level: false,
            lower_level: false,
            low_address_direction: false,
            high_address_direction: false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stage {
    preview: BoardView,
    arbiter: arbiter::Inputs,
    pins: main_bus::Pins,
    data_mask: u16,
    companion: Companion,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Board {
    half: u64,
    next_pass: u8,
    clock_high: bool,
    main: Option<main_bus::MainBus>,
    grant: grant::Grant,
    arbiter: arbiter::Arbiter,
    outputs: Outputs,
    address: u32,
    data: u16,
    data_mask: u16,
    instruction_origin: instruction_origin::Metadata,
    held_controls: [bool; 3],
    companion: Option<Companion>,
    last_companion: Option<Companion>,
    sound: sound::SoundState,
    psg: psg::Psg,
    stage: Option<Stage>,
    protocol: Option<protocol::Binding>,
    protocol_seen: bool,
    protocol_drive: Option<ProtocolDrive>,
}

impl Board {
    pub fn software_origin() -> Self {
        Self {
            half: 0,
            next_pass: 0,
            clock_high: false,
            main: None,
            grant: grant::Grant::software_origin(),
            arbiter: arbiter::Arbiter::software_origin(),
            outputs: Outputs::zero(),
            address: 0,
            data: 0,
            data_mask: 0,
            instruction_origin: Default::default(),
            held_controls: [false; 3],
            companion: None,
            last_companion: None,
            sound: sound::SoundState::origin(),
            psg: psg::Psg::origin(),
            stage: None,
            protocol: None,
            protocol_seen: false,
            protocol_drive: None,
        }
    }

    pub fn qualify_companion(&mut self, companion: Companion) -> Result<()> {
        ensure!(
            self.stage.is_none() && self.companion.is_none(),
            "MAIN companion qualification already pending"
        );
        self.validate_sound_continuation(companion)?;
        let half = if self.next_pass == 0 {
            self.half.checked_add(1)
        } else {
            Some(self.half)
        };
        ensure!(
            Some(companion.half) == half && companion.pass == self.next_pass,
            "stale MAIN companion qualification"
        );
        self.companion = Some(companion);
        Ok(())
    }

    pub fn ready_tick(&self) -> Option<u64> {
        if self.next_pass != 0
            || self.stage.is_some()
            || self.grant.tristate()
            || !self.main_reset_released()
        {
            return None;
        }
        self.main.as_ref()?.ready_tick()
    }

    pub fn main_reset_released(&self) -> bool {
        self.arbiter.main_reset_released()
    }

    pub fn offer(&mut self, request: main_bus::Request) -> Result<()> {
        ensure!(self.ready_tick().is_some(), "MAIN board is not ready");
        self.validate_owner(request)?;
        self.main
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("MAIN clock prefix is incomplete"))?
            .offer(request)
            .map_err(|error| anyhow::anyhow!("invalid MAIN request: {error:?}"))?;
        self.instruction_origin.0.clear_sample();
        Ok(())
    }

    pub fn take_completed(&mut self) -> Result<Option<Completion>> {
        ensure!(
            self.next_pass == 0 && self.stage.is_none(),
            "MAIN completion inside a board pass"
        );
        let Some(main) = self
            .main
            .as_mut()
            .and_then(main_bus::MainBus::take_completed)
        else {
            return Ok(None);
        };
        let protocol = self.protocol.take().map(|binding| {
            let binding = ProtocolRequest {
                request: main.request,
                start_half: main.start_half,
                target: binding.target,
            };
            match main.request.operation {
                main_bus::Operation::Write(word) => ProtocolCompletion::Write { binding, word },
                _ => ProtocolCompletion::Read(binding),
            }
        });
        self.protocol_seen = false;
        self.protocol_drive = None;
        let origin = self.instruction_origin.0.take(main);
        Ok(Some(Completion {
            main,
            protocol,
            origin,
        }))
    }

    pub fn snapshot(&self) -> Option<main_bus::Snapshot> {
        self.main.as_ref().map(main_bus::MainBus::snapshot)
    }

    pub fn half(&self) -> u64 {
        self.half
    }

    pub fn observed_clocks(&self) -> [bool; 2] {
        self.arbiter.observed_clocks()
    }
    pub fn sound_companion(&self) -> Option<Companion> {
        self.last_companion
    }
    pub fn machine_boundary_ready(&self) -> bool {
        self.next_pass == 0
            && self.stage.is_none()
            && self.companion.is_none()
            && self.protocol.is_none()
            && !self.protocol_seen
            && self.protocol_drive.is_none()
    }

    fn validate_owner(&self, request: main_bus::Request) -> Result<()> {
        let address = request.access.bus_address;
        let fixed_memory = address <= 0x3ffffe || address >= 0xe00000;
        let vdp = address & 0xe700e0 == 0xc00000;
        ensure!(
            fixed_memory || vdp,
            "MAIN peripheral data/termination owner is absent"
        );
        if vdp {
            let port = (address >> 1) & 12;
            ensure!(
                port == 0 || port == 4 && request.operation == main_bus::Operation::Read,
                "MAIN VDP test/PSG or unsupported port direction"
            );
        }
        Ok(())
    }

    fn idle_pins() -> main_bus::Pins {
        main_bus::Pins {
            function_code: FunctionCode::SupervisorProgram,
            address: None,
            read: true,
            address_strobe: false,
            upper_strobe: false,
            lower_strobe: false,
            write_word: None,
            write_lanes: None,
            interrupt_acknowledge: false,
        }
    }

    pub(crate) fn finish_inner(
        &mut self,
        view: BoardView,
        storage: &impl Storage,
    ) -> Result<Vec<Effect>> {
        self.finish_inner_capturing(view, storage, &mut ())
    }

    pub(crate) fn finish_inner_capturing<P: previous::TagSink>(
        &mut self,
        view: BoardView,
        storage: &impl Storage,
        previous: &mut P,
    ) -> Result<Vec<Effect>> {
        self.finish_inner_physical(view, storage, previous, &mut ())
    }

    pub(crate) fn finish_inner_physical<P: previous::TagSink, H: HalfCapture>(
        &mut self,
        view: BoardView,
        storage: &impl Storage,
        previous: &mut P,
        physical: &mut H,
    ) -> Result<Vec<Effect>> {
        self.finish_inner_recording(
            view,
            storage,
            previous,
            physical,
            #[cfg(test)]
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn finish_inner_with_work_witness<P: previous::TagSink, H: HalfCapture>(
        &mut self,
        view: BoardView,
        storage: &impl Storage,
        previous: &mut P,
        witness: &mut Option<work_witness::Pulse>,
        physical: &mut H,
    ) -> Result<Vec<Effect>> {
        self.finish_inner_recording(view, storage, previous, physical, Some(witness))
    }

    fn finish_inner_recording<P: previous::TagSink, H: HalfCapture>(
        &mut self,
        view: BoardView,
        storage: &impl Storage,
        previous: &mut P,
        physical: &mut H,
        #[cfg(test)] witness: Option<&mut Option<work_witness::Pulse>>,
    ) -> Result<Vec<Effect>> {
        let stage = self
            .stage
            .ok_or_else(|| anyhow::anyhow!("MAIN postpass without resolve"))?;
        ensure!(
            stage.preview.half == view.half && stage.preview.pass == view.pass,
            "stale MAIN postpass"
        );
        self.outputs = Outputs {
            request_level: self.arbiter.br_level && !stage.preview.bus_request,
            acknowledge_level: self.arbiter.dtack_level && !stage.preview.drives.levels.acknowledge,
            grant_acknowledge_level: self.arbiter.bgack_level
                && !stage.preview.bus_grant_acknowledge,
            vpa_level: self.arbiter.vpa_level,
            cart_enable_level: self.arbiter.ce_level,
            strobe_direction: self.arbiter.strobe_direction,
            read_direction: self.arbiter.read_direction,
            address_strobe_level: self.arbiter.as_level,
            upper_level: self.arbiter.upper_level,
            lower_level: self.arbiter.lower_level,
            low_address_direction: self.arbiter.low_address_direction(),
            high_address_direction: self.arbiter.high_address_direction(),
        };
        physical.vdp(VdpEvent::Outputs);
        #[cfg(test)]
        physical.vdp_raw(VdpRaw::Outputs {
            levels: [
                self.outputs.address_strobe_level,
                self.outputs.upper_level,
                self.outputs.lower_level,
            ],
        });
        let cycle = view
            .half
            .checked_add(3)
            .ok_or_else(|| anyhow::anyhow!("MAIN arbiter clock overflow"))?;
        self.observe_psg(stage, view)?;
        if H::ENABLED {
            self.arbiter
                .evaluate_physical(cycle, stage.arbiter, physical)?;
        } else {
            self.arbiter.evaluate(cycle, stage.arbiter)?;
        }
        let effects = self.respond_memory(
            (stage, view),
            storage,
            previous,
            physical,
            #[cfg(test)]
            witness,
        )?;
        self.stage = None;
        self.next_pass = if view.pass == 0 { 1 } else { 0 };
        self.last_companion = self.companion.take();
        self.protocol_drive = None;
        Ok(effects)
    }
}

impl BoardBoundary for Board {
    fn resolve(&mut self, view: BoardView) -> Result<BoardInput> {
        let mut candidate = self.clone();
        let input = candidate.resolve_inner(view)?;
        *self = candidate;
        Ok(input)
    }

    fn finish_pass(&mut self, view: BoardView, storage: &impl Storage) -> Result<Vec<Effect>> {
        let mut candidate = self.clone();
        let effects = candidate.finish_inner(view, storage)?;
        *self = candidate;
        Ok(effects)
    }
}
