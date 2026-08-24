use anyhow::bail;
use std::error::Error;
use std::fmt::{Display, Formatter};
use zeff_emu_common::address::Address;
use zeff_emu_common::cheats::CheatByteTarget;
use zeff_emu_common::debug::{
    AddressDebugController, AddressWatchHit, AddressWatchpoint, BreakpointHitCondition, DebugEvent,
    InstructionTraceRecord, InstructionTraceStore, MAX_TRACE_WRITES, RegisterDelta, TraceExecMode,
    TraceWrite, TraceWriteKind, TraceWriteWidth, WatchType,
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
use super::psg::{MAX_PSG_STATE_SECTION_BYTES, PsgRevision};
use super::save_state::{PceV1Identity, read_section, write_section};
use super::vdc::{HuC6270, VdcDmaError};
use super::vdc_scanline::{VceFrameLength, VdcExternalVceScanline, VdcScanlineAdvanceError};
use super::vdc_video::{PceActiveOnlyVideoFrame, PcePresentedFrame, PceVideoRenderError};
use super::vpc::VpcVdc;

pub const PCE_NTSC_REFERENCE_MHZ_NUMERATOR: u64 = 315;
pub const PCE_NTSC_REFERENCE_MHZ_DENOMINATOR: u64 = 88;
pub const PCE_MASTER_CLOCK_NTSC_REFERENCE_MULTIPLIER: u64 = 6;
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
    trace_frame: u64,
    debug: AddressDebugController,
}

struct TimedMachineBus<'a> {
    inner: &'a mut BaseBus<PceDevices>,
    front_video: &'a mut PceActiveOnlyVideoFrame,
    back_video: &'a mut PceActiveOnlyVideoFrame,
    vce_line_accumulator: &'a mut u64,
    vdc_pixel_clock_remainder: &'a mut u8,
    vce_line_index: &'a mut u16,
    vce_frame_length: &'a mut VceFrameLength,
    master_ticks_per_cycle: u64,
    observed_cycles: u32,
    video_wait_cycles: u32,
    vram_contention_wait_cycles: u32,
    elapsed_master_ticks: u64,
    unclaimed_on_chip_master_ticks: u64,
    vce_lines: u64,
    frames_published: u64,
    fault: Option<PceMachineError>,
    pending_debug_write: Option<(u32, u8)>,
    instruction_bytes: Vec<u8>,
    trace_writes: Vec<TraceWrite>,
    trace_write_overflow: u16,
    trace_enabled: bool,
    dma_completed: bool,
    debug: &'a mut AddressDebugController,
}

impl<'a> TimedMachineBus<'a> {
    #[allow(clippy::too_many_arguments)]
    fn new(
        inner: &'a mut BaseBus<PceDevices>,
        front_video: &'a mut PceActiveOnlyVideoFrame,
        back_video: &'a mut PceActiveOnlyVideoFrame,
        vce_line_accumulator: &'a mut u64,
        vdc_pixel_clock_remainder: &'a mut u8,
        vce_line_index: &'a mut u16,
        vce_frame_length: &'a mut VceFrameLength,
        master_ticks_per_cycle: u64,
        trace_enabled: bool,
        debug: &'a mut AddressDebugController,
    ) -> Self {
        Self {
            inner,
            front_video,
            back_video,
            vce_line_accumulator,
            vdc_pixel_clock_remainder,
            vce_line_index,
            vce_frame_length,
            master_ticks_per_cycle,
            observed_cycles: 0,
            video_wait_cycles: 0,
            vram_contention_wait_cycles: 0,
            elapsed_master_ticks: 0,
            unclaimed_on_chip_master_ticks: 0,
            vce_lines: 0,
            frames_published: 0,
            fault: None,
            pending_debug_write: None,
            instruction_bytes: Vec::new(),
            trace_writes: Vec::new(),
            trace_write_overflow: 0,
            trace_enabled,
            dma_completed: false,
            debug,
        }
    }

    fn advance_cycle(&mut self) {
        self.observed_cycles += 1;
        self.advance_devices(self.master_ticks_per_cycle);
    }

    fn advance_access(&mut self, physical_addr: u32, write: bool) -> bool {
        self.pending_debug_write = None;
        self.advance_cycle();
        if is_vdc_vce_access(physical_addr) {
            self.advance_devices(
                u64::from(PCE_VDC_VCE_ACCESS_WAIT_CYCLES) * self.master_ticks_per_cycle,
            );
            self.video_wait_cycles += PCE_VDC_VCE_ACCESS_WAIT_CYCLES;
        }
        if let Some(target) = vdc_vram_cycle_target(self.inner, physical_addr, write) {
            self.wait_for_vdc_dma(target);
        }
        let completed = self.fault.is_none();
        if completed && write {
            self.pending_debug_write = Some((physical_addr, self.inner.peek(physical_addr)));
        }
        completed
    }

    fn advance_direct_vdc_access(&mut self, port: VdcPort) -> bool {
        self.advance_cycle();
        self.advance_devices(
            u64::from(PCE_VDC_VCE_ACCESS_WAIT_CYCLES) * self.master_ticks_per_cycle,
        );
        self.video_wait_cycles += PCE_VDC_VCE_ACCESS_WAIT_CYCLES;
        if let Some(target) = direct_vdc_vram_write_target(self.inner, port) {
            self.wait_for_vdc_dma(target);
        }
        self.fault.is_none()
    }

    fn advance_remaining(&mut self, reported_cycles: u32) -> Result<(), PceMachineError> {
        let remaining = reported_cycles.checked_sub(self.observed_cycles).ok_or(
            PceMachineError::CpuCycleAccounting {
                reported: reported_cycles,
                observed: self.observed_cycles,
            },
        )?;
        self.advance_devices(u64::from(remaining) * self.master_ticks_per_cycle);
        self.fault.map_or(Ok(()), Err)
    }

    fn record_trace_write(&mut self, write: TraceWrite) {
        if !self.trace_enabled {
            return;
        }
        if self.trace_writes.len() == MAX_TRACE_WRITES {
            self.trace_write_overflow = self.trace_write_overflow.saturating_add(1);
        } else {
            self.trace_writes.push(write);
        }
    }

    fn wait_for_vdc_dma(&mut self, target: VpcVdc) {
        while self.fault.is_none()
            && self
                .inner
                .devices()
                .vdc_for(target)
                .is_some_and(HuC6270::dma_owns_vram_slots)
        {
            self.advance_devices(self.master_ticks_per_cycle);
            self.video_wait_cycles += 1;
            self.vram_contention_wait_cycles += 1;
        }
    }

    fn advance_devices(&mut self, master_ticks: u64) {
        if self.fault.is_some() || master_ticks == 0 {
            return;
        }
        let mut remaining = master_ticks;
        while remaining != 0 {
            let until_line = PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE - *self.vce_line_accumulator;
            let elapsed = remaining.min(until_line);
            let result = self.advance_video_chunk(elapsed);
            self.elapsed_master_ticks += elapsed;
            self.unclaimed_on_chip_master_ticks += elapsed;
            self.inner.devices_mut().advance_master_ticks(elapsed);
            self.observe_dma_completion();
            remaining -= elapsed;
            if let Err(error) = result {
                self.fault = Some(error);
                return;
            }
        }
    }

