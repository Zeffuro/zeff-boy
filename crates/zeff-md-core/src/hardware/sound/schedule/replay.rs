use super::*;
#[cfg(test)]
use crate::sound_trace::NoSoundCapture;
use crate::sound_trace::SoundCapture;

impl PendingSound {
    #[cfg(test)]
    pub fn refresh_reads(
        &mut self,
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
        audio: &Audio,
    ) -> Result<(), SoundFault> {
        self.refresh_reads_captured(cpu, memory, cartridge, audio, &mut NoSoundCapture)
    }

    pub(crate) fn refresh_reads_captured<C: SoundCapture>(
        &mut self,
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
        audio: &Audio,
        capture: &mut C,
    ) -> Result<(), SoundFault> {
        let Some(replay) = self.replay else {
            return Ok(());
        };
        self.validate_download_source(cpu, memory, cartridge)
            .map_err(|_| SoundFault::UnsupportedAccess {
                address: self.instruction.pc,
                direction: Direction::Write,
            })?;
        // CPU and RAM remain unpublished; bank and interrupt inputs were sampled at preparation.
        let mut source = self.svp_history.as_ref().map_or_else(
            || memory.snapshot(),
            |history| history.source_memory.snapshot(),
        );
        source.bank = replay.bank;
        source.irq_asserted = replay.irq_asserted;
        let mut refreshed_slot = None;
        let attempt = Self::prepare_into_history_captured::<C::Attempt>(
            &mut refreshed_slot,
            cpu,
            &source,
            (
                cartridge,
                self.svp_history
                    .as_ref()
                    .map(|history| history.reads.as_slice()),
            ),
            Preparation {
                opn2_status: audio.opn2_status_at(self.deadline),
                ..replay.preparation
            },
            PreparationPolicy {
                irq_asserted: replay.irq_asserted,
                legacy_relative: self.legacy_relative_replay(replay),
            },
        )?;
        let mut refreshed = refreshed_slot.unwrap();
        if self.bank_access.is_none()
            && replay.bank_identity.is_some_and(|(address, direction)| {
                direction == Direction::Read && CandidateBus::bank_opn2_port(address).is_some()
            })
        {
            // Retained unreserved status reads keep their original arbitration.
            refreshed.bank_access = None;
        }
        let sf001_fetch = cartridge.sf001.is_some()
            && cpu.state() != zeff_z80::CpuState::Halted
            && replay.preparation.state.next_segment_reads_opcode()
            && replay.bank_identity.is_some_and(|(address, direction)| {
                address <= CARTRIDGE_UNUSED_END && direction == Direction::Read
            });
        let lock_on_fetch = cpu.state() != zeff_z80::CpuState::Halted
            && replay.preparation.state.next_segment_reads_opcode()
            && replay.bank_identity.is_some_and(|(address, direction)| {
                direction == Direction::Read && cartridge.mode_sensitive_read(address)
            });
        let stable = refreshed.replay.is_some_and(|next| {
            next.cycles == replay.cycles
                && next.accesses == replay.accesses
                && next.bank_identity == replay.bank_identity
        }) && refreshed.instruction.pc == self.instruction.pc
            && refreshed.instruction.opcode == self.instruction.opcode
            && refreshed
                .bank_access
                .map(|access| (access.address, access.direction))
                == self
                    .bank_access
                    .map(|access| (access.address, access.direction))
            && (!(sf001_fetch || lock_on_fetch) || refreshed.bank_access == self.bank_access);
        if !stable {
            let cartridge_read = replay.bank_identity.is_some_and(|(address, _)| {
                cartridge.sf001.is_some()
                    || cartridge.mode_sensitive_read(address)
                    || cartridge
                        .save
                        .as_ref()
                        .is_some_and(|save| save.banked_readable() && save.contains(address))
                    || cartridge
                        .registers
                        .as_ref()
                        .is_some_and(|registers| registers.read(address).is_some())
            });
            return Err(if cartridge_read {
                SoundFault::UnstableCartridgeRead {
                    pc: self.instruction.pc,
                }
            } else {
                SoundFault::UnstableOpn2Read {
                    pc: self.instruction.pc,
                }
            });
        }
        let waits = self
            .instruction
            .cycles
            .checked_sub(replay.cycles)
            .ok_or(SoundFault::ClockOverflow)?;
        let reservation_waits = self
            .svp_history
            .as_ref()
            .map_or(0, |history| history.reservation_waits);
        let remaining_waits = u64::from(waits)
            .checked_sub(reservation_waits)
            .ok_or(SoundFault::ClockOverflow)?;
        refreshed.apply_reservation_waits(reservation_waits)?;
        refreshed.extend_bus_segment(remaining_waits)?;
        if self.svp_history.is_some() && refreshed.start != self.start {
            return Err(SoundFault::ClockOverflow);
        }
        if refreshed.deadline != self.deadline {
            return Err(SoundFault::ClockOverflow);
        }
        refreshed.start = self.start;
        refreshed.trace_origin = self.trace_origin;
        if lock_on_fetch && let Some(origin) = &mut refreshed.trace_origin {
            origin.source = crate::hardware::audio::trace::sound_origin(
                origin.boundary_pc as u16,
                replay.bank,
                cartridge,
            )
            .source;
        }
        refreshed.replay = None;
        if let Some(history) = &mut refreshed.svp_history {
            history.reads_settled = true;
        }
        *self = refreshed;
        capture.prepared(attempt);
        Ok(())
    }

    pub(super) fn legacy_relative_replay(&self, replay: ReadReplay) -> Option<RelativeReplay> {
        if !replay.preparation.bounded
            || replay.accesses != 2
            || self.state.pending()
            || replay.preparation.state.transfer_pending()
        {
            return None;
        }
        let prefixed = replay.preparation.state.prefix_pending()
            && matches!(self.instruction.opcode, 0xdd | 0xfd);
        match replay.cycles {
            7 | 12
                if prefixed
                    || matches!(self.instruction.opcode, 0x18 | 0x20 | 0x28 | 0x30 | 0x38) =>
            {
                Some(RelativeReplay::Jr)
            }
            8 | 13 if prefixed || self.instruction.opcode == 0x10 => Some(RelativeReplay::Djnz),
            _ => None,
        }
    }
}
