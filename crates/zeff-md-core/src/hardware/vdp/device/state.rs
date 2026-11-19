use super::*;
use crate::hardware::vdp::state::*;
use crate::hardware::vdp::{VdpDmaCopy, VdpDmaRegisters, VdpReadCache, VdpSpriteCache};
use anyhow::{Result, ensure};

#[cfg(test)]
#[path = "state/inhibited_data.rs"]
pub(crate) mod inhibited_data;
#[cfg(test)]
#[path = "state/odd_increment.rs"]
pub(crate) mod odd_increment;
#[cfg(test)]
#[path = "state/tests.rs"]
mod tests;

impl Vdp {
    pub(crate) fn validate_state(&self, region: Region) -> Result<()> {
        if self.owned.is_some() {
            self.validate_owned_state(region)
        } else {
            self.state.validate_state(region)
        }
    }

    pub(crate) fn write_state_in_region(&self, w: &mut StateWriter, region: Region) -> Result<()> {
        self.validate_state(region)?;
        ensure!(
            self.state.pending_status_known(),
            "unknown VDP timed status provenance"
        );
        self.state.registers.write_state(w);
        self.state.interrupts.write_core_state(w);
        write_optional(w, self.state.control.first_word, StateWriter::write_u16);
        write_command(w, self.state.command);
        self.state.fifo.write_state(w);
        self.state.cache.write_state(w);
        write_optional(w, self.state.read_phase, StateWriter::write_bool);
        match self.state.dma {
            None => w.write_u8(0),
            Some(dma::Phase::External) => w.write_u8(1),
            Some(dma::Phase::FillWaiting) => w.write_u8(2),
            Some(dma::Phase::Fill(data)) => {
                w.write_u8(3);
                w.write_u16(data);
            }
            Some(dma::Phase::FillUnknownCram) => w.write_u8(5),
            Some(dma::Phase::Copy(copy)) => {
                w.write_u8(4);
                write_optional(w, copy, |w, copy| copy.write_state(w));
            }
        }
        write_optional(w, self.state.hv_latch, StateWriter::write_u16);
        w.write_bool(self.state.status_known);
        w.write_bool(self.state.memory_mode_known);
        self.state.sprites.write_state(w);
        self.memory.write_state(w);
        self.state.interrupts.write_blank_state(w)?;
        self.write_owned_state(w)?;
        Ok(())
    }

    pub(crate) fn read_state(&mut self, r: &mut StateReader<'_>, region: Region) -> Result<()> {
        let registers = VdpRegisterPipeline::read_state(r)?;
        let interrupts = VdpInterrupts::read_core_state(r, region)?;
        let control = VdpControl {
            first_word: read_optional(r, StateReader::read_u16)?,
        };
        let command = read_command(r)?;
        let fifo = VdpWriteFifo::read_state(r)?;
        let cache = VdpReadCache::read_state(r)?;
        let read_phase = read_optional(r, StateReader::read_bool)?;
        let dma = match r.read_u8()? {
            0 => None,
            1 => Some(dma::Phase::External),
            2 => Some(dma::Phase::FillWaiting),
            3 => Some(dma::Phase::Fill(r.read_u16()?)),
            4 => Some(dma::Phase::Copy(read_optional(r, VdpDmaCopy::read_state)?)),
            5 => Some(dma::Phase::FillUnknownCram),
            _ => anyhow::bail!("invalid VDP DMA phase"),
        };
        let mut state = State {
            registers,
            interrupts,
            control,
            command,
            fifo,
            cache,
            read_phase,
            dma,
            hv_latch: read_optional(r, StateReader::read_u16)?,
            status_known: r.read_bool()?,
            memory_mode_known: r.read_bool()?,
            sprites: VdpSpriteCache::read_state(r)?,
        };
        let memory = VdpMemory::read_state(r)?;
        {
            state.interrupts.read_blank_state(r)?;
        }
        let owned = Self::read_owned_state(r, region)?.map(Box::new);
        let candidate = Self {
            state,
            memory,
            interval_hint: None,
            dma_activity_epoch: self.dma_activity_epoch,
            owned,
        };
        candidate.validate_state(region)?;
        ensure!(
            candidate.state.pending_status_known(),
            "unknown VDP timed status provenance"
        );
        *self = candidate;
        Ok(())
    }
}

