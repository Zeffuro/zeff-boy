mod rollback_memory;
use super::*;
use crate::hardware::vdp::owner::sound_cpu::{AudioEffect, SoundEvent, SoundOwner};
use crate::hardware::vdp::owner::{
    Board, Companion, ProtocolCompletion, ProtocolDrive, ProtocolTarget, main_bus,
};
use crate::sound_trace::physical::{HalfCapture, PhysicalCapture};
use anyhow::{Result, ensure};
use zeff_m68k::timing::{
    InstructionByteOrigin, InstructionInput, InterruptAperture, ObservedCycle,
};

mod boundary;
mod checkpoint;
#[cfg(test)]
mod dma_activity_tests;
mod events;
#[cfg(test)]
mod fifo_witness;
#[cfg(test)]
mod physical_trace_tests;
mod previous;
mod protocol;
mod state;
#[cfg(test)]
pub(crate) mod uart_tx_cases;
#[cfg(test)]
mod work_witness;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OwnedState {
    board: Board,
    sound: SoundOwner,
    sound_event: Option<SoundEvent>,
    audio_pending: Vec<AudioEffect>,
    epoch_tick: u64,
    idle_clocks: u64,
    fault: Option<String>,
    transfers: u64,
}

pub(crate) struct Context<'a, C = ()> {
    physical: C,
    region: Region,
    state: &'a mut OwnedState,
    cartridge: &'a mut Cartridge,
    work: &'a mut [u8],
    vdp: &'a mut Vdp,
    input: &'a mut Input,
    audio: &'a mut Audio,
    prefetch: &'a mut Prefetch,
    clock: &'a mut CpuClocks,
    now: &'a mut MasterTicks,
    clock_fault: &'a mut bool,
    video: &'a mut row_delivery::RowDelivery,
    #[cfg(test)]
    fifo_witness: Option<fifo_witness::Witness>,
    #[cfg(test)]
    work_witness: Option<work_witness::Witness>,
}

struct Transfer {
    main: main_bus::Receipt,
    wait_clocks: u32,
    origin: [InstructionByteOrigin; 2],
}

#[derive(Debug)]
struct GrantTimeout {
    wait_clocks: u32,
}

impl std::fmt::Display for GrantTimeout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("MAIN bus grant was not released")
    }
}

impl std::error::Error for GrantTimeout {}

impl OwnedState {
    pub fn software_origin() -> Self {
        Self {
            board: Board::software_origin(),
            sound: SoundOwner::software_origin(),
            sound_event: None,
            audio_pending: Vec::new(),
            epoch_tick: 0,
            idle_clocks: 0,
            fault: None,
            transfers: 0,
        }
    }
}

impl Bus {
    pub(crate) fn new_owned(cartridge: Cartridge, region: Region) -> Result<Self> {
        validate_cartridge(&cartridge)?;
        let mut bus = Self::new(cartridge, region);
        bus.vdp = Vdp::software_origin_owned(region);
        bus.owned = Some(Box::new(OwnedState::software_origin()));
        bus.video.begin_owned();
        bus.owned_context().expect("fresh owned bus").initialize()?;
        Ok(bus)
    }

    pub(crate) fn active_sound_memory(&self) -> &SoundMemory {
        self.owned
            .as_ref()
            .map_or(&self.sound, |owner| owner.sound.memory())
    }

    #[cfg(test)]
    pub(crate) fn timed_port_sound_phase(
        &self,
    ) -> Option<(crate::hardware::vdp::owner::board::SoundPins, u64)> {
        let owner = self.owned.as_ref()?;
        Some((owner.sound.pins(), owner.sound.half()))
    }

    pub(crate) fn owned_sound_cpu(&self) -> Option<&zeff_z80::Cpu> {
        self.owned.as_ref().map(|owner| owner.sound.cpu())
    }

    pub(crate) fn active_sound_state(&self) -> Option<zeff_z80::BusState> {
        self.owned.as_ref().map(|owner| owner.sound.bus_state())
    }

    pub(crate) fn owned_fault(&self) -> Option<&str> {
        self.owned.as_ref()?.fault.as_deref()
    }

    pub(crate) fn owned_context(&mut self) -> Option<Context<'_>> {
        self.owned_context_capturing(())
    }

    pub(crate) fn owned_context_capturing<C: PhysicalCapture>(
        &mut self,
        physical: C,
    ) -> Option<Context<'_, C>> {
        Some(Context {
            physical,
            region: self.region,
            state: self.owned.as_deref_mut()?,
            cartridge: &mut self.cartridge,
            work: &mut self.ram,
            vdp: &mut self.vdp,
            input: &mut self.input,
            audio: &mut self.audio,
            prefetch: &mut self.prefetch,
            clock: &mut self.cpu_clock,
            now: &mut self.now,
            clock_fault: &mut self.clock_fault,
            video: &mut self.video,
            #[cfg(test)]
            fifo_witness: None,
            #[cfg(test)]
            work_witness: None,
        })
    }
}

