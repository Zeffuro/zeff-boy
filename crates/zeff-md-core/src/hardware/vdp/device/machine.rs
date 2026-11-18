#[path = "rollback_memory.rs"]
mod rollback_memory;
use super::*;
use crate::hardware::vdp::VdpDmaSource;
use crate::hardware::vdp::memory::MemorySnapshot;
use crate::hardware::vdp::{ControlWrite, VdpMemory, VdpWriteFifo};

#[path = "interval.rs"]
mod interval;
#[path = "owned.rs"]
mod owned;
#[path = "zero_fill_port.rs"]
mod zero_fill_port;
pub(crate) use zero_fill_port::ZeroFillWrite;
#[path = "line.rs"]
mod line;
#[path = "timed_port.rs"]
mod timed_port;
pub use line::{VdpLineError, VdpScanline};
#[cfg(feature = "first-failure-diagnostic")]
#[path = "diagnostic.rs"]
mod diagnostic;
#[path = "long.rs"]
mod long;
#[path = "state.rs"]
mod state;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
#[path = "clock_transition_tests.rs"]
mod clock_transition_tests;

#[cfg(test)]
#[path = "internal_clock_tests.rs"]
mod internal_clock_tests;

#[derive(Clone)]
pub struct Vdp {
    state: State,
    memory: VdpMemory,
    interval_hint: Option<interval::IntervalHint>,
    dma_activity_epoch: u64,
    owned: Option<Box<crate::hardware::vdp::owner::runtime::Runtime>>,
}

pub(crate) struct Checkpoint {
    state: State,
    memory: Option<MemorySnapshot>,
    dma_activity_epoch: u64,
    owned: Option<Box<crate::hardware::vdp::owner::runtime::Runtime>>,
}

impl Vdp {
    pub(crate) fn display_height(&self, region: Region) -> usize {
        if let Some(runtime) = &self.owned {
            return usize::from(runtime.geometry().height);
        }
        self.state.display_height(region)
    }

    pub(crate) fn vertical_next_field(&self) -> Option<bool> {
        self.assert_legacy();
        self.state
            .interrupts
            .blank_history()
            .filter(|b| b.vertical.is_some())
            .map(|b| b.core.wrap)
    }
    pub(crate) fn scroll_fetch_phase(&self, region: Region) -> Result<u64, VdpScheduleError> {
        self.require_legacy()?;
        self.state
            .interrupts
            .scroll_fetch_phase(region, &self.state.registers)
            .map_err(|error| super::super::VdpInterruptError::from(error).into())
    }
    pub(crate) fn is_scroll_fetch(&self, region: Region) -> bool {
        self.assert_legacy();
        self.state
            .interrupts
            .is_scroll_fetch(region, &self.state.registers)
    }

    pub(crate) fn next_scroll_deadline(
        &self,
        region: Region,
    ) -> Result<Option<MasterTicks>, VdpScheduleError> {
        self.require_legacy()?;
        match self
            .state
            .interrupts
            .next_scroll_deadline(region, &self.state.registers)
        {
            Ok(at) => Ok(Some(at)),
            Err(super::super::VdpTimingError::ClockOverflow) => Ok(None),
            Err(error) => Err(super::super::VdpInterruptError::from(error).into()),
        }
    }

    pub(crate) fn scroll_fetch_target(&self, region: Region) -> Option<(u64, u16)> {
        self.assert_legacy();
        if let Some(b) = self
            .state
            .interrupts
            .blank_history()
            .filter(|b| b.vertical.is_some())
        {
            if !self.is_scroll_fetch(region) {
                return None;
            }
            let (id, row) = if b.core.wrap {
                (b.core.field_id.checked_add(1)?, 0)
            } else {
                (b.core.field_id, b.core.row.checked_add(1)?)
            };
            return (usize::from(row) < self.state.display_height(region)).then_some((id, row));
        }
        let field = self.field_position(region).ok()?;
        let (id, row) = if u64::from(field.line) + 1 == field.lines {
            (field.id.checked_add(1)?, 0)
        } else {
            (field.id, field.line.checked_add(1)?)
        };
        (row < if region == Region::Pal { 240 } else { 224 }).then_some((id, row))
    }

    pub(crate) fn scroll_preparation(&self, region: Region) -> bool {
        self.assert_legacy();
        if let Some(b) = self
            .state
            .interrupts
            .blank_history()
            .filter(|b| b.vertical.is_some())
        {
            return self.is_scroll_fetch(region) && b.core.v == 511 && !b.core.wrap;
        }
        self.field_position(region)
            .is_ok_and(|field| u64::from(field.line) + 2 == field.lines)
    }