impl State {
    fn validate_state(&self, region: Region) -> Result<()> {
        self.registers.validate_state()?;
        self.interrupts.validate_state(&self.registers, region)?;
        validate_command(self.command)?;
        self.fifo.validate_state()?;
        self.cache.validate_state()?;
        if self.interrupts.staged_blank() {
            ensure!(
                self.fifo.is_empty()
                    && !self.cache.is_ready()
                    && self.read_phase.is_none()
                    && self.dma.is_none(),
                "unsupported VDP blank memory history"
            );
        }
        let published = self.registers.registers();
        let mode = published[usize::from(MODE_REGISTER)].unwrap();
        let size = published[usize::from(DISPLAY_SIZE_REGISTER)].unwrap();
        ensure!(
            matches!(size & INTERLACE_MODE_MASK, 0 | 2 | INTERLACE_MODE_MASK),
            "unsupported VDP interlace image"
        );
        if let Some(write) = self.registers.pending() {
            self.validate_clock_write(write, region).map_err(|error| {
                anyhow::anyhow!("invalid pending VDP clock transition: {error:?}")
            })?;
            ensure!(
                write.index != MODE_REGISTER
                    || ((write.value & MODE5_ENABLE != 0
                        || self
                            .interrupts
                            .blank_transition_supported(&self.registers, write.value))
                        && (region == Region::Pal
                            || write.value & V30_ENABLE == 0
                            || self.interrupts.vertical_change_supported(
                                &self.registers,
                                write.value,
                                region
                            ))),
                "invalid pending VDP mode"
            );
            ensure!(
                write.index != DISPLAY_SIZE_REGISTER
                    || (matches!(
                        write.value & INTERLACE_MODE_MASK,
                        0 | 2 | INTERLACE_MODE_MASK
                    ) && crate::hardware::vdp::mode::HorizontalMode::from_register(
                        write.value
                    )
                    .is_some()),
                "invalid pending VDP display size"
            );
        }
        if let Some(word) = self.control.first_word {
            ensure!(
                word & REGISTER_TAG_MASK != REGISTER_TAG,
                "invalid VDP control half command"
            );
            ensure!(
                self.command.address & u32::from(FIRST_ADDRESS_MASK)
                    == u32::from(word & FIRST_ADDRESS_MASK)
                    && self.command.code & FIRST_CODE_MASK == (word >> FIRST_CODE_SHIFT) as u8,
                "inconsistent VDP control half command"
            );
        }
        let latched =
            published[usize::from(HINT_ENABLE_REGISTER)].unwrap() & HV_COUNTER_LATCH_MASK != 0;
        ensure!(
            latched == self.hv_latch.is_some(),
            "inconsistent VDP HV latch"
        );
        ensure!(
            !self.cache.is_ready() || self.read_phase.is_none(),
            "conflicting VDP read phases"
        );
        if self.read_phase.is_some() {
            ensure!(
                self.control.first_word.is_none(),
                "VDP read has a pending control word"
            );
            self.registers
                .auto_increment()
                .ok_or_else(|| anyhow::anyhow!("unknown VDP read increment"))?;
            ensure!(
                matches!(
                    self.command.code,
                    VRAM_READ_CODE | CRAM_READ_CODE | VSRAM_READ_CODE
                ),
                "invalid VDP read command"
            );
            ensure!(
                self.command.code != VSRAM_READ_CODE
                    || ((self.command.address as u16 & COLOR_WORD_ADDRESS_MASK) >> 1)
                        < VSRAM_WORDS as u16,
                "invalid VDP VSRAM read address"
            );
            ensure!(
                self.read_phase != Some(true) || self.command.code == VRAM_READ_CODE,
                "invalid VDP read byte phase"
            );
            ensure!(
                !self
                    .registers
                    .pending()
                    .is_some_and(|write| write.index == AUTO_INCREMENT_REGISTER),
                "pending VDP read increment"
            );
        }
        if !self.fifo.is_empty() || self.read_phase.is_some() || self.dma.is_some() {
            ensure!(mode & VRAM_128K_ENABLE == 0, "unsupported VDP memory mode");
            ensure!(
                !self.registers.pending_memory_mode_change()
                    || self.pending_fifo_mode_change(region).is_some(),
                "pending VDP transfer mode"
            );
        }
        self.validate_dma_state()?;
        Ok(())
    }