    fn observe_dma_completion(&mut self) {
        self.dma_completed |= self.inner.devices_mut().take_debug_dma_completed();
    }

    fn advance_video_chunk(&mut self, master_ticks: u64) -> Result<(), PceMachineError> {
        debug_assert!(
            master_ticks <= PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE - *self.vce_line_accumulator
        );
        self.advance_vdc_master_ticks(master_ticks)?;
        *self.vce_line_accumulator += master_ticks;
        if *self.vce_line_accumulator == PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE {
            *self.vce_line_accumulator = 0;
            self.process_vce_line()?;
            self.inner.devices_mut().begin_external_horizontal_line();
            self.vce_lines += 1;
            *self.vce_line_index += 1;
            if *self.vce_line_index == self.vce_frame_length.scanlines() {
                *self.vce_line_index = 0;
                std::mem::swap(self.front_video, self.back_video);
                self.back_video.begin_frame();
                self.frames_published += 1;
            }
        }
        Ok(())
    }

    fn advance_vdc_master_ticks(&mut self, master_ticks: u64) -> Result<(), PceMachineError> {
        let divisor = u64::from(self.inner.devices().vce().pixel_clock().divisor());
        let total = master_ticks + u64::from(*self.vdc_pixel_clock_remainder);
        let pixel_clocks = total / divisor;
        *self.vdc_pixel_clock_remainder = (total % divisor) as u8;
        self.inner
            .devices_mut()
            .advance_horizontal_pixels(pixel_clocks)
            .map_err(PceMachineError::Dma)?;
        Ok(())
    }

    fn process_vce_line(&mut self) -> Result<(), PceMachineError> {
        let vsync_started = *self.vce_line_index == 0;
        if vsync_started {
            *self.vce_frame_length = self.inner.devices().vce().frame_length();
        }
        let input = VdcExternalVceScanline::new(1, vsync_started, *self.vce_frame_length);
        let (boundary, second_boundary) = self
            .inner
            .devices_mut()
            .advance_machine_vce_scanline(input)
            .map_err(PceMachineError::UnsupportedVdcSync)?;
        let pixel_clock = self.inner.devices().vce().pixel_clock();
        match second_boundary {
            None => {
                if let Some(display) = boundary.active_display() {
                    let (vdc, vce) = self.inner.devices_mut().video_devices_mut();
                    self.back_video
                        .render_active_line(vdc, vce, display, pixel_clock)
                        .map_err(PceMachineError::VideoRender)?;
                }
            }
            Some(second) => {
                let display_one = boundary.active_display();
                let display_two = second.active_display();
                if display_one.is_some() || display_two.is_some() {
                    let (vdc_one, vdc_two, vpc, vce) = self
                        .inner
                        .devices_mut()
                        .supergrafx_video_devices_mut()
                        .expect("SuperGrafx boundary requires SuperGrafx devices");
                    self.back_video
                        .render_supergrafx_active_line(
                            vdc_one,
                            vdc_two,
                            vpc,
                            vce,
                            display_one,
                            display_two,
                            *self.vce_line_index,
                            pixel_clock,
                        )
                        .map_err(PceMachineError::VideoRender)?;
                }
            }
        }
        Ok(())
    }
}

impl CpuBus for TimedMachineBus<'_> {
    fn read(&mut self, physical_addr: u32) -> u8 {
        if self.advance_access(physical_addr, false) {
            self.inner.read(physical_addr)
        } else {
            OPEN_BUS_VALUE
        }
    }

    fn write(&mut self, physical_addr: u32, value: u8) {
        if self.advance_access(physical_addr, true) {
            self.inner.write(physical_addr, value);
            self.observe_dma_completion();
        }
    }

    fn dummy_read(&mut self, physical_addr: u32) -> u8 {
        if self.advance_access(physical_addr, false) {
            self.inner.dummy_read(physical_addr)
        } else {
            OPEN_BUS_VALUE
        }
    }

    fn dummy_write(&mut self, physical_addr: u32, value: u8) {
        if self.advance_access(physical_addr, true) {
            self.inner.dummy_write(physical_addr, value);
            self.observe_dma_completion();
        }
    }

    fn write_vdc(&mut self, port: VdcPort, value: u8) {
        let old_value = self.inner.peek(0x1F_E000 | u32::from(port.offset()));
        if self.advance_direct_vdc_access(port) {
            self.inner.write_vdc(port, value);
            self.record_trace_write(TraceWrite {
                address: u32::from(port.offset()),
                old_value: u32::from(old_value),
                new_value: u32::from(value),
                width: TraceWriteWidth::Byte,
                kind: TraceWriteKind::Io,
            });
        }
    }

    fn advance_internal_access(&mut self, physical_addr: u32, write: bool) -> bool {
        self.advance_access(physical_addr, write)
    }

    fn take_elapsed_master_ticks(&mut self) -> u64 {
        std::mem::take(&mut self.unclaimed_on_chip_master_ticks)
    }

    fn observe_internal_read(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        self.inner
            .observe_internal_read(physical_addr, value, dummy);
    }

    fn observe_internal_write(&mut self, physical_addr: u32, value: u8, dummy: bool) {
        self.inner
            .observe_internal_write(physical_addr, value, dummy);
    }

    fn observe_logical_read(
        &mut self,
        logical_addr: u16,
        _physical_addr: u32,
        value: u8,
        _dummy: bool,
    ) {
        if self.debug.hit_watchpoint.is_none() {
            self.debug
                .check_watch_read(Address::from(logical_addr), value);
        }
    }

    fn observe_logical_write(
        &mut self,
        logical_addr: u16,
        physical_addr: u32,
        value: u8,
        _dummy: bool,
    ) {
        let Some((pending_addr, old_value)) = self.pending_debug_write.take() else {
            return;
        };
        debug_assert_eq!(pending_addr, physical_addr);
        self.record_trace_write(TraceWrite {
            address: u32::from(logical_addr),
            old_value: u32::from(old_value),
            new_value: u32::from(value),
            width: TraceWriteWidth::Byte,
            kind: TraceWriteKind::Memory,
        });
        if self.debug.hit_watchpoint.is_none() {
            self.debug
                .check_watch_write(Address::from(logical_addr), old_value, value);
        }
    }

    fn observe_instruction_byte(&mut self, _logical_addr: u16, _physical_addr: u32, value: u8) {
        if self.trace_enabled {
            self.instruction_bytes.push(value);
        }
    }

    fn idle(&mut self) {
        self.advance_cycle();
        self.inner.idle();
    }
}

impl PceMachine {
    pub(super) fn validate_v1_state_target(&self) -> anyhow::Result<()> {
        if self.devices().cdrom2().is_some()
            && self.hardware_topology() != PceHardwareTopology::Base
        {
            bail!("PC Engine CD save-states require the base hardware topology");
        }
        if self.devices().arcade_card().is_some()
            && (self.devices().cdrom2().is_none()
                || self.hucard_board() != PceHuCardBoard::SystemCardV3)
        {
            bail!("Arcade Card save-states require a System Card v3 CD machine");
        }
        Ok(())
    }