    pub(crate) fn fetched_horizontal_scroll(&self, row: u16) -> Option<[u16; 2]> {
        self.assert_legacy();
        self.memory
            .horizontal_scroll(&self.state.registers, row)
            .ok()
    }

    pub(crate) fn display_enabled(&self) -> bool {
        if let Some(runtime) = &self.owned {
            return runtime.geometry().display_enabled;
        }
        self.state.registers.registers()[usize::from(MODE_REGISTER)]
            .is_some_and(|value| value & DISPLAY_ENABLE != 0)
    }

    pub(crate) fn effective_line_ticks(&self) -> u64 {
        self.assert_legacy();
        self.state.interrupts.effective_line_ticks()
    }

    pub(crate) fn next_row_deadline(
        &self,
        region: Region,
    ) -> Result<Option<MasterTicks>, VdpScheduleError> {
        self.require_legacy()?;
        match self
            .state
            .interrupts
            .next_row_deadline(region, &self.state.registers)
        {
            Ok(at) => Ok(Some(at)),
            Err(super::super::VdpTimingError::ClockOverflow) => Ok(None),
            Err(error) => Err(super::super::VdpInterruptError::from(error).into()),
        }
    }

    pub(crate) fn field_position(
        &self,
        region: Region,
    ) -> Result<crate::hardware::vdp::FieldPosition, VdpScheduleError> {
        self.require_legacy()?;
        self.state
            .interrupts
            .field_position(region, &self.state.registers)
            .map_err(|error| super::super::VdpInterruptError::from(error).into())
    }

    pub(crate) fn completed_row(
        &self,
        at: MasterTicks,
        region: Region,
    ) -> Result<Option<(u16, crate::hardware::vdp::FieldPosition)>, VdpScheduleError> {
        self.require_legacy()?;
        self.state
            .interrupts
            .completed_row(at, region, &self.state.registers)
            .map_err(|error| super::super::VdpInterruptError::from(error).into())
    }

    pub(crate) fn field_count(&self) -> u64 {
        if let Some(runtime) = &self.owned {
            return runtime.raster_field();
        }
        self.state.interrupts.field_count()
    }

    pub(crate) fn progressive_skip_allowed(&self) -> bool {
        if self.owned.is_some() {
            return false;
        }
        self.state
            .interrupts
            .progressive_skip_allowed(&self.state.registers)
    }

    pub(crate) fn preflight_with_source(
        &self,
        at: MasterTicks,
        region: Region,
        source: VdpDmaSource<'_>,
    ) -> Result<(), VdpScheduleError> {
        self.require_legacy()?;
        self.preflight_dma_from(at, region, false, Some(source))
    }
    /// Requires a running DMA immediately before its next memory opportunity.
    pub fn resume_dma_from_known_phase(
        &mut self,
        command: ControlCommand,
        fill: Option<u16>,
        region: Region,
    ) -> Result<(), VdpPortError> {
        self.require_legacy_port()?;
        self.invalidate_interval();
        self.state.resume_dma(command, fill, region)
    }

    pub fn bootstrap_mode5() -> Self {
        Self {
            state: State::bootstrap_mode5(),
            memory: VdpMemory::default(),
            interval_hint: None,
            dma_activity_epoch: 0,
            owned: None,
        }
    }

    pub fn from_known_state(
        interrupts: VdpInterrupts,
        registers: VdpRegisterPipeline,
        region: Region,
    ) -> Result<Self, VdpScheduleError> {
        Ok(Self {
            state: State::from_known_state(interrupts, registers, region)?,
            memory: VdpMemory::default(),
            interval_hint: None,
            dma_activity_epoch: 0,
            owned: None,
        })
    }

    /// Requires stable DE and prior preparation on this counter phase until memory mode changes.
    pub fn from_stable_display_state(
        interrupts: VdpInterrupts,
        registers: VdpRegisterPipeline,
        region: Region,
    ) -> Result<Self, VdpScheduleError> {
        let mut state = State::from_known_state(interrupts, registers, region)?;
        state.memory_mode_known = true;
        state.validate_memory_mode(region)?;
        Ok(Self {
            state,
            memory: VdpMemory::default(),
            interval_hint: None,
            dma_activity_epoch: 0,
            owned: None,
        })
    }

    pub(crate) fn dma_activity_epoch(&self) -> u64 {
        self.dma_activity_epoch
    }

    fn commit_dma_activity(&mut self, activity: bool) {
        if activity {
            self.dma_activity_epoch = self.dma_activity_epoch.wrapping_add(1);
        }
    }