impl<S: PhysicalCapture> Context<'_, S> {
    pub(crate) fn initialize(&mut self) -> Result<()> {
        loop {
            let half = self.half_now()?;
            if self.state.board.ready_tick().is_some()
                && self
                    .state
                    .board
                    .snapshot()
                    .is_some_and(|main| main.last_edge_half == half)
            {
                break;
            }
            ensure!(
                self.half_now()? < 30_000_000,
                "MAIN reset prefix did not release"
            );
            self.half()?;
        }
        let origin = self
            .state
            .board
            .snapshot()
            .ok_or_else(|| anyhow::anyhow!("missing MAIN epoch"))?
            .origin_rise_half;
        let elapsed = self
            .half_now()?
            .checked_sub(origin)
            .ok_or_else(|| anyhow::anyhow!("MAIN origin is ahead of physical time"))?;
        ensure!(
            elapsed % 14 == 0,
            "MAIN constructor frontier is off its CPU clock"
        );
        self.state.idle_clocks = elapsed / 14;
        self.state.epoch_tick = main_bus::ceil_tick(origin);
        Ok(())
    }

    fn half_now(&self) -> Result<u64> {
        self.vdp
            .owned_half()
            .ok_or_else(|| anyhow::anyhow!("physical VDP profile is not selected"))
    }

    fn half(&mut self) -> Result<()> {
        self.half_capturing(&mut ())
    }

    fn synchronize(&mut self, start: CpuClocks) -> Result<()> {
        ensure!(
            self.state.fault.is_none(),
            "prior owned bus failure: {:?}",
            self.state.fault
        );
        ensure!(*self.clock == start, "owned CPU clock mismatch");
        let origin = self
            .state
            .board
            .snapshot()
            .ok_or_else(|| anyhow::anyhow!("missing MAIN clock"))?
            .origin_rise_half;
        let clocks = start
            .0
            .checked_add(self.state.idle_clocks)
            .ok_or_else(|| anyhow::anyhow!("clock overflow"))?;
        let half = clocks
            .checked_mul(14)
            .and_then(|clocks| origin.checked_add(clocks))
            .ok_or_else(|| anyhow::anyhow!("clock overflow"))?;
        ensure!(self.half_now()? <= half, "missed MAIN clock frontier");
        while self.half_now()? < half {
            self.half()?;
        }
        Ok(())
    }

    pub(crate) fn wait_stopped(&mut self) -> Result<()> {
        self.synchronize(*self.clock)?;
        for _ in 0..14 {
            self.half()?;
        }
        self.state.idle_clocks = self
            .state
            .idle_clocks
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("STOP clock overflow"))?;
        Ok(())
    }

    fn grant_timeout(
        &mut self,
        start: CpuClocks,
        begin_half: u64,
        arbitration: u32,
    ) -> Result<Transfer> {
        let elapsed_half = self
            .half_now()?
            .checked_sub(begin_half)
            .ok_or_else(|| anyhow::anyhow!("MAIN arbitration clock reversed"))?;
        let expected_half = u64::from(arbitration)
            .checked_mul(14)
            .ok_or_else(|| anyhow::anyhow!("MAIN arbitration clock overflow"))?;
        ensure!(
            *self.clock == start && elapsed_half == expected_half && elapsed_half % 14 == 0,
            "MAIN arbitration clock differs"
        );
        let elapsed = u32::try_from(elapsed_half / 14)?;
        let wait_clocks = elapsed
            .checked_sub(4)
            .ok_or_else(|| anyhow::anyhow!("MAIN arbitration prefix is shorter than a transfer"))?;
        *self.clock = CpuClocks(
            start
                .0
                .checked_add(u64::from(elapsed))
                .ok_or_else(|| anyhow::anyhow!("CPU clock overflow"))?,
        );
        Err(GrantTimeout { wait_clocks }.into())
    }

    fn transfer(&mut self, request: main_bus::Request, start: CpuClocks) -> Result<Transfer> {
        self.transfer_capturing(request, start, &mut ())
    }
}

