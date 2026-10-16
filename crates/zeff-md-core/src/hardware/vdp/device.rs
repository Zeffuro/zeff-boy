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
pub use machine::{Vdp, VdpLineError, VdpScanline};
mod data;
mod dma;
mod external;
mod read;
mod status;
pub use status::VdpStatusError;

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
    /// Deterministic mode-5 bootstrap, with display and interrupts disabled.
    pub fn bootstrap_mode5() -> Self {
        Self {
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
        }
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
        if self.dma.is_some() {
            return Err(VdpScheduleError::Dma(super::VdpDmaError::Busy));
        }
        let changes_mode = self.registers.changes_memory_mode(index, value);
        if self.read_phase.is_some() && (changes_mode || index == AUTO_INCREMENT_REGISTER) {
            return Err(VdpRegisterError::UnsupportedModeChange { index, value }.into());
        }
        if !self.fifo.is_empty()
            && changes_mode
            && !self.registers.changes_display_enable(index, value)
        {
            return Err(VdpRegisterError::UnsupportedModeChange { index, value }.into());
        }
        if (index == MODE_REGISTER
            && (value & MODE5_ENABLE == 0 || (value & V30_ENABLE != 0 && region != Region::Pal)))
            || (index == DISPLAY_SIZE_REGISTER
                && (!matches!(value & INTERLACE_MODE_MASK, 0 | INTERLACE_MODE_MASK)
                    || super::mode::HorizontalMode::from_register(value).is_none()))
        {
            return Err(VdpRegisterError::UnsupportedModeChange { index, value }.into());
        }
        Ok(())
    }

    fn pending_fifo_display_change(&self) -> Option<VdpRegisterWrite> {
        self.registers.pending().filter(|write| {
            self.read_phase.is_none()
                && self.dma.is_none()
                && !self.fifo.is_empty()
                && self
                    .registers
                    .changes_display_enable(write.index, write.value)
        })
    }

    pub fn queue_register(
        &mut self,
        index: u8,
        value: u8,
        region: Region,
    ) -> Result<VdpRegisterWrite, VdpScheduleError> {
        self.queue_register_at(index, value, region, false)
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
        let write = if allow_edge {
            candidate.queue_control_register(index, value)?
        } else {
            candidate.queue_register(index, value)?
        };
        let next = self.interrupts.timing_next_edge(region, &self.registers)?;
        if !allow_edge && next.is_some_and(|edge| edge.at == write.publish_at) {
            return Err(VdpScheduleError::CoincidentPublication(write.publish_at));
        }
        self.registers = candidate;
        Ok(write)
    }

    pub fn advance_to(&mut self, at: MasterTicks, region: Region) -> Result<(), VdpScheduleError> {
        let mut interrupts = self.interrupts;
        let mut registers = self.registers;
        let mut memory_mode_known = self.memory_mode_known;
        let mut hv_latch = self.hv_latch;
        let mut status_known = self.status_known
            && registers.registers()[usize::from(MODE_REGISTER)]
                .is_some_and(|mode| mode & DISPLAY_ENABLE == 0);
        loop {
            if let Some(next) = interrupts.skip_cycles(at, region, &registers)? {
                registers.advance_to(next)?;
            }
            let horizon = interrupts
                .cycle_boundary(region, &registers)?
                .map_or(at, |boundary| boundary.min(at));
            loop {
                let previous_mode = registers.registers()[usize::from(MODE_REGISTER)];
                let previous_latch = registers.registers()[usize::from(HINT_ENABLE_REGISTER)]
                    .is_some_and(|value| value & HV_COUNTER_LATCH_MASK != 0);
                let event = interrupts.advance_pair(horizon, region, &mut registers, false)?;
                memory_mode_known &= previous_mode.map(|mode| mode & MEMORY_MODE_MASK)
                    == registers.registers()[usize::from(MODE_REGISTER)]
                        .map(|mode| mode & MEMORY_MODE_MASK);
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