    pub fn now(&self) -> MasterTicks {
        if let Some(runtime) = &self.owned {
            return runtime.now();
        }
        self.state.now()
    }
    pub fn registers(&self) -> &VdpRegisterPipeline {
        self.assert_legacy();
        self.state.registers()
    }
    pub fn interrupts(&self) -> &VdpInterrupts {
        self.assert_legacy();
        self.state.interrupts()
    }
    pub fn control(&self) -> &VdpControl {
        self.assert_legacy();
        self.state.control()
    }
    pub fn command(&self) -> ControlCommand {
        self.assert_legacy();
        self.state.command()
    }
    pub fn interrupt_level(&self) -> u8 {
        if let Some(runtime) = &self.owned {
            return runtime.interrupt_level();
        }
        self.state.interrupt_level()
    }

    pub(crate) fn acknowledge_cpu(&mut self) {
        self.assert_legacy();
        self.invalidate_interval();
        self.state
            .interrupts
            .complete_acknowledge(&self.state.registers)
            .expect("validated VDP interrupt image");
    }
    pub fn memory(&self) -> &VdpMemory {
        &self.memory
    }
    pub fn fifo(&self) -> &VdpWriteFifo {
        self.assert_legacy();
        &self.state.fifo
    }

    pub fn sprite_cache(
        &self,
    ) -> Result<&super::super::VdpSpriteCache, super::super::VdpRenderError> {
        if self.owned.is_some() {
            return Err(super::super::VdpRenderError::PendingSpriteWrites);
        }
        if !self.state.fifo.is_empty() || self.state.dma.is_some() {
            return Err(super::super::VdpRenderError::PendingSpriteWrites);
        }
        Ok(&self.state.sprites)
    }

    pub fn read_cache(&self) -> &super::super::VdpReadCache {
        self.assert_legacy();
        &self.state.cache
    }

    pub fn data_read_wait(&self, region: Region) -> Result<Option<MasterTicks>, VdpPortError> {
        self.require_legacy_port()?;
        self.state.data_read_wait(region, &self.memory)
    }

    pub fn read_data(&mut self, region: Region) -> Result<u16, VdpPortError> {
        self.require_legacy_port()?;
        self.invalidate_interval();
        let word = self.state.read_data(region, &self.memory)?;
        self.retire_timed_port_history();
        Ok(word)
    }

    /// VDP-driven status bits; the CPU bus supplies the floating upper bits.
    pub fn read_status(&mut self, region: Region) -> Result<u16, VdpPortError> {
        self.require_legacy_port()?;
        self.invalidate_interval();
        self.state.read_status(region)
    }

    pub(crate) fn read_cpu_status(&mut self, region: Region) -> Result<u16, VdpPortError> {
        self.require_legacy_port()?;
        self.invalidate_interval();
        self.state.read_cpu_status(region)
    }

    pub fn read_hv(&self, region: Region) -> Result<u16, VdpScheduleError> {
        if let Some(runtime) = &self.owned {
            return Ok(runtime.hv_word());
        }
        self.state.read_hv(region)
    }

    pub fn queue_register(
        &mut self,
        index: u8,
        value: u8,
        region: Region,
    ) -> Result<VdpRegisterWrite, VdpScheduleError> {
        self.require_legacy()?;
        self.invalidate_interval();
        self.state.queue_register(index, value, region)
    }

    pub fn write_control(
        &mut self,
        word: u16,
        region: Region,
    ) -> Result<ControlWrite, VdpPortError> {
        self.require_legacy_port()?;
        self.invalidate_interval();
        self.state.write_control(word, region, &self.memory)
    }

    pub fn write_data(&mut self, word: u16, region: Region) -> Result<(), VdpPortError> {
        self.require_legacy_port()?;
        self.invalidate_interval();
        self.state.write_data(word, region, &self.memory)?;
        self.retire_timed_port_history();
        Ok(())
    }

    pub fn data_write_wait(&self, region: Region) -> Result<Option<MasterTicks>, VdpPortError> {
        self.require_legacy_port()?;
        self.state.data_write_wait(region)
    }

    pub fn advance_to(&mut self, at: MasterTicks, region: Region) -> Result<(), VdpScheduleError> {
        self.advance_from(at, region, None)
    }

    pub fn advance_with_source(
        &mut self,
        at: MasterTicks,
        region: Region,
        source: VdpDmaSource<'_>,
    ) -> Result<(), VdpScheduleError> {
        self.advance_from(at, region, Some(source))
    }

    pub(super) fn advance_from(
        &mut self,
        at: MasterTicks,
        region: Region,
        source: Option<VdpDmaSource<'_>>,
    ) -> Result<(), VdpScheduleError> {
        self.require_legacy()?;
        if interval::ENABLED && self.advance_interval(at, region) {
            return Ok(());
        }
        self.interval_hint = None;
        #[cfg(test)]
        interval::record(interval::Metric::Walker);
        self.advance_from_walker(at, region, source)
    }

