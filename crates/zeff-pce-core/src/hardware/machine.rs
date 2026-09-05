use anyhow::bail;
use std::error::Error;
use std::fmt::{Display, Formatter};
use zeff_emu_common::address::Address;
use zeff_emu_common::cheats::CheatByteTarget;
use zeff_emu_common::debug::{
    AddressDebugController, AddressWatchHit, AddressWatchpoint, BreakpointHitCondition, DebugEvent,
    InstructionTraceRecord, InstructionTraceStore, MAX_TRACE_INSTRUCTION_BYTES, MAX_TRACE_WRITES,
    RegisterDelta, TraceExecMode, TraceWrite, TraceWriteKind, TraceWriteWidth, WatchType,
};
use zeff_emu_common::save_state::{StateReader, StateWriter};

use super::bus::{BaseBus, BaseBusError, OPEN_BUS_VALUE, PceHardwareTopology, PhysicalRegion};
use super::cartridge::{
    POPULOUS_HUCARD_RAM_LEN, PceCartridgeDescriptor, PceCartridgeHardware, PceConsoleWiring,
    PceHuCardBoard,
};
use super::cd_media::CdDisc;
use super::controller::{ControllerPort, MAX_CONTROLLER_STATE_SECTION_BYTES};
use super::cpu::{
    CpuBus, CpuStep, CpuTrap, HuC6280, InterruptStep, LineLevel, Registers, SpeedMode, StatusFlags,
    VdcPort,
};
use super::pce_devices::PceDevices;
#[cfg(feature = "profiling")]
use super::profiling::{PceProfiling, PceProfilingSnapshot};
use super::psg::{MAX_PSG_STATE_SECTION_BYTES, PsgRevision};
use super::save_state::{PceStateIdentity, read_section, write_section};
use super::vdc::{HuC6270, VdcDmaError};
use super::vdc_scanline::{VceFrameLength, VdcExternalVceScanline, VdcScanlineAdvanceError};
use super::vdc_video::{PceActiveOnlyVideoFrame, PcePresentedFrame, PceVideoRenderError};
use super::vpc::VpcVdc;

mod construction;
mod debug;
mod memory;
mod runtime;
mod state;
mod timed_bus;

use timed_bus::{TimedInstructionTrace, TimedMachineBus};

pub const PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE: u64 = 1_365;
pub const PROVISIONAL_PCE_HIGH_SPEED_MASTER_TICKS_PER_CPU_CYCLE: u64 = 3;
pub const PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE: u64 = 12;
pub const PCE_VDC_VCE_ACCESS_WAIT_CYCLES: u32 = 1;
pub const PROVISIONAL_PCE_CPU_ACTION_USES_ENTERING_SPEED: bool = true;
pub const PROVISIONAL_PCE_NON_PSG_DEVICE_ADVANCEMENT_IS_INSTRUCTION_ATOMIC: bool = true;
pub const PROVISIONAL_PCE_VSYNC_ASSERT_NORMALIZED_TO_LINE_ZERO: bool = true;
pub const PCE_OPCODE_HISTORY_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PceOpcodeHistoryEntry {
    logical_pc: u16,
    physical_pc: u32,
    opcode: u8,
    master_ticks: u64,
}

impl PceOpcodeHistoryEntry {
    #[inline]
    pub const fn logical_pc(self) -> u16 {
        self.logical_pc
    }

    #[inline]
    pub const fn physical_pc(self) -> u32 {
        self.physical_pc
    }

    #[inline]
    pub const fn opcode(self) -> u8 {
        self.opcode
    }

