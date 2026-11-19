use super::constants::*;
use super::{
    ControlCommand, VdpControl, VdpInterrupts, VdpRegisterError, VdpRegisterPipeline,
    VdpRegisterWrite, VdpScheduleError,
};
use crate::Region;
use zeff_emu_common::time::MasterTicks;

mod ports;
pub use ports::VdpPortError;
mod machine;
pub(crate) use machine::ZeroFillWrite;
pub use machine::{Vdp, VdpLineError, VdpScanline};
mod clock;
mod data;
mod dma;
mod external;
mod pending_fill;
mod read;
mod status;
pub use status::VdpStatusError;

#[cfg(test)]
pub(crate) mod position_tests;

#[derive(Clone, Debug, PartialEq, Eq)]
struct State {
    registers: VdpRegisterPipeline,
    interrupts: VdpInterrupts,
    control: VdpControl,
    command: ControlCommand,
    fifo: super::VdpWriteFifo,
    cache: super::VdpReadCache,
    read_phase: Option<bool>,
    dma: Option<dma::Phase>,
    hv_latch: Option<u16>,
    status_known: bool,
    memory_mode_known: bool,
    sprites: super::VdpSpriteCache,
}

impl State {
    fn display_height(&self, region: Region) -> usize {
        if self
            .interrupts
            .blank_history()
            .is_some_and(|history| history.vertical.is_some())
            && self.registers.registers()[usize::from(MODE_REGISTER)]
                .is_some_and(|mode| mode & V30_ENABLE != 0)
        {
            V30_VBLANK_SET_LINE as usize
        } else {
            self.registers.display_height(region)
        }
    }

    pub fn bootstrap_mode5() -> Self {
        let mut value = Self {
            registers: VdpRegisterPipeline::bootstrap_mode5(),
            interrupts: VdpInterrupts::at_counters_zero(MasterTicks::ZERO, 0, false, false),
            fifo: super::VdpWriteFifo::default(),
            cache: super::VdpReadCache::default(),
            read_phase: None,
            dma: None,
            hv_latch: None,
            status_known: true,
            memory_mode_known: true,
            sprites: super::VdpSpriteCache::default(),
            control: VdpControl::default(),
            command: ControlCommand {
                address: 0,
                code: 0,
            },
        };
        value.interrupts.track_software_origin();
        value
    }

    pub fn from_known_state(
        mut interrupts: VdpInterrupts,
        mut registers: VdpRegisterPipeline,
        region: Region,
    ) -> Result<Self, VdpScheduleError> {
        interrupts.advance_pair(interrupts.now(), region, &mut registers, false)?;
        interrupts.interrupt_level(&registers)?;
        let mut device = Self {
            registers,
            interrupts,
            fifo: super::VdpWriteFifo::default(),
            cache: super::VdpReadCache::default(),
            read_phase: None,
            dma: None,
            hv_latch: None,
            status_known: false,
            memory_mode_known: registers.registers()[usize::from(MODE_REGISTER)]
                .is_some_and(|mode| mode & DISPLAY_ENABLE == 0),
            sprites: super::VdpSpriteCache::default(),
            control: VdpControl::default(),
            command: ControlCommand {
                address: 0,
                code: 0,
            },
        };
        if let Some(write) = device.registers.pending() {
            device.registers = device.registers.published_state().1;
            device.validate_write(write.index, write.value, region)?;
            device.validate_clock_write(write, region)?;
            let next = device
                .interrupts
                .timing_next_edge(region, &device.registers)?;
            if next.is_some_and(|edge| edge.at == write.publish_at) {
                return Err(VdpScheduleError::CoincidentPublication(write.publish_at));
            }
            device.registers = registers;
        }
        if device.registers.registers()[usize::from(HINT_ENABLE_REGISTER)]
            .is_some_and(|value| value & HV_COUNTER_LATCH_MASK != 0)
        {
            device.hv_latch = Some(
                device
                    .interrupts
                    .hv_counter(region, &device.registers.published_state().1)?,
            );
        }
        Ok(device)
    }

    pub fn now(&self) -> MasterTicks {
        self.interrupts.now()
    }
    pub fn registers(&self) -> &VdpRegisterPipeline {
        &self.registers
    }
    pub fn interrupts(&self) -> &VdpInterrupts {
        &self.interrupts
    }
    pub fn interrupt_level(&self) -> u8 {
        self.interrupts
            .interrupt_level(&self.registers)
            .expect("validated VDP interrupt image")
    }

    pub(super) fn read_hv(&self, region: Region) -> Result<u16, VdpScheduleError> {
        let current = self.interrupts.hv_counter(region, &self.registers)?;
        Ok(self.hv_latch.unwrap_or(current))
    }

