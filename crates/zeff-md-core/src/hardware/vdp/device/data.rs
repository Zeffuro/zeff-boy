use super::*;
use crate::hardware::vdp::VdpDmaSource;
use crate::hardware::vdp::{VdpInterruptError, VdpTimingError, VdpWrite, VdpWriteError};

pub(super) type WriteBatch = [Option<(VdpWrite, Option<bool>)>; WRITE_BATCH_CAPACITY];

impl State {
    pub(super) fn validate_transfer_mode(&self, region: Region) -> Result<(), VdpPortError> {
        self.validate_runtime_configuration(region)
            .map_err(VdpPortError::Schedule)?;
        self.interrupts
            .is_machine_write_slot(self.now(), region, &self.registers)
            .map_err(VdpPortError::Schedule)?;
        Ok(())
    }

    pub(super) fn validate_memory_mode(&self, region: Region) -> Result<(), VdpScheduleError> {
        self.validate_memory_configuration(region)?;
        self.interrupts
            .is_write_slot(self.now(), region, &self.registers)?;
        Ok(())
    }

    pub(super) fn validate_memory_configuration(
        &self,
        region: Region,
    ) -> Result<(), VdpScheduleError> {
        self.validate_runtime_configuration(region)?;
        let mode = self.registers.registers()[usize::from(MODE_REGISTER)].unwrap();
        if mode & DISPLAY_ENABLE != 0 && !self.memory_mode_known {
            return Err(VdpInterruptError::Timing(VdpTimingError::UnsupportedMode {
                index: MODE_REGISTER,
                value: mode,
            })
            .into());
        }
        if !self.memory_mode_known {
            return Err(
                VdpInterruptError::Timing(VdpTimingError::UnqualifiedDisplayHistory).into(),
            );
        }
        Ok(())
    }

    pub(super) fn validate_runtime_configuration(
        &self,
        region: Region,
    ) -> Result<(), VdpScheduleError> {
        self.validate_runtime_registers(region, &self.registers)
    }

    fn validate_runtime_registers(
        &self,
        region: Region,
        registers: &VdpRegisterPipeline,
    ) -> Result<(), VdpScheduleError> {
        self.interrupts.timing_next_edge(region, registers)?;
        if registers.pending_memory_mode_change() {
            return Err(VdpInterruptError::Timing(VdpTimingError::PendingModeChange).into());
        }
        let mode = registers.registers()[usize::from(MODE_REGISTER)].unwrap();
        if mode & VRAM_128K_ENABLE != 0 {
            return Err(VdpInterruptError::Timing(VdpTimingError::UnsupportedMode {
                index: MODE_REGISTER,
                value: mode,
            })
            .into());
        }
        Ok(())
    }

    fn validate_data(&self, region: Region) -> Result<(), VdpPortError> {
        self.reject_dma_data()?;
        self.validate_transfer_mode(region)?;
        let code = self.command.code & !(COMMAND_CD4 | COMMAND_CD5);
        self.registers
            .auto_increment()
            .ok_or(VdpPortError::Data(VdpWriteError::UnknownIncrement))?;
        if self.command.address > COMMAND_ADDRESS_MASK
            || (!matches!(code, VRAM_WRITE_CODE | CRAM_WRITE_CODE | VSRAM_WRITE_CODE)
                && !self.command.is_unused_write_target())
        {
            return Err(VdpPortError::Data(VdpWriteError::UnsupportedCommand(
                self.command,
            )));
        }
        Ok(())
    }

    pub(super) fn write_data(
        &mut self,
        word: u16,
        region: Region,
        memory: &super::super::VdpMemory,
    ) -> Result<(), VdpPortError> {
        self.validate_data(region)?;
        self.interrupts
            .next_write_slot(self.now(), region, &self.registers)
            .map_err(VdpPortError::Schedule)?;
        let mut candidate = self.clone();
        let mut command = candidate.command;
        command.code &= !COMMAND_CD5;
        candidate
            .fifo
            .push_data(&mut command, candidate.registers.auto_increment(), word)
            .map_err(VdpPortError::Data)?;
        candidate.command.address = command.address;
        let mut remaining = candidate.fifo.clone();
        let mut end = candidate.now();
        while !remaining.is_empty() {
            end = candidate
                .interrupts
                .next_write_slot(end, region, &candidate.registers)
                .map_err(VdpPortError::Schedule)?;
            remaining.take_slot();
        }
        candidate
            .prepare_to(end, region, false, memory)
            .map_err(VdpPortError::Schedule)?;
        candidate.control.cancel_pending();
        candidate.cache.invalidate();
        candidate.read_phase = None;
        *self = candidate;
        Ok(())
    }

    pub(super) fn data_write_wait(
        &self,
        region: Region,
    ) -> Result<Option<MasterTicks>, VdpPortError> {
        self.validate_data(region)?;
        if !self.fifo.is_full() {
            return Ok(None);
        }
        let mut fifo = self.fifo.clone();
        let mut slot = self.now();
        loop {
            slot = self
                .interrupts
                .next_write_slot(slot, region, &self.registers)
                .map_err(VdpPortError::Schedule)?;
            fifo.take_slot();
            if !fifo.is_full() {
                return Ok(Some(slot));
            }
        }
    }

    pub(super) fn prepare_to(
        &self,
        at: MasterTicks,
        region: Region,
        acknowledge: bool,
        memory: &super::super::VdpMemory,
    ) -> Result<(Self, WriteBatch, Option<u8>), VdpScheduleError> {
        self.prepare_to_from(at, region, acknowledge, memory, None)
    }