    pub(super) fn validate_v1_encode_state(&self) -> anyhow::Result<()> {
        self.validate_v1_state_target()?;
        if self.faulted {
            bail!("faulted PC Engine machines cannot be saved");
        }
        if !self.cpu.at_action_boundary() {
            bail!("PC Engine states can only be saved at CPU action boundaries");
        }
        self.devices().psg().validate_v1_state()?;
        if let Some(cdrom2) = self.devices().cdrom2() {
            cdrom2.validate_v1_state()?;
        }
        Ok(())
    }

    pub(super) fn write_v1_state(&self, writer: &mut StateWriter) {
        write_section(writer, |section| self.cpu.write_state(section));
        write_section(writer, |section| self.bus.write_state(section));
        write_section(writer, |section| {
            self.bus.devices().vdc().write_state(section)
        });
        if let Some(supergrafx) = self.bus.devices().supergrafx_video() {
            write_section(writer, |section| supergrafx.vdc2().write_state(section));
            write_section(writer, |section| supergrafx.vpc().write_state(section));
        }
        write_section(writer, |section| {
            self.bus.devices().vce().write_state(section)
        });
        write_section(writer, |section| {
            self.bus.devices().psg().write_state(section)
        });
        write_section(writer, |section| {
            self.bus.devices().controller().write_state(section)
        });
        if let Some(cdrom2) = self.bus.devices().cdrom2() {
            write_section(writer, |section| cdrom2.write_state(section));
        }
        if let Some(arcade_card) = self.bus.devices().arcade_card() {
            write_section(writer, |section| arcade_card.write_state(section));
        }
        write_section(writer, |section| {
            section.write_u64(self.master_ticks);
            section.write_u64(self.vce_line_accumulator);
            section.write_u8(self.vdc_pixel_clock_remainder);
            section.write_u16(self.vce_line_index);
            section.write_u8(match self.vce_frame_length {
                VceFrameLength::Lines262 => 0,
                VceFrameLength::Lines263 => 1,
            });
            section.write_u8(match self.execution_state {
                PceExecutionState::Running => 0,
                PceExecutionState::Suspended => 1,
            });
            section.write_bool(self.suspend_after_instruction);
        });
        write_section(writer, |section| self.front_video.write_state(section));
        write_section(writer, |section| self.back_video.write_state(section));
    }

    pub(super) fn replace_from_v1_state(
        &mut self,
        data: &[u8],
        identity: PceV1Identity,
    ) -> anyhow::Result<()> {
        let PceV1Identity {
            board,
            topology,
            wiring,
            psg_revision,
            is_cd,
            has_arcade_card,
        } = identity;
        let runtime_audio = self.devices().psg().runtime_config();
        let runtime_cd_audio = self
            .devices()
            .cdrom2()
            .map(super::cdrom2::CdRom2::runtime_audio_config);
        let history_enabled = self.opcode_history.enabled;
        let trace_enabled = self.instruction_trace.is_enabled();
        let trace_capacity = self.instruction_trace.capacity();
        let cartridge = PceCartridgeDescriptor::default()
            .with_console_wiring(wiring)
            .with_required_hardware(match topology {
                PceHardwareTopology::Base => PceCartridgeHardware::Base,
                PceHardwareTopology::SuperGrafx => PceCartridgeHardware::SuperGrafx,
            })
            .with_hucard_board(board);
        let mut restored = if is_cd {
            if topology != PceHardwareTopology::Base {
                bail!("PC Engine CD save-state has an unsupported hardware topology");
            }
            let disc = self
                .devices()
                .cdrom2()
                .expect("CD state target has CD hardware")
                .disc()
                .clone();
            Self::with_cdrom2_system_card_controller_and_arcade_card(
                self.hucard_rom().to_vec(),
                board,
                disc,
                wiring,
                ControllerPort::default(),
                has_arcade_card,
            )
            .map_err(|error| anyhow::anyhow!(error))?
        } else {
            Self::with_topology(
                self.hucard_rom().to_vec(),
                cartridge,
                ControllerPort::default(),
                psg_revision,
                topology,
            )
            .map_err(|error| anyhow::anyhow!(error))?
        };
        restored.devices_mut().psg_mut().apply_runtime_config(
            runtime_audio.0,
            runtime_audio.1,
            runtime_audio.2,
            runtime_audio.3,
        );

        let mut reader = StateReader::new(data);
        read_section(&mut reader, 256, "CPU", |section| {
            restored.cpu.read_state(section)
        })?;
        if !restored.cpu.at_action_boundary() {
            bail!("PC Engine save-state is not at a CPU action boundary");
        }
        read_section(&mut reader, 256 * 1024, "bus", |section| {
            restored.bus.read_state(section)
        })?;
        read_section(&mut reader, 80 * 1024, "VDC", |section| {
            restored.bus.devices_mut().vdc_mut().read_state(section)
        })?;
        if topology == PceHardwareTopology::SuperGrafx {
            read_section(&mut reader, 80 * 1024, "VDC2", |section| {
                restored
                    .bus
                    .devices_mut()
                    .supergrafx_video_mut()
                    .expect("SuperGrafx state target has SuperGrafx devices")
                    .vdc2_mut()
                    .read_state(section)
            })?;
            read_section(&mut reader, 64, "VPC", |section| {
                restored
                    .bus
                    .devices_mut()
                    .supergrafx_video_mut()
                    .expect("SuperGrafx state target has SuperGrafx devices")
                    .vpc_mut()
                    .read_state(section)
            })?;
        }
        read_section(&mut reader, 2 * 1024, "VCE", |section| {
            restored.bus.devices_mut().vce_mut().read_state(section)
        })?;
        read_section(&mut reader, MAX_PSG_STATE_SECTION_BYTES, "PSG", |section| {
            restored.bus.devices_mut().psg_mut().read_state(section)
        })?;
        read_section(
            &mut reader,
            MAX_CONTROLLER_STATE_SECTION_BYTES,
            "controller",
            |section| {
                restored
                    .bus
                    .devices_mut()
                    .controller_mut()
                    .read_state(section)
            },
        )?;
        if is_cd {
            let (sample_rate, generation_enabled) =
                runtime_cd_audio.expect("CD state target has a retained CD audio configuration");
            let cdrom2 = restored
                .bus
                .devices_mut()
                .cdrom2_mut()
                .expect("restored CD state has CD hardware");
            cdrom2.set_sample_rate(sample_rate);
            cdrom2.set_sample_generation_enabled(generation_enabled);
            read_section(
                &mut reader,
                super::cdrom2::state::MAX_CDROM2_STATE_SECTION_BYTES,
                "CD-ROM2",
                |section| cdrom2.read_state(section),
            )?;
        }
        if has_arcade_card {
            read_section(
                &mut reader,
                super::arcade_card::MAX_ARCADE_CARD_STATE_SECTION_BYTES,
                "Arcade Card",
                |section| {
                    restored
                        .bus
                        .devices_mut()
                        .arcade_card_mut()
                        .expect("Arcade Card state target has Arcade Card hardware")
                        .read_state(section)
                },
            )?;
        }
        read_section(&mut reader, 64, "machine timing", |section| {
            restored.master_ticks = section.read_u64()?;
            restored.vce_line_accumulator = section.read_u64()?;
            if restored.vce_line_accumulator >= PROVISIONAL_PCE_MASTER_TICKS_PER_VCE_LINE {
                bail!(
                    "invalid machine VCE-line accumulator in save-state: {}",
                    restored.vce_line_accumulator
                );
            }
            restored.vdc_pixel_clock_remainder = section.read_u8()?;
            let pixel_divisor = restored.bus.devices().vce().pixel_clock().divisor();
            if restored.vdc_pixel_clock_remainder >= pixel_divisor {
                bail!(
                    "invalid machine VDC pixel-clock remainder in save-state: {}",
                    restored.vdc_pixel_clock_remainder
                );
            }
            restored.vce_line_index = section.read_u16()?;
            restored.vce_frame_length = match section.read_u8()? {
                0 => VceFrameLength::Lines262,
                1 => VceFrameLength::Lines263,
                tag => bail!("invalid VCE frame-length tag in save-state: {tag}"),
            };
            if restored.vce_line_index >= restored.vce_frame_length.scanlines() {
                bail!(
                    "invalid machine VCE line index in save-state: {}",
                    restored.vce_line_index
                );
            }
            restored.execution_state = match section.read_u8()? {
                0 => PceExecutionState::Running,
                1 => PceExecutionState::Suspended,
                tag => bail!("invalid machine execution-state tag in save-state: {tag}"),
            };
            restored.suspend_after_instruction = section.read_bool()?;
            if restored.execution_state == PceExecutionState::Suspended
                && restored.suspend_after_instruction
            {
                bail!("suspended PC Engine save-state has a pending debug step");
            }
            Ok(())
        })?;
        read_section(
            &mut reader,
            super::vdc_video::PCE_ACTIVE_FRAME_RGBA_BYTES + 5 * 512,
            "front video frame",
            |section| restored.front_video.read_state(section),
        )?;
        read_section(
            &mut reader,
            super::vdc_video::PCE_ACTIVE_FRAME_RGBA_BYTES + 5 * 512,
            "back video frame",
            |section| restored.back_video.read_state(section),
        )?;
        if !reader.is_exhausted() {
            bail!("PC Engine save-state payload has unexpected trailing data");
        }
        restored.validate_v1_encode_state()?;
        restored.opcode_history.enabled = history_enabled;
        restored.opcode_history.clear();
        restored.instruction_trace.set_capacity(trace_capacity);
        restored.instruction_trace.set_enabled(trace_enabled);
        restored.instruction_trace.clear();
        *self = restored;
        Ok(())
    }

