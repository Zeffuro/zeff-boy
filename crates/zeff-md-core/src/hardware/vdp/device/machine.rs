use super::*;
use crate::hardware::vdp::VdpDmaSource;
use crate::hardware::vdp::memory::MemorySnapshot;
use crate::hardware::vdp::{ControlWrite, VdpMemory, VdpWriteFifo};

#[path = "line.rs"]
mod line;
pub use line::{VdpLineError, VdpScanline};
#[path = "long.rs"]
mod long;
#[path = "state.rs"]
mod state;

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vdp {
    state: State,
    memory: VdpMemory,
}

pub(crate) struct Checkpoint {
    state: State,
    memory: MemorySnapshot,
}

impl Vdp {
    pub(crate) fn field_position(
        &self,
        region: Region,
    ) -> Result<crate::hardware::vdp::FieldPosition, VdpScheduleError> {
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
        self.state
            .interrupts
            .completed_row(at, region, &self.state.registers)
            .map_err(|error| super::super::VdpInterruptError::from(error).into())
    }

    pub(crate) fn field_count(&self) -> u64 {
        self.state.interrupts.field_count()
    }

    pub(crate) fn progressive_skip_allowed(&self) -> bool {
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
        self.preflight_dma_from(at, region, false, Some(source))
    }
    /// Requires a running DMA immediately before its next memory opportunity.
    pub fn resume_dma_from_known_phase(
        &mut self,
        command: ControlCommand,
        fill: Option<u16>,
        region: Region,
    ) -> Result<(), VdpPortError> {
        self.state.resume_dma(command, fill, region)
    }

    pub fn bootstrap_mode5() -> Self {
        Self {
            state: State::bootstrap_mode5(),
            memory: VdpMemory::default(),
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
        })
    }

    pub fn now(&self) -> MasterTicks {
        self.state.now()
    }
    pub fn registers(&self) -> &VdpRegisterPipeline {
        self.state.registers()
    }
    pub fn interrupts(&self) -> &VdpInterrupts {
        self.state.interrupts()
    }
    pub fn control(&self) -> &VdpControl {
        self.state.control()
    }
    pub fn command(&self) -> ControlCommand {
        self.state.command()
    }
    pub fn interrupt_level(&self) -> u8 {
        self.state.interrupt_level()
    }

    pub(crate) fn acknowledge_cpu(&mut self) {
        self.state
            .interrupts
            .complete_acknowledge(&self.state.registers)
            .expect("validated VDP interrupt image");
    }
    pub fn memory(&self) -> &VdpMemory {
        &self.memory
    }
    pub fn fifo(&self) -> &VdpWriteFifo {
        &self.state.fifo
    }

    /// Settled cache only; excludes enqueue and intermediate byte-latch timing.
    pub fn sprite_cache(
        &self,
    ) -> Result<&super::super::VdpSpriteCache, super::super::VdpRenderError> {
        if !self.state.fifo.is_empty() || self.state.dma.is_some() {
            return Err(super::super::VdpRenderError::PendingSpriteWrites);
        }
        Ok(&self.state.sprites)
    }

    pub fn read_cache(&self) -> &super::super::VdpReadCache {
        &self.state.cache
    }

    pub fn data_read_wait(&self, region: Region) -> Result<Option<MasterTicks>, VdpPortError> {
        self.state.data_read_wait(region, &self.memory)
    }

    pub fn read_data(&mut self, region: Region) -> Result<u16, VdpPortError> {
        self.state.read_data(region, &self.memory)
    }

    /// VDP-driven status bits; the CPU bus supplies the floating upper bits.
    pub fn read_status(&mut self, region: Region) -> Result<u16, VdpPortError> {
        self.state.read_status(region)
    }

    pub(crate) fn read_cpu_status(&mut self, region: Region) -> Result<u16, VdpPortError> {
        self.state.read_cpu_status(region)
    }

    pub fn read_hv(&self, region: Region) -> Result<u16, VdpScheduleError> {
        self.state.read_hv(region)
    }

    pub fn queue_register(
        &mut self,
        index: u8,
        value: u8,
        region: Region,
    ) -> Result<VdpRegisterWrite, VdpScheduleError> {
        self.state.queue_register(index, value, region)
    }

    pub fn write_control(
        &mut self,
        word: u16,
        region: Region,
    ) -> Result<ControlWrite, VdpPortError> {
        self.state.write_control(word, region, &self.memory)
    }

    pub fn write_data(&mut self, word: u16, region: Region) -> Result<(), VdpPortError> {
        self.state.write_data(word, region, &self.memory)
    }

    pub fn data_write_wait(&self, region: Region) -> Result<Option<MasterTicks>, VdpPortError> {
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
        if self.state.fifo.is_empty()
            && self.state.read_phase.is_none()
            && self
                .state
                .dma
                .is_none_or(|phase| phase == dma::Phase::FillWaiting)
        {
            return self.state.advance_to(at, region);
        }
        let (state, writes, _) =
            match self
                .state
                .prepare_to_from(at, region, false, &self.memory, source)
            {
                Err(VdpScheduleError::Dma(super::super::VdpDmaError::BatchFull)) => {
                    return self.advance_dma_from(at, region, false, source).map(|_| ());
                }
                result => result?,
            };
        self.publish(state, writes);
        Ok(())
    }

    pub fn acknowledge_to(
        &mut self,
        at: MasterTicks,
        region: Region,
    ) -> Result<u8, VdpScheduleError> {
        let (state, writes, level) = match self.state.prepare_to(at, region, true, &self.memory) {
            Err(VdpScheduleError::Dma(super::super::VdpDmaError::BatchFull)) => {
                return self
                    .advance_dma_to(at, region, true)
                    .map(|level| level.unwrap());
            }
            result => result?,
        };
        self.publish(state, writes);
        Ok(level.unwrap())
    }

    fn publish(&mut self, state: State, writes: data::WriteBatch) {
        for (entry, phase) in writes.into_iter().flatten() {
            let result = if let Some(phase) = phase {
                self.memory
                    .write_vram_byte(entry.command, entry.data, phase)
            } else {
                self.memory.write_word(entry.command, entry.data)
            };
            result.expect("validated FIFO write target");
        }
        self.state = state;
    }

    pub(crate) fn checkpoint(&self) -> Checkpoint {
        Checkpoint {
            state: self.state.clone(),
            memory: self.memory.snapshot(),
        }
    }

    pub(crate) fn restore(&mut self, checkpoint: Checkpoint) {
        self.state = checkpoint.state;
        self.memory.restore(checkpoint.memory);
    }

    pub(crate) fn power_on(&mut self) {
        self.state = State::bootstrap_mode5();
        self.memory.clear();
    }
}

impl Vdp {
    pub fn external_dma_release(
        &self,
        region: Region,
        source: VdpDmaSource<'_>,
    ) -> Result<Option<MasterTicks>, VdpScheduleError> {
        self.state.external_release(region, &self.memory, source)
    }

    pub(crate) fn write_control_from(
        &mut self,
        word: u16,
        region: Region,
        source: VdpDmaSource<'_>,
    ) -> Result<crate::hardware::vdp::ControlWrite, VdpPortError> {
        let saved = self.state.clone();
        let request = self.state.write_control(word, region, &self.memory)?;
        if let Err(error) = self.external_dma_release(region, source) {
            self.state = saved;
            return Err(VdpPortError::Schedule(error));
        }
        Ok(request)
    }
}
