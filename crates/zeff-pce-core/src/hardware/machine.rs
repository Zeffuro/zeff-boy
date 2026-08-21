use std::error::Error;
use std::fmt::{Display, Formatter};

use super::bus::{BaseBus, BaseBusError, OPEN_BUS_VALUE, PceHardwareTopology, PhysicalRegion};
use super::cartridge::{
    POPULOUS_HUCARD_RAM_LEN, PceCartridgeDescriptor, PceCartridgeHardware, PceConsoleWiring,
    PceHuCardBoard,
};
use super::cd_media::CdDisc;
use super::controller::ControllerPort;
use super::cpu::{
    CpuBus, CpuStep, CpuTrap, HuC6280, InterruptStep, LineLevel, Registers, SpeedMode, VdcPort,
};
use super::pce_devices::PceDevices;
use super::psg::PsgRevision;
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
    speed_mode: SpeedMode,
    master_ticks: u64,
    vce_line_index: u16,
    faulted: bool,
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
    pub const fn speed_mode(self) -> SpeedMode {
        self.speed_mode
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
        }
    }

    fn advance_cycle(&mut self) {
        self.observed_cycles += 1;
        self.advance_devices(self.master_ticks_per_cycle);
    }

    fn advance_access(&mut self, physical_addr: u32, write: bool) -> bool {
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
        self.fault.is_none()
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
            remaining -= elapsed;
            if let Err(error) = result {
                self.fault = Some(error);
                return;
            }
        }
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
        }
    }

    fn write_vdc(&mut self, port: VdcPort, value: u8) {
        if self.advance_direct_vdc_access(port) {
            self.inner.write_vdc(port, value);
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

    fn idle(&mut self) {
        self.advance_cycle();
        self.inner.idle();
    }
}

impl PceMachine {
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
        if !matches!(
            system_card_board,
            PceHuCardBoard::SystemCardV1V2 | PceHuCardBoard::SystemCardV3
        ) {
            return Err(PceMachineError::InvalidSystemCardBoard(system_card_board));
        }
        let bus = BaseBus::with_hucard(
            system_card_rom,
            system_card_board,
            PceDevices::with_cdrom2_system_card(
                controller,
                console_wiring,
                disc,
                system_card_board == PceHuCardBoard::SystemCardV3,
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
        self.refresh_cdrom2_irq2();
    }

    #[inline]
    pub fn debug_snapshot(&self) -> PceCpuDebugSnapshot {
        PceCpuDebugSnapshot {
            registers: self.cpu.cpu().registers(),
            mapping_registers: self.cpu.cpu().mapping_registers(),
            speed_mode: self.cpu.cpu().speed_mode(),
            master_ticks: self.master_ticks,
            vce_line_index: self.vce_line_index,
            faulted: self.faulted,
        }
    }

    #[inline]
    pub fn debug_peek_cpu8(&self, logical_addr: u16) -> u8 {
        self.bus
            .peek(self.cpu.cpu().logical_to_physical(logical_addr))
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
            let step = self.step_boundary_faulting()?;
            cpu_boundaries += 1;
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