impl<S: PhysicalCapture> ClockedBus for Context<'_, S> {
    type Error = BusError;

    fn acknowledge_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<BusResponse<InterruptVector>, BusError> {
        let result = self.transfer(
            main_bus::Request {
                access,
                operation: main_bus::Operation::InterruptAcknowledge,
            },
            start,
        );
        let waits = transfer_wait_clocks(&result);
        let result = result.and_then(|receipt| {
            let vector = 24 + ((access.bus_address >> 1) & 7) as u8;
            ensure!(
                receipt.main.autovector == Some(vector) && receipt.main.read_word.is_none(),
                "invalid physical autovector"
            );
            Ok(BusResponse::Complete(InterruptVector::Autovector))
        });
        *self.prefetch = Prefetch::Empty;
        CycleResult {
            result: result.map_err(owned_error),
            wait_clocks: waits,
        }
    }

    fn read_cycle(&mut self, access: BusAccess, start: CpuClocks) -> CycleResult<u16, BusError> {
        let sample = self.read_instruction_cycle(access, start);
        CycleResult {
            result: sample.result.map(|sample| sample.value),
            wait_clocks: sample.wait_clocks,
        }
    }

    fn read_instruction_cycle_observed(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
        _aperture: InterruptAperture,
    ) -> ObservedCycle<InstructionInput, BusError> {
        ObservedCycle::unobserved(self.read_instruction_cycle(access, start))
    }

    fn read_instruction_cycle(
        &mut self,
        access: BusAccess,
        start: CpuClocks,
    ) -> CycleResult<InstructionInput, BusError> {
        let result = self.transfer(
            main_bus::Request {
                access,
                operation: main_bus::Operation::Read,
            },
            start,
        );
        let waits = transfer_wait_clocks(&result);
        let result = result.and_then(|receipt| {
            receipt
                .main
                .read_word
                .map(|value| InstructionInput {
                    value,
                    origin: receipt.origin,
                })
                .ok_or_else(|| anyhow::anyhow!("missing sampled MAIN word"))
        });
        if let Ok(sample) = result {
            let word = sample.value;
            let closed = access.bus_address & 0xff0000 == 0xa00000
                && (!self.state.sound.pins().bus_ack || self.state.sound.memory().reset_asserted);
            if !closed {
                *self.prefetch =
                    if access.kind == AccessKind::Prefetch && access.lanes == ByteLanes::Both {
                        Prefetch::Full(word)
                    } else {
                        Prefetch::Empty
                    };
            }
        }
        CycleResult {
            result: result.map_err(owned_error),
            wait_clocks: waits,
        }
    }

    fn write_cycle(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> CycleResult<(), BusError> {
        self.write_cycle_capturing(access, value, start, &mut ())
    }

    fn write_cycle_with_previous(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> (CycleResult<(), BusError>, [Option<u8>; 2]) {
        let mut previous = previous::Previous::default();
        let cycle = self.write_cycle_capturing(access, value, start, &mut previous);
        let previous = previous.finish(cycle.result.is_ok());
        (cycle, previous)
    }

    fn write_cycle_observed_with_previous(
        &mut self,
        access: BusAccess,
        value: u16,
        start: CpuClocks,
    ) -> (ObservedCycle<(), BusError>, [Option<u8>; 2]) {
        let (cycle, previous) = self.write_cycle_with_previous(access, value, start);
        (ObservedCycle::unobserved(cycle), previous)
    }

    fn idle(&mut self, start: CpuClocks, clocks: u32) {
        let result = (|| -> Result<()> {
            self.synchronize(start)?;
            *self.clock = CpuClocks(
                start
                    .0
                    .checked_add(u64::from(clocks))
                    .ok_or_else(|| anyhow::anyhow!("CPU clock overflow"))?,
            );
            self.synchronize(*self.clock)
        })();
        if let Err(error) = result {
            self.state.fault = Some(format!("{error:#}"));
            *self.clock_fault = true;
        }
        if clocks != 0 {
            *self.prefetch = Prefetch::Empty;
        }
    }
}

fn transfer_wait_clocks(result: &Result<Transfer>) -> u32 {
    match result {
        Ok(receipt) => receipt.wait_clocks,
        Err(error) => error
            .downcast_ref::<GrantTimeout>()
            .map_or(0, |timeout| timeout.wait_clocks),
    }
}

fn owned_error(error: anyhow::Error) -> BusError {
    BusError::Owned(format!("{error:#}"))
}

fn validate_cartridge(cartridge: &Cartridge) -> Result<()> {
    ensure!(
        cartridge.parallel_adapter.is_none()
            && cartridge.svp.is_none()
            && cartridge.sf001.is_none()
            && cartridge.sf002.is_none()
            && cartridge.sf004.is_none()
            && cartridge.download.is_none()
            && cartridge.realtec.is_none()
            && cartridge.ssf2.is_none()
            && cartridge.extended_ssf.is_none()
            && cartridge.registers.is_none()
            && cartridge.protection.is_none()
            && cartridge.save.is_none()
            && cartridge.save_latch.is_none()
            && cartridge.lock_on.is_none()
            && cartridge.lock_on_save.is_none()
            && cartridge.linear_rom.is_none()
            && cartridge.bytes.len() <= 0x400000,
        "cartridge hardware is not qualified for owned timing"
    );
    Ok(())
}
