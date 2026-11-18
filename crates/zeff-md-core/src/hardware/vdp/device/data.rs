use super::*;
use crate::hardware::vdp::VdpDmaSource;
use crate::hardware::vdp::memory::MemoryTransfer;
use crate::hardware::vdp::{VdpInterruptError, VdpTimingError, VdpWriteError};

pub(super) type WriteBatch = [Option<MemoryTransfer>; WRITE_BATCH_CAPACITY];

pub(super) struct Prepared {
    pub state: State,
    pub writes: WriteBatch,
    pub level: Option<u8>,
    pub dma_activity: bool,
}

impl State {
    pub(super) fn validate_transfer_mode(&self, region: Region) -> Result<(), VdpPortError> {
        if self.interrupts.staged_blank() {
            return Err(VdpPortError::Schedule(
                VdpRegisterError::UnsupportedModeChange {
                    index: 1,
                    value: self.registers.registers()[1].unwrap(),
                }
                .into(),
            ));
        }
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
        if self.dma == Some(dma::Phase::FillWaiting)
            && (self.control.pending_word().is_some() || self.registers.pending().is_some())
        {
            return Err(VdpPortError::Dma(super::super::VdpDmaError::PendingInputs));
        }
        self.validate_transfer_mode(region)?;
        let code = self.command.code & !(COMMAND_CD4 | COMMAND_CD5);
        self.registers
            .auto_increment()
            .ok_or(VdpPortError::Data(VdpWriteError::UnknownIncrement))?;
        if self.command.address > COMMAND_ADDRESS_MASK
            || (!matches!(code, VRAM_WRITE_CODE | CRAM_WRITE_CODE | VSRAM_WRITE_CODE)
                && !self.command.is_no_effect_write_target())
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
        self.enqueue_data(word, region, memory)
    }

    pub(super) fn enqueue_data(
        &mut self,
        word: u16,
        region: Region,
        memory: &super::super::VdpMemory,
    ) -> Result<(), VdpPortError> {
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
    ) -> Result<Prepared, VdpScheduleError> {
        self.prepare_to_from(at, region, acknowledge, memory, None)
    }

    pub(super) fn prepare_to_from(
        &self,
        at: MasterTicks,
        region: Region,
        acknowledge: bool,
        memory: &super::super::VdpMemory,
        source: Option<VdpDmaSource<'_>>,
    ) -> Result<Prepared, VdpScheduleError> {
        let prepared = self.prepare_chunk_to_from(at, region, acknowledge, memory, source)?;
        if prepared.state.now() < at {
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
    ) -> Result<Prepared, VdpScheduleError> {
        let mut candidate = self.clone();
        let mut writes = [None; WRITE_BATCH_CAPACITY];
        let mut count = 0;
        let mut dma_activity = false;
        while (!candidate.fifo.is_empty()
            || candidate.read_phase.is_some()
            || candidate
                .dma
                .is_some_and(|phase| phase != dma::Phase::FillWaiting))
            && at > candidate.now()
        {
            let mode_change = candidate.pending_fifo_mode_change(region);
            let published = mode_change.map(|_| candidate.registers.published_state().1);
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
                    Err(VdpScheduleError::Interrupt(VdpInterruptError::Timing(
                        VdpTimingError::UnqualifiedMemoryInterval(next),
                    ))) if next > at => {
                        candidate
                            .interrupts
                            .is_external_slot(at, region, &candidate.registers)?;
                        None
                    }
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            if let Some(write) = mode_change.filter(|write| {
                write.publish_at <= at && consume.is_none_or(|slot| write.publish_at <= slot)
            }) {
                if acknowledge && write.publish_at == at {
                    break;
                }
                if count == writes.len() {
                    return Ok(Prepared {
                        state: candidate,
                        writes,
                        level: None,
                        dma_activity,
                    });
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
                        Some(
                            dma::Phase::Fill(_)
                                | dma::Phase::FillUnknownCram
                                | dma::Phase::Copy(Some(_))
                        )
                    ))
            {
                return Ok(Prepared {
                    state: candidate,
                    writes,
                    level: None,
                    dma_activity,
                });
            }
            candidate.advance_to(slot, region)?;
            if consume == Some(slot) && !candidate.fifo.is_empty() {
                if candidate.dma == Some(dma::Phase::FillWaiting)
                    && candidate.command.code & !(COMMAND_CD4 | COMMAND_CD5) == VRAM_WRITE_CODE
                    && candidate.fifo.len() == 1
                    && candidate
                        .fifo
                        .front()
                        .is_some_and(|entry| entry.command.code & !COMMAND_CD4 != VRAM_WRITE_CODE)
                    && !candidate.zero_color_fill_seed()
                {
                    return Err(VdpScheduleError::Dma(
                        super::super::VdpDmaError::PendingInputs,
                    ));
                }
                let previous_count = candidate.fifo.len();
                let retired = candidate.fifo.take_slot();
                writes[count] = retired.map(|(entry, phase)| MemoryTransfer::Known(entry, phase));
                if let Some((entry, _)) = retired {
                    if candidate.fifo.len() < previous_count
                        && entry.command.code & !COMMAND_CD4 == VRAM_WRITE_CODE
                    {
                        candidate.sprites.observe_completed_write(
                            candidate.registers.registers()[usize::from(SPRITE_TABLE_REGISTER)],
                            entry,
                            candidate.registers.horizontal_mode().unwrap(),
                        );
                    }
                    if candidate.dma == Some(dma::Phase::FillWaiting) && candidate.fifo.is_empty() {
                        candidate.dma = Some(
                            if matches!(
                                candidate.command.code & !(COMMAND_CD4 | COMMAND_CD5),
                                CRAM_WRITE_CODE | VSRAM_WRITE_CODE
                            ) {
                                // Word fills reuse the next free FIFO slot, not the triggering word.
                                match candidate.fifo.next_slot_data() {
                                    Some(data) => dma::Phase::Fill(data),
                                    None if candidate.command.code
                                        & !(COMMAND_CD4 | COMMAND_CD5)
                                        == CRAM_WRITE_CODE =>
                                    {
                                        dma::Phase::FillUnknownCram
                                    }
                                    None => {
                                        return Err(VdpScheduleError::Dma(
                                            super::super::VdpDmaError::MissingSource,
                                        ));
                                    }
                                }
                            } else {
                                dma::Phase::Fill(entry.data)
                            },
                        );
                    }
                }
                count += 1;
            } else if consume == Some(slot) && candidate.dma.is_some() {
                let accepts_source = matches!(
                    candidate.dma,
                    Some(
                        dma::Phase::Copy(None) | dma::Phase::Fill(_) | dma::Phase::FillUnknownCram
                    )
                );
                if let Some(write) = candidate.service_dma(memory, &writes)? {
                    writes[count] = Some(write);
                    count += 1;
                }
                dma_activity |= accepts_source;
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
                dma_activity = true;
            }
        }
        let level = if acknowledge {
            Some(candidate.acknowledge_to(at, region)?)
        } else {
            candidate.advance_to(at, region)?;
            None
        };
        Ok(Prepared {
            state: candidate,
            writes,
            level,
            dma_activity,
        })
    }
}