    pub fn new(hucard_rom: Vec<u8>) -> Result<Self, PceMachineError> {
        Self::with_cartridge_and_controller(
            hucard_rom,
            PceCartridgeDescriptor::default(),
            ControllerPort::default(),
        )
    }

    pub fn with_controller(
        hucard_rom: Vec<u8>,
        controller: ControllerPort,
    ) -> Result<Self, PceMachineError> {
        Self::with_cartridge_and_controller(
            hucard_rom,
            PceCartridgeDescriptor::default(),
            controller,
        )
    }

    pub fn with_psg_revision(
        hucard_rom: Vec<u8>,
        psg_revision: PsgRevision,
    ) -> Result<Self, PceMachineError> {
        Self::with_cartridge_controller_and_psg_revision(
            hucard_rom,
            PceCartridgeDescriptor::default(),
            ControllerPort::default(),
            psg_revision,
        )
    }

    pub fn with_cartridge(
        hucard_rom: Vec<u8>,
        cartridge: PceCartridgeDescriptor,
    ) -> Result<Self, PceMachineError> {
        Self::with_cartridge_and_controller(hucard_rom, cartridge, ControllerPort::default())
    }

    pub fn with_cartridge_and_controller(
        hucard_rom: Vec<u8>,
        cartridge: PceCartridgeDescriptor,
        controller: ControllerPort,
    ) -> Result<Self, PceMachineError> {
        let psg_revision = match cartridge.required_hardware() {
            PceCartridgeHardware::Base => PsgRevision::HuC6280,
            PceCartridgeHardware::SuperGrafx => PsgRevision::HuC6280A,
        };
        Self::with_cartridge_controller_and_psg_revision(
            hucard_rom,
            cartridge,
            controller,
            psg_revision,
        )
    }

    pub fn with_cdrom2(system_card_rom: Vec<u8>, disc: CdDisc) -> Result<Self, PceMachineError> {
        Self::with_cdrom2_and_controller(
            system_card_rom,
            disc,
            PceConsoleWiring::PcEngine,
            ControllerPort::default(),
        )
    }

    pub fn with_cdrom2_and_controller(
        system_card_rom: Vec<u8>,
        disc: CdDisc,
        console_wiring: PceConsoleWiring,
        controller: ControllerPort,
    ) -> Result<Self, PceMachineError> {
        Self::with_cdrom2_system_card_and_controller(
            system_card_rom,
            PceHuCardBoard::SystemCardV1V2,
            disc,
            console_wiring,
            controller,
        )
    }

    pub fn with_cdrom2_system_card_and_controller(
        system_card_rom: Vec<u8>,
        system_card_board: PceHuCardBoard,
        disc: CdDisc,
        console_wiring: PceConsoleWiring,
        controller: ControllerPort,
    ) -> Result<Self, PceMachineError> {
        Self::with_cdrom2_system_card_controller_and_arcade_card(
            system_card_rom,
            system_card_board,
            disc,
            console_wiring,
            controller,
            false,
        )
    }

    pub fn with_cdrom2_system_card_controller_and_arcade_card(
        system_card_rom: Vec<u8>,
        system_card_board: PceHuCardBoard,
        disc: CdDisc,
        console_wiring: PceConsoleWiring,
        controller: ControllerPort,
        arcade_card: bool,
    ) -> Result<Self, PceMachineError> {
        if !matches!(
            system_card_board,
            PceHuCardBoard::SystemCardV1V2 | PceHuCardBoard::SystemCardV3
        ) {
            return Err(PceMachineError::InvalidSystemCardBoard(system_card_board));
        }
        if arcade_card && system_card_board != PceHuCardBoard::SystemCardV3 {
            return Err(PceMachineError::InvalidSystemCardBoard(system_card_board));
        }
        let bus = BaseBus::with_hucard(
            system_card_rom,
            system_card_board,
            PceDevices::with_cdrom2_system_card_and_arcade_card(
                controller,
                console_wiring,
                disc,
                system_card_board == PceHuCardBoard::SystemCardV3,
                arcade_card,
            ),
        )
        .map_err(PceMachineError::BusConstruction)?;
        Ok(Self::finish_new(bus))
    }

    fn with_cartridge_controller_and_psg_revision(
        hucard_rom: Vec<u8>,
        cartridge: PceCartridgeDescriptor,
        controller: ControllerPort,
        psg_revision: PsgRevision,
    ) -> Result<Self, PceMachineError> {
        let topology = match cartridge.required_hardware() {
            PceCartridgeHardware::Base => PceHardwareTopology::Base,
            PceCartridgeHardware::SuperGrafx => PceHardwareTopology::SuperGrafx,
        };
        Self::with_topology(hucard_rom, cartridge, controller, psg_revision, topology)
    }