    fn validate_write(&self, index: u8, value: u8, region: Region) -> Result<(), VdpScheduleError> {
        if self.dma.is_some() && !self.waiting_register_write(index, value) {
            return Err(VdpScheduleError::Dma(super::VdpDmaError::Busy));
        }
        let changes_mode = self.registers.changes_memory_mode(index, value);
        if self.interrupts.m5_vertical()
            && ((index == 0 && value & 1 != 0)
                || index == MODE_REGISTER
                    && value & (MODE5_ENABLE | VRAM_128K_ENABLE) != MODE5_ENABLE)
        {
            return Err(VdpRegisterError::UnsupportedModeChange { index, value }.into());
        }
        if self.read_phase.is_some() && (changes_mode || index == AUTO_INCREMENT_REGISTER) {
            #[cfg(feature = "first-failure-diagnostic")]
            self.record_rejection(crate::diagnostic::RejectionSite::ReadPhase, index, value);
            return Err(VdpRegisterError::UnsupportedModeChange { index, value }.into());
        }
        if !self.fifo.is_empty()
            && changes_mode
            && !self.registers.changes_display_enable(index, value)
            && !self.changes_vertical_mode(index, value)
            && !self.changes_progressive_clock(index, value)
        {
            #[cfg(feature = "first-failure-diagnostic")]
            self.record_rejection(crate::diagnostic::RejectionSite::Fifo, index, value);
            return Err(VdpRegisterError::UnsupportedModeChange { index, value }.into());
        }
        let blank_transition = index == MODE_REGISTER && value & MODE5_ENABLE == 0;
        if blank_transition
            && (!self.fifo.is_empty()
                || self.cache.is_ready()
                || self.read_phase.is_some()
                || self.dma.is_some()
                || !self
                    .interrupts
                    .blank_transition_supported(&self.registers, value))
        {
            return Err(VdpRegisterError::UnsupportedModeChange { index, value }.into());
        }
        if self.interrupts.staged_blank()
            && (index == MODE_REGISTER
                && value & (DISPLAY_ENABLE | V30_ENABLE | VRAM_128K_ENABLE) != 0
                || index == DISPLAY_SIZE_REGISTER && value != 0
                || index == 0 && value & 1 != 0)
        {
            return Err(VdpRegisterError::UnsupportedModeChange { index, value }.into());
        }
        if (index == MODE_REGISTER
            && value & V30_ENABLE != 0
            && region != Region::Pal
            && !self
                .interrupts
                .vertical_change_supported(&self.registers, value, region))
            || (index == DISPLAY_SIZE_REGISTER
                && (!matches!(value & INTERLACE_MODE_MASK, 0 | 2 | INTERLACE_MODE_MASK)
                    || super::mode::HorizontalMode::from_register(value).is_none()))
        {
            #[cfg(feature = "first-failure-diagnostic")]
            self.record_rejection(
                crate::diagnostic::RejectionSite::UnsupportedMode,
                index,
                value,
            );
            return Err(VdpRegisterError::UnsupportedModeChange { index, value }.into());
        }
        if index == DISPLAY_SIZE_REGISTER
            && value & INTERLACE_MODE_MASK != 0
            && (value & H40_CLOCK_MASK == 1 || self.interrupts.has_timing_history())
        {
            #[cfg(feature = "first-failure-diagnostic")]
            self.record_rejection(
                crate::diagnostic::RejectionSite::InterlaceHistory,
                index,
                value,
            );
            return Err(VdpRegisterError::UnsupportedModeChange { index, value }.into());
        }
        Ok(())
    }

    pub fn queue_register(
        &mut self,
        index: u8,
        value: u8,
        region: Region,
    ) -> Result<VdpRegisterWrite, VdpScheduleError> {
        let mut candidate = self.clone();
        if candidate.interrupts.staged_blank() {
            return Err(super::VdpInterruptError::Timing(
                super::VdpTimingError::UnqualifiedDisplayHistory,
            )
            .into());
        }
        if let Some(mut blank) = candidate.interrupts.blank_history() {
            blank.retire_port();
            candidate.interrupts.set_blank_history(Some(blank));
        }
        let result = candidate.queue_register_at(index, value, region, false)?;
        *self = candidate;
        Ok(result)
    }

    pub(super) fn queue_control_register(
        &mut self,
        index: u8,
        value: u8,
        region: Region,
    ) -> Result<VdpRegisterWrite, VdpScheduleError> {
        self.queue_register_at(index, value, region, true)
    }

    fn queue_register_at(
        &mut self,
        index: u8,
        value: u8,
        region: Region,
        allow_edge: bool,
    ) -> Result<VdpRegisterWrite, VdpScheduleError> {
        self.validate_write(index, value, region)?;
        let mut candidate = self.registers;
        let write = if index == MODE_REGISTER && value & MODE5_ENABLE == 0 {
            candidate.queue_qualified_blank(index, value, allow_edge)?
        } else if allow_edge {
            candidate.queue_control_register(index, value)?
        } else {
            candidate.queue_register(index, value)?
        };
        self.validate_clock_write(write, region)?;
        if !allow_edge {
            let next = self.interrupts.timing_next_edge(region, &self.registers)?;
            if next.is_some_and(|edge| edge.at == write.publish_at) {
                return Err(VdpScheduleError::CoincidentPublication(write.publish_at));
            }
        }
        self.registers = candidate;
        Ok(write)
    }