    fn validate_dma_state(&self) -> Result<()> {
        let Some(phase) = self.dma else {
            return Ok(());
        };
        if phase == dma::Phase::FillWaiting && self.valid_editable_waiting_inputs() {
            return Ok(());
        }
        ensure!(
            self.read_phase.is_none()
                && !self.cache.is_ready()
                && self.control.first_word.is_none()
                && self.registers.pending().is_none(),
            "VDP DMA has pending inputs"
        );
        let registers = VdpDmaRegisters::from_registers(&self.registers)
            .map_err(|error| anyhow::anyhow!("invalid VDP DMA registers: {error:?}"))?;
        let increment = self
            .registers
            .auto_increment()
            .ok_or_else(|| anyhow::anyhow!("unknown VDP DMA increment"))?;
        ensure!(
            self.command.code & COMMAND_CD5 != 0,
            "inactive VDP DMA command"
        );
        match phase {
            dma::Phase::External => {
                let mut registers = registers;
                let mut command = self.command;
                registers
                    .external_word(&mut command, increment, 0)
                    .map_err(|error| anyhow::anyhow!("invalid external VDP DMA: {error:?}"))?;
            }
            dma::Phase::FillWaiting
            | dma::Phase::Fill(_)
            | dma::Phase::FillUnknownCram
            | dma::Phase::Copy(_) => {
                if phase == dma::Phase::FillUnknownCram {
                    ensure!(
                        self.command.code & !(COMMAND_CD4 | COMMAND_CD5) == CRAM_WRITE_CODE,
                        "unknown VDP fill target"
                    );
                }
                let mode = if matches!(phase, dma::Phase::Copy(_)) {
                    DMA_COPY_MODE
                } else {
                    DMA_FILL_MODE
                };
                registers
                    .next_address(&self.command, increment, mode)
                    .map_err(|error| anyhow::anyhow!("invalid local VDP DMA: {error:?}"))?;
                if matches!(
                    phase,
                    dma::Phase::Copy(_) | dma::Phase::Fill(_) | dma::Phase::FillUnknownCram
                ) {
                    ensure!(
                        self.fifo.is_empty()
                            || (phase == dma::Phase::Fill(0)
                                && self.command.code == 0x21
                                && registers.source_high() & DMA_MODE_MASK == DMA_FILL_MODE
                                && self.fifo.zero_fill_write_pending()),
                        "local VDP DMA conflicts with FIFO"
                    );
                } else {
                    let mut fifo = self.fifo.clone();
                    while let Some((entry, _)) = fifo.take_slot() {
                        ensure!(
                            entry.command.code & !COMMAND_CD4 == VRAM_WRITE_CODE
                                || (self.command.code & !(COMMAND_CD4 | COMMAND_CD5)
                                    == VRAM_WRITE_CODE
                                    && matches!(
                                        entry.command.code & !COMMAND_CD4,
                                        CRAM_WRITE_CODE | VSRAM_WRITE_CODE
                                    ))
                                || (matches!(
                                    self.command.code & !(COMMAND_CD4 | COMMAND_CD5),
                                    CRAM_WRITE_CODE | VSRAM_WRITE_CODE
                                ) && entry.command.code & !COMMAND_CD4
                                    == self.command.code & !(COMMAND_CD4 | COMMAND_CD5)),
                            "invalid VDP fill trigger FIFO"
                        );
                    }
                }
                if let dma::Phase::Copy(Some(copy)) = phase {
                    copy.validate_state(registers, self.command, increment)?;
                }
            }
        }
        Ok(())
    }
}