    fn with_topology(
        hucard_rom: Vec<u8>,
        cartridge: PceCartridgeDescriptor,
        controller: ControllerPort,
        psg_revision: PsgRevision,
        topology: PceHardwareTopology,
    ) -> Result<Self, PceMachineError> {
        let board = cartridge.hucard_board(hucard_rom.len());
        let bus = BaseBus::with_hucard_and_topology(
            hucard_rom,
            board,
            topology,
            PceDevices::with_topology_console_wiring_and_psg_revision(
                topology,
                controller,
                cartridge.console_wiring(),
                psg_revision,
            ),
        )
        .map_err(PceMachineError::BusConstruction)?;
        Ok(Self::finish_new(bus))
    }

    fn finish_new(bus: BaseBus<PceDevices>) -> Self {
        let mut machine = Self {
            cpu: HuC6280::new(),
            bus,
            front_video: PceActiveOnlyVideoFrame::new(),
            back_video: PceActiveOnlyVideoFrame::new(),
            master_ticks: 0,
            vce_line_accumulator: 0,
            vdc_pixel_clock_remainder: 0,
            vce_line_index: 0,
            vce_frame_length: VceFrameLength::Lines262,
            faulted: false,
            execution_state: PceExecutionState::Running,
            suspend_after_instruction: false,
            skip_breakpoint_once: false,
            opcode_history: PceOpcodeHistory::default(),
            instruction_trace: InstructionTraceStore::default(),
            trace_frame: 0,
            debug: AddressDebugController::new(),
        };
        machine.reset();
        machine
    }

    #[cfg(test)]
    pub(super) fn with_supergrafx_substrate_for_test(
        hucard_rom: Vec<u8>,
    ) -> Result<Self, PceMachineError> {
        Self::with_topology(
            hucard_rom,
            PceCartridgeDescriptor::default(),
            ControllerPort::default(),
            PsgRevision::HuC6280,
            PceHardwareTopology::SuperGrafx,
        )
    }

    pub fn reset(&mut self) {
        self.bus.reset_hucard();
        self.bus.devices_mut().reset();
        self.cpu.set_irq1_line(LineLevel::High);
        self.cpu.set_irq2_line(LineLevel::High);
        self.cpu.set_nmi_line(LineLevel::High);
        self.cpu.reset(&mut self.bus);
        self.front_video.begin_frame();
        self.back_video.begin_frame();
        self.master_ticks = 0;
        self.vce_line_accumulator = 0;
        self.vdc_pixel_clock_remainder = 0;
        self.vce_line_index = 0;
        self.vce_frame_length = self.bus.devices().vce().frame_length();
        self.faulted = false;
        self.execution_state = PceExecutionState::Running;
        self.suspend_after_instruction = false;
        self.skip_breakpoint_once = false;
        self.opcode_history.clear();
        self.instruction_trace.clear();
        self.trace_frame = 0;
        self.debug.clear_hits();
        self.refresh_cdrom2_irq2();
    }

    #[inline]
    pub fn debug_snapshot(&self) -> PceCpuDebugSnapshot {
        let registers = self.cpu.cpu().registers();
        let on_chip_io = self.cpu.on_chip_io();
        PceCpuDebugSnapshot {
            registers,
            mapping_registers: self.cpu.cpu().mapping_registers(),
            physical_pc: self.cpu.cpu().logical_to_physical(registers.pc),
            speed_mode: self.cpu.cpu().speed_mode(),
            timer_counter: on_chip_io.read_timer_counter(),
            timer_reload: on_chip_io.timer_reload(),
            timer_running: on_chip_io.timer_running(),
            timer_prescaler_ticks: on_chip_io.timer_prescaler_ticks(),
            irq_disable: on_chip_io.read_irq(super::cpu::IrqPort::Disable),
            irq_request: on_chip_io.read_irq(super::cpu::IrqPort::Request),
            sampled_interrupt: self.cpu.sampled_interrupt(),
            master_ticks: self.master_ticks,
            vce_line_index: self.vce_line_index,
            faulted: self.faulted,
            execution_state: self.execution_state,
        }
    }

    #[inline]
    pub const fn execution_state(&self) -> PceExecutionState {
        self.execution_state
    }

    #[inline]
    pub const fn is_cpu_suspended(&self) -> bool {
        matches!(self.execution_state, PceExecutionState::Suspended)
    }

    pub fn debug_continue(&mut self) {
        self.skip_breakpoint_once = self.debug.hit_breakpoint.is_some();
        self.debug.clear_hits();
        self.execution_state = PceExecutionState::Running;
        self.suspend_after_instruction = false;
    }

    pub fn debug_step(&mut self) {
        self.debug.clear_hits();
        self.execution_state = PceExecutionState::Running;
        self.suspend_after_instruction = true;
        self.skip_breakpoint_once = false;
    }

    pub fn debug_suspend(&mut self) {
        self.execution_state = PceExecutionState::Suspended;
        self.suspend_after_instruction = false;
        self.skip_breakpoint_once = false;
    }

    pub fn debug_execute_guest_call(
        &mut self,
        target: u16,
        instruction_budget: u64,
    ) -> Result<u64, String> {
        if !self.is_cpu_suspended() {
            return Err("CPU must be suspended".to_owned());
        }
        if self.faulted {
            return Err("machine is faulted".to_owned());
        }

        let registers = self.cpu.cpu().registers();
        if target == registers.pc || instruction_budget == 0 {
            return Err("invalid call target or budget".to_owned());
        }

        let return_pc = registers.pc;
        let return_sp = registers.sp;
        let saved_interrupt_disable = registers.status.contains(StatusFlags::INTERRUPT);
        let saved_sampled_interrupt = self.cpu.replace_sampled_interrupt(None);
        let return_address = return_pc.wrapping_sub(1);
        self.cpu.debug_write_logical(
            &mut self.bus,
            0x2100 | u16::from(return_sp),
            (return_address >> 8) as u8,
        );
        self.cpu.cpu_mut().registers_mut().sp = return_sp.wrapping_sub(1);
        self.cpu.debug_write_logical(
            &mut self.bus,
            0x2100 | u16::from(return_sp.wrapping_sub(1)),
            return_address as u8,
        );
        let registers = self.cpu.cpu_mut().registers_mut();
        registers.sp = return_sp.wrapping_sub(2);
        registers.pc = target;
        registers.status.insert(StatusFlags::INTERRUPT);
        self.debug.clear_hits();
        self.execution_state = PceExecutionState::Running;
        self.suspend_after_instruction = false;
        self.skip_breakpoint_once = false;

        for instructions in 1..=instruction_budget {
            self.cpu.replace_sampled_interrupt(None);
            self.step_boundary_faulting()
                .map_err(|error| error.to_string())?;

            let registers = self.cpu.cpu().registers();
            if registers.pc == return_pc && registers.sp == return_sp {
                self.cpu
                    .cpu_mut()
                    .registers_mut()
                    .status
                    .set(StatusFlags::INTERRUPT, saved_interrupt_disable);
                self.cpu.replace_sampled_interrupt(saved_sampled_interrupt);
                self.execution_state = PceExecutionState::Suspended;
                return Ok(instructions);
            }
            if self.is_cpu_suspended() {
                return Err("call hit a debugger stop".to_owned());
            }
        }

        self.execution_state = PceExecutionState::Suspended;
        Err("call exceeded its instruction budget".to_owned())
    }