    #[inline(never)]
    fn advance_from_walker(
        &mut self,
        at: MasterTicks,
        region: Region,
        source: Option<VdpDmaSource<'_>>,
    ) -> Result<(), VdpScheduleError> {
        if self.state.fifo.is_empty()
            && self.state.read_phase.is_none()
            && self
                .state
                .dma
                .is_none_or(|phase| phase == dma::Phase::FillWaiting)
        {
            self.state.advance_to(at, region)?;
            if interval::ENABLED {
                self.seed_interval(region);
            }
            return Ok(());
        }
        let prepared = match self
            .state
            .prepare_to_from(at, region, false, &self.memory, source)
        {
            Err(VdpScheduleError::Dma(super::super::VdpDmaError::BatchFull)) => {
                return self.advance_dma_from(at, region, false, source).map(|_| ());
            }
            result => result?,
        };
        self.publish(prepared);
        Ok(())
    }

    pub fn acknowledge_to(
        &mut self,
        at: MasterTicks,
        region: Region,
    ) -> Result<u8, VdpScheduleError> {
        self.require_legacy()?;
        self.invalidate_interval();
        let prepared = match self.state.prepare_to(at, region, true, &self.memory) {
            Err(VdpScheduleError::Dma(super::super::VdpDmaError::BatchFull)) => {
                return self
                    .advance_dma_to(at, region, true)
                    .map(|level| level.unwrap());
            }
            result => result?,
        };
        let level = prepared.level;
        self.publish(prepared);
        Ok(level.unwrap())
    }

    fn publish(&mut self, prepared: data::Prepared) {
        let data::Prepared {
            state,
            writes,
            dma_activity,
            ..
        } = prepared;
        self.invalidate_interval();
        for transfer in writes.into_iter().flatten() {
            self.memory.publish_retirement(transfer);
        }
        self.state = state;
        self.commit_dma_activity(dma_activity);
    }

    pub(crate) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            state: self.state.clone(),
            dma_activity_epoch: self.dma_activity_epoch,
            memory: Some(self.memory.snapshot()),
            owned: self.owned.clone(),
        }
    }

    pub(crate) fn checkpoint_for_advance(&self) -> Checkpoint {
        let may_write =
            self.owned.is_some() || !self.state.fifo.is_empty() || self.state.dma.is_some();
        Checkpoint {
            state: self.state.clone(),
            dma_activity_epoch: self.dma_activity_epoch,
            memory: may_write.then(|| self.memory.snapshot()),
            owned: self.owned.clone(),
        }
    }

    pub(crate) fn restore(&mut self, checkpoint: Checkpoint) {
        self.interval_hint = None;
        self.dma_activity_epoch = checkpoint.dma_activity_epoch;
        self.owned = checkpoint.owned;
        self.state = checkpoint.state;
        if let Some(memory) = checkpoint.memory {
            self.memory.restore(memory);
        }
    }

    pub(crate) fn power_on(&mut self) {
        self.interval_hint = None;
        if let Some(runtime) = &mut self.owned {
            let region = if runtime.pal() {
                Region::Pal
            } else {
                Region::NtscUs
            };
            **runtime = crate::hardware::vdp::owner::runtime::Runtime::software_origin(region);
        }
        self.state = State::bootstrap_mode5();
        self.memory.clear();
    }
}

impl Vdp {
    #[inline]
    pub fn external_dma_release(
        &self,
        region: Region,
        source: VdpDmaSource<'_>,
    ) -> Result<Option<MasterTicks>, VdpScheduleError> {
        self.require_legacy()?;
        if self.state.dma != Some(dma::Phase::External) {
            return Ok(None);
        }
        self.state.external_release(region, &self.memory, source)
    }

    pub(crate) fn write_control_from(
        &mut self,
        word: u16,
        region: Region,
        source: VdpDmaSource<'_>,
    ) -> Result<crate::hardware::vdp::ControlWrite, VdpPortError> {
        self.require_legacy_port()?;
        self.invalidate_interval();
        let saved = self.state.clone();
        let request = {
            #[cfg(feature = "first-failure-diagnostic")]
            let _scope =
                crate::diagnostic::ControlScope::begin(word, self.state.diagnostic_context());
            self.state.write_control(word, region, &self.memory)
        }?;
        if let Err(error) = self.external_dma_release(region, source) {
            self.state = saved;
            return Err(VdpPortError::Schedule(error));
        }
        Ok(request)
    }
}
