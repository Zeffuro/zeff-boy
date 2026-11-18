use crate::cartridge::sf001::{Location, Mapping, Revision};
use crate::hardware::bus::ownership::Reservation;
use crate::hardware::bus::scheduled::{CompactBus, ScheduledBus};
use crate::hardware::bus::{Bus, Prefetch};
use crate::hardware::sound::schedule::SoundSchedule;
use crate::{Region, SoundFault};
use zeff_m68k::Registers;
use zeff_m68k::bus::Direction;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    Empty,
    UpperOnly(u8),
    Full(u16),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoardLocation {
    Rom(u32),
    Sram(u16),
    Disconnected,
    Unclaimed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BoardContext {
    pub hidden_save: bool,
    pub mapping: u8,
    pub locked: bool,
    pub target: Option<BoardLocation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub requester: u8,
    pub start: u64,
    pub end: u64,
}

impl From<&Reservation> for Slot {
    fn from(value: &Reservation) -> Self {
        Self {
            requester: value.requester.0,
            start: value.ticks.start,
            end: value.ticks.end,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BusContext {
    pub master_ticks: u64,
    pub cpu_clock: u64,
    pub prefetch: Observation,
    pub board: Option<BoardContext>,
    pub sound_bank: u16,
    pub sound_requested: bool,
    pub sound_granted: bool,
    pub sound_reset: bool,
    pub sound_irq: bool,
    pub rmw: bool,
    pub rmw_reservation: Option<Slot>,
    pub reset_output: bool,
    pub clock_fault: bool,
    pub vdp_ticks: u64,
    pub vdp: super::VdpContext,
    pub vdp_mode: Option<u8>,
    pub vdp_dma_registers: [Option<u8>; 5],
    pub vdp_command: (u32, u8),
    pub dma_phase_and_release_unavailable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingContext {
    pub start: u64,
    pub deadline: u64,
    pub bank_access: Option<(u32, Direction, u8)>,
    pub any_bank_access: bool,
    pub reservation: Option<Slot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingSoundObservation {
    pub committed_pc: u16,
    pub committed_cycles: u64,
    pub before: zeff_z80::BusState,
    pub after: zeff_z80::BusState,
    pub instruction_pc: u16,
    pub instruction_opcode: u8,
    pub segment_cycles: u32,
    pub start: u64,
    pub deadline: u64,
    pub unclocked_bytes: [Option<u8>; 4],
    pub epoch_present: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScheduleContext {
    pub sound_cpu_cycles: u64,
    pub epoch_present: bool,
    pub rmw_checkpoint: bool,
    pub pending: Option<PendingContext>,
    pub fault: Option<SoundFault>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdapterContext {
    pub bus: BusContext,
    pub schedule: Option<ScheduleContext>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MachineSnapshot {
    pub registers: Registers,
    pub queued_words: Option<[u16; 2]>,
    pub cpu_clock: u64,
    pub context: AdapterContext,
    pub concurrent_sound: bool,
    pub audio_trace_enabled: bool,
    pub region: Region,
}

pub(crate) fn bus_context(bus: &Bus, target: Option<u32>) -> BusContext {
    let registers = bus.vdp.registers().registers();
    BusContext {
        master_ticks: bus.now.get(),
        cpu_clock: bus.cpu_clock.0,
        prefetch: match bus.prefetch {
            Prefetch::Empty => Observation::Empty,
            Prefetch::UpperOnly(value) => Observation::UpperOnly(value),
            Prefetch::Full(value) => Observation::Full(value),
        },
        board: bus.cartridge.sf001.as_ref().map(|board| BoardContext {
            hidden_save: board.revision() == Revision::HiddenSave,
            mapping: match board.mapping() {
                Mapping::Linear => 0,
                Mapping::Switched => 1,
                Mapping::Disconnected => 2,
            },
            locked: board.locked(),
            target: target.map(|address| match board.decode(address) {
                Location::Rom(offset) => BoardLocation::Rom(offset),
                Location::Sram(offset) => BoardLocation::Sram(offset),
                Location::Disconnected => BoardLocation::Disconnected,
                Location::Unclaimed => BoardLocation::Unclaimed,
            }),
        }),
        sound_bank: bus.sound.bank,
        sound_requested: bus.sound.requested,
        sound_granted: bus.sound.granted,
        sound_reset: bus.sound.reset_asserted,
        sound_irq: bus.sound.irq_asserted,
        rmw: bus.rmw.is_some(),
        rmw_reservation: bus
            .rmw
            .as_ref()
            .and_then(|scope| scope.reservation.as_ref())
            .map(Slot::from),
        reset_output: bus.reset_output,
        clock_fault: bus.clock_fault,
        vdp_ticks: bus.vdp.now().get(),
        vdp: bus.vdp.diagnostic_context(),
        vdp_mode: registers[1],
        vdp_dma_registers: std::array::from_fn(|index| registers[19 + index]),
        vdp_command: (bus.vdp.command().address, bus.vdp.command().code),
        dma_phase_and_release_unavailable: true,
    }
}

pub(crate) fn scheduled_context(
    bus: &Bus,
    cpu: &zeff_z80::Cpu,
    schedule: &SoundSchedule,
    target: Option<u32>,
) -> AdapterContext {
    AdapterContext {
        bus: bus_context(bus, target),
        schedule: Some(ScheduleContext {
            sound_cpu_cycles: cpu.cycles(),
            epoch_present: schedule.epoch.is_some(),
            rmw_checkpoint: schedule.rmw_checkpoint.is_some(),
            pending: schedule.pending.as_ref().map(|pending| PendingContext {
                start: pending.start,
                deadline: pending.deadline,
                bank_access: pending
                    .bank_access
                    .map(|access| (access.address, access.direction, access.value)),
                any_bank_access: pending.any_bank_access,
                reservation: pending.reservation().as_ref().map(Slot::from),
            }),
            fault: schedule.fault,
        }),
    }
}

pub(crate) trait TraceContext {
    fn trace_context(&self, target: Option<u32>) -> AdapterContext;
}

impl TraceContext for Bus {
    fn trace_context(&self, target: Option<u32>) -> AdapterContext {
        AdapterContext {
            bus: bus_context(self, target),
            schedule: None,
        }
    }
}

impl<C: crate::sound_trace::SoundCapture> TraceContext for ScheduledBus<'_, C> {
    fn trace_context(&self, target: Option<u32>) -> AdapterContext {
        scheduled_context(self.bus, self.cpu, self.schedule, target)
    }
}

impl<C: crate::sound_trace::SoundCapture> TraceContext for CompactBus<'_, C> {
    fn trace_context(&self, target: Option<u32>) -> AdapterContext {
        self.0.trace_context(target)
    }
}