    pub fn set_opcode_history_enabled(&mut self, enabled: bool) {
        self.opcode_history.enabled = enabled;
    }

    pub fn recent_opcodes(&self, count: usize) -> Vec<PceOpcodeHistoryEntry> {
        self.opcode_history.recent(count)
    }

    pub const fn instruction_trace(&self) -> &InstructionTraceStore {
        &self.instruction_trace
    }

    pub fn set_instruction_trace_enabled(&mut self, enabled: bool) {
        self.instruction_trace.set_enabled(enabled);
    }

    pub fn set_instruction_trace_capacity(&mut self, capacity: usize) {
        self.instruction_trace.set_capacity(capacity);
    }

    pub fn clear_instruction_trace(&mut self) {
        self.instruction_trace.clear();
    }

    pub fn set_event_breakpoint(&mut self, event: DebugEvent, enabled: bool) {
        if matches!(event, DebugEvent::Interrupt | DebugEvent::Dma) {
            self.debug.set_event_breakpoint(event, enabled);
        }
    }

    pub fn iter_event_breakpoints(&self) -> impl Iterator<Item = DebugEvent> + '_ {
        self.debug.iter_event_breakpoints()
    }

    pub const fn debug_hit_event(&self) -> Option<DebugEvent> {
        self.debug.hit_event
    }

    pub fn add_breakpoint(&mut self, addr: u16) {
        self.debug.add_breakpoint(Address::from(addr));
    }

    pub fn add_one_shot_breakpoint(&mut self, addr: u16) {
        self.debug.add_one_shot_breakpoint(Address::from(addr));
    }

    pub fn add_breakpoint_after(&mut self, addr: u16, target_hits: u64) {
        self.debug
            .add_breakpoint_after(Address::from(addr), target_hits);
    }

    pub fn remove_breakpoint(&mut self, addr: u16) {
        self.debug.remove_breakpoint(Address::from(addr));
    }

    pub fn toggle_breakpoint(&mut self, addr: u16) {
        self.debug.toggle_breakpoint(Address::from(addr));
    }

    pub fn iter_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.debug.iter_breakpoints()
    }

    pub fn iter_one_shot_breakpoints(&self) -> impl Iterator<Item = Address> + '_ {
        self.debug.iter_one_shot_breakpoints()
    }

    pub fn iter_breakpoint_hit_conditions(
        &self,
    ) -> impl Iterator<Item = BreakpointHitCondition> + '_ {
        self.debug.iter_breakpoint_hit_conditions()
    }

    pub fn add_watchpoint_range(&mut self, start: u16, end: u16, watch_type: WatchType) {
        self.debug
            .add_watchpoint_range(Address::from(start), Address::from(end), watch_type);
    }

    pub fn remove_watchpoint(&mut self, start: u16, end: u16, watch_type: WatchType) {
        self.debug
            .remove_watchpoint(Address::from(start), Address::from(end), watch_type);
    }

    pub fn debug_watchpoints(&self) -> &[AddressWatchpoint] {
        &self.debug.watchpoints
    }

    pub fn debug_hit_breakpoint(&self) -> Option<Address> {
        self.debug.hit_breakpoint
    }

    pub fn debug_hit_watchpoint(&self) -> Option<&AddressWatchHit> {
        self.debug.hit_watchpoint.as_ref()
    }

    #[inline]
    pub fn debug_peek_cpu8(&self, logical_addr: u16) -> u8 {
        self.bus
            .peek(self.cpu.cpu().logical_to_physical(logical_addr))
    }

    pub fn debug_peek_physical8(&self, physical_addr: u32) -> u8 {
        self.bus
            .peek(physical_addr & super::cpu::PHYSICAL_ADDRESS_MASK)
    }

    pub fn debug_write_cpu8(&mut self, logical_addr: u16, value: u8) {
        let old_value = self.debug_peek_cpu8(logical_addr);
        self.cpu
            .debug_write_logical(&mut self.bus, logical_addr, value);
        self.debug
            .check_watch_write(Address::from(logical_addr), old_value, value);
        if self.debug.hit_watchpoint.is_some() {
            self.execution_state = PceExecutionState::Suspended;
            self.suspend_after_instruction = false;
        }
    }

    #[inline]
    pub fn rom_offset_for_cpu_address(&self, logical_addr: u16) -> Option<u32> {
        self.bus
            .hucard_rom_offset(self.cpu.cpu().logical_to_physical(logical_addr))
    }

    pub fn rom_mapping_token(&self) -> u64 {
        self.cpu
            .cpu()
            .mapping_registers()
            .into_iter()
            .chain([self.bus.hucard_mapping_token()])
            .fold(0xCBF2_9CE4_8422_2325, |token, byte| {
                (token ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3)
            })
    }

    #[inline]
    pub fn hucard_rom(&self) -> &[u8] {
        self.bus.hucard_rom()
    }

    #[inline]
    pub fn hucard_board(&self) -> PceHuCardBoard {
        self.bus.hucard_board()
    }

    #[inline]
    pub fn hucard_ram(&self) -> Option<&[u8; POPULOUS_HUCARD_RAM_LEN]> {
        self.bus.hucard_ram()
    }

    #[cfg(test)]
    pub(super) fn system_card_ram_mut_for_test(
        &mut self,
    ) -> Option<&mut [u8; super::cartridge::SUPER_SYSTEM_CARD_RAM_LEN]> {
        self.bus.system_card_ram_mut()
    }

    #[inline]
    pub fn work_ram(&self) -> &[u8; super::bus::WORK_RAM_LEN] {
        self.bus.work_ram()
    }

    #[inline]
    pub fn mapped_work_ram(&self) -> &[u8] {
        self.bus.mapped_work_ram()
    }

    #[inline]
    pub const fn hardware_topology(&self) -> PceHardwareTopology {
        self.bus.topology()
    }

    pub fn step_boundary(&mut self) -> Result<PceMachineStep, PceMachineError> {
        self.step_boundary_faulting()
    }

    pub fn run_until_frame(&mut self) -> Result<PceFrameRun, PceMachineError> {
        let starting_ticks = self.master_ticks;
        let mut cpu_boundaries = 0_u64;
        loop {
            if self.is_cpu_suspended() {
                return Ok(PceFrameRun {
                    cpu_boundaries,
                    master_ticks: self.master_ticks - starting_ticks,
                    frames_published: 0,
                });
            }
            if self.skip_breakpoint_once {
                self.skip_breakpoint_once = false;
            } else if !self.suspend_after_instruction {
                let pc = Address::from(self.cpu.cpu().registers().pc);
                if self.debug.should_break(pc) {
                    self.execution_state = PceExecutionState::Suspended;
                    return Ok(PceFrameRun {
                        cpu_boundaries,
                        master_ticks: self.master_ticks - starting_ticks,
                        frames_published: 0,
                    });
                }
            }
            let step = self.step_boundary_faulting()?;
            cpu_boundaries += 1;
            if self.suspend_after_instruction && matches!(step.action, PceCpuAction::Instruction(_))
            {
                self.execution_state = PceExecutionState::Suspended;
                self.suspend_after_instruction = false;
                return Ok(PceFrameRun {
                    cpu_boundaries,
                    master_ticks: self.master_ticks - starting_ticks,
                    frames_published: step.frames_published,
                });
            }
            if step.frames_published != 0 {
                return Ok(PceFrameRun {
                    cpu_boundaries,
                    master_ticks: self.master_ticks - starting_ticks,
                    frames_published: step.frames_published,
                });
            }
        }
    }

    #[inline]
    pub const fn cpu(&self) -> &HuC6280 {
        &self.cpu
    }

    #[inline]
    pub fn cpu_mut(&mut self) -> &mut HuC6280 {
        &mut self.cpu
    }

    #[inline]
    pub fn devices(&self) -> &PceDevices {
        self.bus.devices()
    }

    #[inline]
    pub fn devices_mut(&mut self) -> &mut PceDevices {
        self.bus.devices_mut()
    }

    pub fn set_sample_rate(&mut self, sample_rate: u32) {
        self.bus.devices_mut().set_sample_rate(sample_rate);
    }

    pub fn set_sample_generation_enabled(&mut self, enabled: bool) {
        self.bus
            .devices_mut()
            .set_sample_generation_enabled(enabled);
    }

    pub fn set_channel_mutes(&mut self, mutes: &[bool]) {
        self.bus.devices_mut().set_channel_mutes(mutes);
    }

    pub fn drain_audio_samples_into(&mut self, output: &mut Vec<f32>) {
        self.bus.devices_mut().drain_audio_samples_into(output);
    }

    #[inline]
    pub fn framebuffer(&self) -> &[u8] {
        self.front_video.framebuffer()
    }

    #[inline]
    pub fn presented_frame(&self) -> PcePresentedFrame<'_> {
        self.front_video.presented_frame()
    }

    #[inline]
    pub const fn master_ticks(&self) -> u64 {
        self.master_ticks
    }

    #[inline]
    pub const fn vce_line_accumulator(&self) -> u64 {
        self.vce_line_accumulator
    }

    #[inline]
    pub const fn vdc_pixel_clock_remainder(&self) -> u8 {
        self.vdc_pixel_clock_remainder
    }

    #[inline]
    pub const fn vce_line_index(&self) -> u16 {
        self.vce_line_index
    }

    #[inline]
    pub const fn vce_frame_length(&self) -> VceFrameLength {
        self.vce_frame_length
    }

    #[inline]
    pub const fn faulted(&self) -> bool {
        self.faulted
    }

    fn step_boundary_faulting(&mut self) -> Result<PceMachineStep, PceMachineError> {
        self.step_boundary_faulting_with(|cpu, bus| {
            Ok(match cpu.service_interrupt_boundary(bus) {
                Some(step) => PceCpuAction::Interrupt(step),
                None => PceCpuAction::Instruction(cpu.step_instruction(bus)?),
            })
        })
    }

    fn step_boundary_faulting_with(
        &mut self,
        execute: impl FnOnce(&mut HuC6280, &mut TimedMachineBus<'_>) -> Result<PceCpuAction, CpuTrap>,
    ) -> Result<PceMachineStep, PceMachineError> {
        if self.faulted {
            return Err(PceMachineError::FaultedUntilReset);
        }
        let result = self.step_boundary_inner_with(execute);
        if result.is_err() {
            self.faulted = true;
        }
        result
    }

    fn step_boundary_inner_with(
        &mut self,
        execute: impl FnOnce(&mut HuC6280, &mut TimedMachineBus<'_>) -> Result<PceCpuAction, CpuTrap>,
    ) -> Result<PceMachineStep, PceMachineError> {
        let logical_pc = self.cpu.cpu().registers().pc;
        let physical_pc = self.cpu.cpu().logical_to_physical(logical_pc);
        let trace_enabled = self.instruction_trace.is_enabled();
        let trace_registers_before = trace_enabled.then(|| pce_trace_registers(&self.cpu));
        let trace_rom_offset = trace_enabled
            .then(|| self.bus.hucard_rom_offset(physical_pc))
            .flatten();
        let trace_frame = self.trace_frame;
        let trace_cycle = self.master_ticks;
        let trace_bank = physical_pc >> 13;
        self.refresh_vdc_irq1();
        self.refresh_cdrom2_irq2();
        let entering_speed = self.cpu.cpu().speed_mode();
        let master_ticks_per_cycle = master_ticks_per_cpu_cycle(entering_speed);
        let (
            action,
            trap,
            error,
            wait_cycles,
            contention_wait_cycles,
            master_ticks,
            vce_lines,
            frames_published,
            instruction_bytes,
            trace_writes,
            trace_write_overflow,
            dma_completed,
        ) = {
            let mut bus = TimedMachineBus::new(
                &mut self.bus,
                &mut self.front_video,
                &mut self.back_video,
                &mut self.vce_line_accumulator,
                &mut self.vdc_pixel_clock_remainder,
                &mut self.vce_line_index,
                &mut self.vce_frame_length,
                master_ticks_per_cycle,
                trace_enabled,
                &mut self.debug,
            );
            let mut action = None;
            let mut trap = None;
            let mut error = None;
            match execute(&mut self.cpu, &mut bus) {
                Ok(completed) => match bus.advance_remaining(completed.cycles()) {
                    Ok(()) => action = Some(completed),
                    Err(failure) => error = Some(failure),
                },
                Err(cpu_trap) => trap = Some(cpu_trap),
            }
            self.cpu
                .advance_master_ticks(bus.take_elapsed_master_ticks());
            error = bus.fault.or(error);
            (
                action,
                trap,
                error,
                bus.video_wait_cycles,
                bus.vram_contention_wait_cycles,
                bus.elapsed_master_ticks,
                bus.vce_lines,
                bus.frames_published,
                bus.instruction_bytes,
                bus.trace_writes,
                bus.trace_write_overflow,
                bus.dma_completed,
            )
        };
        self.commit_elapsed_cpu_time(master_ticks)?;
        if error.is_none() && trap.is_none() {
            self.cpu.sample_interrupts_after_action();
        }
        if let Some(error) = error {
            return Err(error);
        }
        if let Some(trap) = trap {
            return Err(PceMachineError::CpuTrap(trap));
        }
        let action = action.expect("successful CPU action is present");
        if trace_enabled {
            let mut record = InstructionTraceRecord::new(
                TraceExecMode::HuC6280,
                u32::from(logical_pc),
                trace_rom_offset.map(u64::from),
                trace_frame,
                trace_cycle,
                if matches!(action, PceCpuAction::Interrupt(_)) {
                    &[]
                } else {
                    &instruction_bytes
                },
            );
            record.bank = Some(trace_bank);
            if matches!(action, PceCpuAction::Interrupt(_)) {
                record.event = Some(DebugEvent::Interrupt);
            } else if dma_completed {
                record.event = Some(DebugEvent::Dma);
            }
            for write in trace_writes {
                record.push_write(write);
            }
            record.write_overflow = trace_write_overflow;
            push_pce_register_deltas(
                &mut record,
                &trace_registers_before.expect("trace state is present"),
                &pce_trace_registers(&self.cpu),
            );
            self.instruction_trace.push(record);
        }
        if let PceCpuAction::Instruction(step) = action {
            self.opcode_history.push(PceOpcodeHistoryEntry {
                logical_pc: step.pc,
                physical_pc,
                opcode: step.opcode,
                master_ticks: self.master_ticks,
            });
        }
        let hit_event = matches!(action, PceCpuAction::Interrupt(_))
            && self.debug.check_event(DebugEvent::Interrupt)
            || dma_completed && self.debug.check_event(DebugEvent::Dma);
        if hit_event {
            self.execution_state = PceExecutionState::Suspended;
            self.suspend_after_instruction = false;
        }
        if self.debug.hit_watchpoint.is_some() {
            self.execution_state = PceExecutionState::Suspended;
            self.suspend_after_instruction = false;
        }
        self.trace_frame = self.trace_frame.wrapping_add(frames_published);
        Ok(PceMachineStep {
            action,
            entering_speed,
            wait_cycles,
            vram_contention_wait_cycles: contention_wait_cycles,
            master_ticks,
            vce_lines,
            frames_published,
        })
    }

    fn commit_elapsed_cpu_time(&mut self, master_ticks: u64) -> Result<(), PceMachineError> {
        let next_master_ticks = checked_clock_add(
            self.master_ticks,
            master_ticks,
            PceClockCounter::MasterTicks,
        )?;
        self.master_ticks = next_master_ticks;
        self.refresh_vdc_irq1();
        self.refresh_cdrom2_irq2();
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn force_unsupported_opcode_trap_after_fetch(
        &mut self,
    ) -> Result<PceMachineStep, PceMachineError> {
        self.step_boundary_faulting_with(|cpu, bus| {
            let pc = cpu.cpu().registers().pc;
            let opcode = bus.read(cpu.cpu().logical_to_physical(pc));
            cpu.cpu_mut().registers_mut().pc = pc.wrapping_add(1);
            Err(CpuTrap::UnsupportedOpcode { pc, opcode })
        })
    }

    #[cfg(test)]
    pub(super) fn advance_devices_for_test(
        &mut self,
        master_ticks: u64,
    ) -> Result<(u64, u64), PceMachineError> {
        let (error, elapsed, lines, frames) = {
            let mut bus = TimedMachineBus::new(
                &mut self.bus,
                &mut self.front_video,
                &mut self.back_video,
                &mut self.vce_line_accumulator,
                &mut self.vdc_pixel_clock_remainder,
                &mut self.vce_line_index,
                &mut self.vce_frame_length,
                1,
                false,
                &mut self.debug,
            );
            bus.advance_devices(master_ticks);
            (
                bus.fault,
                bus.elapsed_master_ticks,
                bus.vce_lines,
                bus.frames_published,
            )
        };
        self.cpu.advance_master_ticks(elapsed);
        self.commit_elapsed_cpu_time(elapsed)?;
        if let Some(error) = error {
            return Err(error);
        }
        Ok((lines, frames))
    }

    #[inline]
    fn refresh_vdc_irq1(&mut self) {
        self.cpu.set_irq1_line(self.bus.devices().vdc_irq_level());
    }

    #[inline]
    fn refresh_cdrom2_irq2(&mut self) {
        self.cpu
            .set_irq2_line(self.bus.devices().cdrom2_irq_level());
    }
}

impl CheatByteTarget<u16> for PceMachine {
    fn cheat_peek8(&self, address: u16) -> u8 {
        self.debug_peek_cpu8(address)
    }

    fn cheat_write8(&mut self, address: u16, value: u8) {
        self.cpu.debug_write_logical(&mut self.bus, address, value);
    }
}

fn pce_trace_registers(cpu: &HuC6280) -> [u32; 15] {
    let registers = cpu.cpu().registers();
    let mapping = cpu.cpu().mapping_registers();
    [
        u32::from(registers.a),
        u32::from(registers.x),
        u32::from(registers.y),
        u32::from(registers.sp),
        u32::from(registers.pc),
        u32::from(registers.status.bits()),
        u32::from(mapping[0]),
        u32::from(mapping[1]),
        u32::from(mapping[2]),
        u32::from(mapping[3]),
        u32::from(mapping[4]),
        u32::from(mapping[5]),
        u32::from(mapping[6]),
        u32::from(mapping[7]),
        u32::from(matches!(cpu.cpu().speed_mode(), SpeedMode::High)),
    ]
}

fn push_pce_register_deltas(
    record: &mut InstructionTraceRecord,
    before: &[u32; 15],
    after: &[u32; 15],
) {
    for (register, (&before, &after)) in before.iter().zip(after).enumerate() {
        if before != after {
            record.push_register_delta(RegisterDelta {
                register: register as u8,
                value: after,
            });
        }
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

#[inline]
const fn master_ticks_per_cpu_cycle(speed: SpeedMode) -> u64 {
    match speed {
        SpeedMode::Low => PROVISIONAL_PCE_LOW_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
        SpeedMode::High => PROVISIONAL_PCE_HIGH_SPEED_MASTER_TICKS_PER_CPU_CYCLE,
    }
}

#[inline]
const fn is_vdc_vce_access(physical_addr: u32) -> bool {
    matches!(
        physical_addr & super::cpu::PHYSICAL_ADDRESS_MASK,
        0x1F_E000..=0x1F_E7FF
    )
}

#[inline]
fn vdc_vram_cycle_target(
    bus: &BaseBus<PceDevices>,
    physical_addr: u32,
    write: bool,
) -> Option<VpcVdc> {
    let (target, port) = match bus.decode_physical_region(physical_addr) {
        PhysicalRegion::Vdc(port) => (VpcVdc::One, port),
        PhysicalRegion::Vdc2(port) => (VpcVdc::Two, port),
        _ => return None,
    };
    is_vdc_vram_port_cycle(bus, target, port, write).then_some(target)
}

#[inline]
fn direct_vdc_vram_write_target(bus: &BaseBus<PceDevices>, port: VdcPort) -> Option<VpcVdc> {
    let target = bus.devices().direct_vdc_target();
    is_vdc_vram_port_cycle(bus, target, port, true).then_some(target)
}

#[inline]
fn is_vdc_vram_port_cycle(
    bus: &BaseBus<PceDevices>,
    target: VpcVdc,
    port: VdcPort,
    write: bool,
) -> bool {
    if port != VdcPort::DataHigh {
        return false;
    }
    let Some(vdc) = bus.devices().vdc_for(target) else {
        return false;
    };
    match vdc.selected_register() {
        Some(super::vdc::VdcRegister::MemoryAddressRead) => write,
        Some(super::vdc::VdcRegister::VramData) => true,
        _ => false,
    }
}