    pub fn advance_to(&mut self, at: MasterTicks, region: Region) -> Result<(), VdpScheduleError> {
        if !self.pending_status_known() {
            return Err(super::VdpInterruptError::Timing(
                super::VdpTimingError::UnqualifiedDisplayHistory,
            )
            .into());
        }
        let mut interrupts = self.interrupts;
        let mut registers = self.registers;
        let mut memory_mode_known = self.memory_mode_known;
        let mut hv_latch = self.hv_latch;
        let mut status_known = self.status_known
            && registers.registers()[usize::from(MODE_REGISTER)]
                .is_some_and(|mode| mode & DISPLAY_ENABLE == 0);
        let mut control = self.control;
        loop {
            if let Some(next) = interrupts.skip_cycles(at, region, &registers)? {
                registers.advance_to(next)?;
            }
            let (boundary, mut position) = interrupts.cycle_boundary(region, &registers)?;
            let horizon = boundary.map_or(at, |boundary| boundary.min(at));
            loop {
                let previous_mode = registers.registers()[usize::from(MODE_REGISTER)];
                let previous_latch = registers.registers()[usize::from(HINT_ENABLE_REGISTER)]
                    .is_some_and(|value| value & HV_COUNTER_LATCH_MASK != 0);
                let uncaptured = interrupts.blank_history().is_some_and(|b| {
                    b.port.is_some_and(|p| {
                        p.kind == super::timing::blank_mode::PortKind::Status
                            && p.captured.is_none()
                    })
                });
                let event = if position.is_some() {
                    let result = interrupts.advance_pair_with_position(
                        horizon,
                        region,
                        &mut registers,
                        false,
                        &mut position,
                    );
                    position = None;
                    result?
                } else {
                    interrupts.advance_pair(horizon, region, &mut registers, false)?
                };
                if uncaptured
                    && let Some(mut blank) = interrupts.blank_history()
                    && blank.vertical.is_some()
                    && let Some(port) = &mut blank.port
                    && let Some(word) = &mut port.captured
                {
                    *word = (*word & !(FIFO_EMPTY_STATUS | FIFO_FULL_STATUS | DMA_BUSY_STATUS))
                        | self.fifo.status_bits()
                        | if self.dma.is_some() {
                            DMA_BUSY_STATUS
                        } else {
                            0
                        };
                    interrupts.set_blank_history(Some(blank));
                }
                let next_mode = registers.registers()[usize::from(MODE_REGISTER)];
                let known_vertical = interrupts.m5_vertical()
                    && previous_mode.zip(next_mode).is_some_and(|(old, new)| {
                        (old ^ new) & MEMORY_MODE_MASK & !(DISPLAY_ENABLE | V30_ENABLE) == 0
                    });
                memory_mode_known &= known_vertical
                    || previous_mode.map(|mode| mode & MEMORY_MODE_MASK)
                        == next_mode.map(|mode| mode & MEMORY_MODE_MASK);
                if let Some(mut blank) = interrupts.blank_history()
                    && blank.cancel_control
                {
                    control.cancel_pending();
                    blank.cancel_control = false;
                    interrupts.set_blank_history(Some(blank));
                }
                let latched = registers.registers()[usize::from(HINT_ENABLE_REGISTER)]
                    .is_some_and(|value| value & HV_COUNTER_LATCH_MASK != 0);
                if !latched {
                    hv_latch = None;
                } else if !previous_latch {
                    hv_latch = Some(interrupts.hv_counter(region, &registers)?);
                }
                status_known &= registers.registers()[usize::from(MODE_REGISTER)]
                    .is_some_and(|mode| mode & DISPLAY_ENABLE == 0);
                if event.is_none() {
                    break;
                }
            }
            if horizon == at {
                self.interrupts = interrupts;
                self.registers = registers;
                self.memory_mode_known = memory_mode_known;
                self.hv_latch = hv_latch;
                self.status_known = status_known;
                self.control = control;
                return Ok(());
            }
        }
    }

    pub fn acknowledge_to(
        &mut self,
        at: MasterTicks,
        region: Region,
    ) -> Result<u8, VdpScheduleError> {
        if at <= self.now() {
            return self
                .interrupts
                .acknowledge_interval(at, region, &mut self.registers, false);
        }
        let mut candidate = self.clone();
        candidate.advance_to(MasterTicks::new(at.get() - 1), region)?;
        let level = candidate.interrupts.acknowledge_interval(
            at,
            region,
            &mut candidate.registers,
            false,
        )?;
        *self = candidate;
        Ok(level)
    }
}