    pub(super) fn prepare_to_from(
        &self,
        at: MasterTicks,
        region: Region,
        acknowledge: bool,
        memory: &super::super::VdpMemory,
        source: Option<VdpDmaSource<'_>>,
    ) -> Result<(Self, WriteBatch, Option<u8>), VdpScheduleError> {
        let prepared = self.prepare_chunk_to_from(at, region, acknowledge, memory, source)?;
        if prepared.0.now() < at {
            return Err(VdpScheduleError::Dma(super::super::VdpDmaError::BatchFull));
        }
        Ok(prepared)
    }

    pub(super) fn prepare_chunk_to_from(
        &self,
        at: MasterTicks,
        region: Region,
        acknowledge: bool,
        memory: &super::super::VdpMemory,
        source: Option<VdpDmaSource<'_>>,
    ) -> Result<(Self, WriteBatch, Option<u8>), VdpScheduleError> {
        let mut candidate = self.clone();
        let mut writes = [None; WRITE_BATCH_CAPACITY];
        let mut count = 0;
        while (!candidate.fifo.is_empty()
            || candidate.read_phase.is_some()
            || candidate
                .dma
                .is_some_and(|phase| phase != dma::Phase::FillWaiting))
            && at > candidate.now()
        {
            let display_change = candidate.pending_fifo_display_change();
            let published = display_change.map(|_| candidate.registers.published_state().1);
            let registers = published.as_ref().unwrap_or(&candidate.registers);
            candidate.validate_runtime_registers(region, registers)?;
            let mut consume = if candidate.fifo.is_empty()
                && candidate.read_phase.is_none()
                && matches!(candidate.dma, Some(dma::Phase::External))
            {
                None
            } else {
                match candidate
                    .interrupts
                    .next_write_slot(candidate.now(), region, registers)
                {
                    Ok(slot) => Some(slot),
                    Err(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
                        VdpTimingError::ClockOverflow,
                    ))) => {
                        candidate
                            .interrupts
                            .is_machine_write_slot(at, region, registers)?;
                        None
                    }
                    Err(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
                        VdpTimingError::UnqualifiedMemoryInterval(next),
                    ))) if next > at => {
                        candidate
                            .interrupts
                            .is_machine_write_slot(at, region, registers)?;
                        None
                    }
                    Err(error) => return Err(error),
                }
            };
            let fetch = if candidate.dma == Some(dma::Phase::External) && !candidate.fifo.is_full()
            {
                match candidate.interrupts.next_external_slot(
                    candidate.now(),
                    region,
                    &candidate.registers,
                ) {
                    Ok(slot) => Some(slot),
                    Err(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
                        VdpTimingError::ClockOverflow,
                    ))) => None,
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            if let Some(write) = display_change.filter(|write| {
                write.publish_at <= at && consume.is_none_or(|slot| write.publish_at <= slot)
            }) {
                if acknowledge && write.publish_at == at {
                    break;
                }
                if count == writes.len() {
                    return Ok((candidate, writes, None));
                }
                candidate.advance_to(write.publish_at, region)?;
                if !candidate.interrupts.is_machine_write_slot(
                    write.publish_at,
                    region,
                    &candidate.registers,
                )? {
                    continue;
                }
                consume = Some(write.publish_at);
            }
            let Some(slot) = consume
                .into_iter()
                .chain(fetch)
                .min()
                .filter(|slot| *slot <= at)
            else {
                break;
            };
            if acknowledge && slot == at {
                return Err(VdpScheduleError::CoincidentMemory(slot));
            }
            if consume == Some(slot)
                && count == writes.len()
                && (!candidate.fifo.is_empty()
                    || matches!(
                        candidate.dma,
                        Some(dma::Phase::Fill(_) | dma::Phase::Copy(Some(_)))
                    ))
            {
                return Ok((candidate, writes, None));
            }
            candidate.advance_to(slot, region)?;
            if consume == Some(slot) && !candidate.fifo.is_empty() {
                writes[count] = candidate.fifo.take_slot();
                if let Some((entry, Some(true))) = writes[count] {
                    candidate.sprites.observe_completed_write(
                        candidate.registers.registers()[usize::from(SPRITE_TABLE_REGISTER)],
                        entry,
                        candidate.registers.horizontal_mode().unwrap(),
                    );
                    if candidate.dma == Some(dma::Phase::FillWaiting) && candidate.fifo.is_empty() {
                        candidate.dma = Some(dma::Phase::Fill(entry.data));
                    }
                }
                count += 1;
            } else if consume == Some(slot) && candidate.dma.is_some() {
                if let Some(write) = candidate.service_dma(memory, &writes)? {
                    writes[count] = Some(write);
                    count += 1;
                }
            } else if consume == Some(slot) {
                candidate
                    .service_read(memory, &writes)
                    .map_err(VdpScheduleError::Read)?;
            }
            if candidate.dma == Some(dma::Phase::External)
                && !candidate.fifo.is_full()
                && (fetch == Some(slot)
                    || candidate
                        .interrupts
                        .is_external_slot(slot, region, &candidate.registers)?)
            {
                candidate.fetch_external(source)?;
            }
        }
        let level = if acknowledge {
            Some(candidate.acknowledge_to(at, region)?)
        } else {
            candidate.advance_to(at, region)?;
            None
        };
        Ok((candidate, writes, level))
    }
}