    #[inline]
    pub const fn master_ticks(self) -> u64 {
        self.master_ticks
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PceExecutionState {
    #[default]
    Running,
    Suspended,
}

#[derive(Debug)]
struct PceOpcodeHistory {
    entries: [PceOpcodeHistoryEntry; PCE_OPCODE_HISTORY_CAPACITY],
    cursor: usize,
    count: usize,
    enabled: bool,
}

impl Default for PceOpcodeHistory {
    fn default() -> Self {
        Self {
            entries: [PceOpcodeHistoryEntry::default(); PCE_OPCODE_HISTORY_CAPACITY],
            cursor: 0,
            count: 0,
            enabled: false,
        }
    }
}

impl PceOpcodeHistory {
    fn push(&mut self, entry: PceOpcodeHistoryEntry) {
        if !self.enabled {
            return;
        }
        self.entries[self.cursor] = entry;
        self.cursor = (self.cursor + 1) % PCE_OPCODE_HISTORY_CAPACITY;
        self.count = (self.count + 1).min(PCE_OPCODE_HISTORY_CAPACITY);
    }

    fn recent(&self, count: usize) -> Vec<PceOpcodeHistoryEntry> {
        let count = count.min(self.count);
        (0..count)
            .map(|index| {
                self.entries[(self.cursor + PCE_OPCODE_HISTORY_CAPACITY - 1 - index)
                    % PCE_OPCODE_HISTORY_CAPACITY]
            })
            .collect()
    }

    fn clear(&mut self) {
        self.cursor = 0;
        self.count = 0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PceCpuAction {
    Instruction(CpuStep),
    Interrupt(InterruptStep),
}

impl PceCpuAction {
    #[inline]
    pub const fn cycles(self) -> u32 {
        match self {
            Self::Instruction(step) => step.cycles,
            Self::Interrupt(step) => step.cycles,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PceMachineStep {
    action: PceCpuAction,
    entering_speed: SpeedMode,
    wait_cycles: u32,
    vram_contention_wait_cycles: u32,
    master_ticks: u64,
    vce_lines: u64,
    frames_published: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PceCpuDebugSnapshot {
    registers: Registers,
    mapping_registers: [u8; 8],
    physical_pc: u32,
    speed_mode: SpeedMode,
    timer_counter: u8,
    timer_reload: u8,
    timer_running: bool,
    timer_prescaler_ticks: u16,
    irq_disable: u8,
    irq_request: u8,
    sampled_interrupt: Option<super::cpu::InterruptSource>,
    master_ticks: u64,
    vce_line_index: u16,
    faulted: bool,
    execution_state: PceExecutionState,
}

impl PceCpuDebugSnapshot {
    #[inline]
    pub const fn registers(self) -> Registers {
        self.registers
    }

    #[inline]
    pub const fn mapping_registers(self) -> [u8; 8] {
        self.mapping_registers
    }

    #[inline]
    pub const fn physical_page(self, logical_addr: u16) -> u8 {
        self.mapping_registers[(logical_addr >> 13) as usize]
    }

    #[inline]
    pub const fn physical_address(self, logical_addr: u16) -> u32 {
        super::cpu::physical_address_for_page(logical_addr, self.physical_page(logical_addr))
    }

    #[inline]
    pub const fn physical_pc(self) -> u32 {
        self.physical_pc
    }

    #[inline]
    pub const fn speed_mode(self) -> SpeedMode {
        self.speed_mode
    }

    #[inline]
    pub const fn timer_counter(self) -> u8 {
        self.timer_counter
    }

    #[inline]
    pub const fn timer_reload(self) -> u8 {
        self.timer_reload
    }

    #[inline]
    pub const fn timer_running(self) -> bool {
        self.timer_running
    }

    #[inline]
    pub const fn timer_prescaler_ticks(self) -> u16 {
        self.timer_prescaler_ticks
    }

    #[inline]
    pub const fn irq_disable(self) -> u8 {
        self.irq_disable
    }

    #[inline]
    pub const fn irq_request(self) -> u8 {
        self.irq_request
    }

    #[inline]
    pub const fn sampled_interrupt(self) -> Option<super::cpu::InterruptSource> {
        self.sampled_interrupt
    }

    #[inline]
    pub const fn master_ticks(self) -> u64 {
        self.master_ticks
    }

    #[inline]
    pub const fn vce_line_index(self) -> u16 {
        self.vce_line_index
    }

    #[inline]
    pub const fn faulted(self) -> bool {
        self.faulted
    }

    #[inline]
    pub const fn execution_state(self) -> PceExecutionState {
        self.execution_state
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PceClockCounter {
    MasterTicks,
    VceLineAccumulator,
}

impl PceMachineStep {
    #[inline]
    pub const fn action(self) -> PceCpuAction {
        self.action
    }

    #[inline]
    pub const fn entering_speed(self) -> SpeedMode {
        self.entering_speed
    }

    #[inline]
    pub const fn wait_cycles(self) -> u32 {
        self.wait_cycles
    }

    #[inline]
    pub const fn vram_contention_wait_cycles(self) -> u32 {
        self.vram_contention_wait_cycles
    }

    #[inline]
    pub const fn master_ticks(self) -> u64 {
        self.master_ticks
    }

    #[inline]
    pub const fn vce_lines(self) -> u64 {
        self.vce_lines
    }

    #[inline]
    pub const fn frames_published(self) -> u64 {
        self.frames_published
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PceFrameRun {
    cpu_boundaries: u64,
    master_ticks: u64,
    frames_published: u64,
}

impl PceFrameRun {
    #[inline]
    pub const fn cpu_boundaries(self) -> u64 {
        self.cpu_boundaries
    }

    #[inline]
    pub const fn master_ticks(self) -> u64 {
        self.master_ticks
    }

    #[inline]
    pub const fn frames_published(self) -> u64 {
        self.frames_published
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PceMachineError {
    BusConstruction(BaseBusError),
    UnsupportedCartridgeHardware(PceCartridgeHardware),
    InvalidSystemCardBoard(PceHuCardBoard),
    CpuTrap(CpuTrap),
    UnsupportedVdcSync(VdcScanlineAdvanceError),
    Dma(VdcDmaError),
    VideoRender(PceVideoRenderError),
    SuperGrafxVideoCompositionUnavailable,
    ClockOverflow {
        counter: PceClockCounter,
        current: u64,
        delta: u64,
    },
    CpuCycleAccounting {
        reported: u32,
        observed: u32,
    },
    FaultedUntilReset,
}

impl Display for PceMachineError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BusConstruction(error) => Display::fmt(error, formatter),
            Self::UnsupportedCartridgeHardware(hardware) => {
                write!(
                    formatter,
                    "unsupported PC Engine cartridge hardware: {hardware:?}"
                )
            }
            Self::InvalidSystemCardBoard(board) => {
                write!(
                    formatter,
                    "invalid PC Engine CD System Card board: {board:?}"
                )
            }
            Self::CpuTrap(error) => write!(formatter, "PC Engine CPU trap: {error:?}"),
            Self::UnsupportedVdcSync(error) => {
                write!(formatter, "unsupported PC Engine VDC sync state: {error:?}")
            }
            Self::Dma(error) => write!(formatter, "PC Engine VDC DMA failed: {error:?}"),
            Self::VideoRender(error) => {
                write!(formatter, "PC Engine video render failed: {error:?}")
            }
            Self::SuperGrafxVideoCompositionUnavailable => {
                write!(formatter, "SuperGrafx video composition is unavailable")
            }
            Self::ClockOverflow {
                counter,
                current,
                delta,
            } => write!(
                formatter,
                "PC Engine {counter:?} clock overflow: {current} + {delta}"
            ),
            Self::CpuCycleAccounting { reported, observed } => write!(
                formatter,
                "PC Engine CPU reported {reported} cycles after {observed} bus cycles"
            ),
            Self::FaultedUntilReset => write!(formatter, "PC Engine machine requires reset"),
        }
    }
}

impl Error for PceMachineError {}

#[derive(Debug)]
pub struct PceMachine {
    cpu: HuC6280,
    bus: BaseBus<PceDevices>,
    front_video: PceActiveOnlyVideoFrame,
    back_video: PceActiveOnlyVideoFrame,
    master_ticks: u64,
    vce_line_accumulator: u64,
    vdc_pixel_clock_remainder: u8,
    vce_line_index: u16,
    vce_frame_length: VceFrameLength,
    faulted: bool,
    execution_state: PceExecutionState,
    suspend_after_instruction: bool,
    skip_breakpoint_once: bool,
    opcode_history: PceOpcodeHistory,
    instruction_trace: InstructionTraceStore,
    trace_scratch: TimedInstructionTrace,
    trace_frame: u64,
    debug: AddressDebugController,
    #[cfg(feature = "profiling")]
    profiling: PceProfiling,
}

impl CheatByteTarget<u16> for PceMachine {
    fn cheat_peek8(&self, address: u16) -> u8 {
        self.debug_peek_cpu8(address)
    }

    fn cheat_write8(&mut self, address: u16, value: u8) {
        self.cpu.debug_write_logical(&mut self.bus, address, value);
    }
}

pub(super) fn checked_clock_add(
    current: u64,
    delta: u64,
    counter: PceClockCounter,
) -> Result<u64, PceMachineError> {
    current
        .checked_add(delta)
        .ok_or(PceMachineError::ClockOverflow {
            counter,
            current,
            delta,
        })
}
