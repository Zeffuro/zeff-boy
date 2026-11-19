use super::*;
#[cfg(test)]
mod tests;
use super::super::svp_reads::{MAX_ROM_READS, RomRead};
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader, StateWriter};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SoundHistory {
    pub preparation: Preparation,
    pub irq_asserted: bool,
    pub legacy_relative: Option<RelativeReplay>,
    pub source_memory: SoundMemory,
    pub reads: Vec<RomRead>,
    pub reads_settled: bool,
    pub reservation_waits: u64,
}

impl PendingSound {
    pub(super) fn validate_historical_svp_owner(
        &self,
        cartridge: &Cartridge,
        history: &SoundHistory,
    ) -> Result<()> {
        ensure!(
            cartridge.svp.is_some()
                && !history.reads.is_empty()
                && history.reads.len() <= MAX_ROM_READS,
            "SVP sound historical owner/bound"
        );
        ensure!(
            history
                .reads
                .iter()
                .any(|read| read.original != read.effective)
                || self.replay.is_some()
                || history.reads_settled,
            "redundant SVP sound history"
        );
        let mut prior = 0;
        for read in &history.reads {
            ensure!(
                read.ordinal > prior
                    && read.local >= SOUND_BANK_WINDOW
                    && super::super::svp_access_allowed(cartridge, read.offset, Direction::Read)
                    && cartridge.bytes.get(read.offset as usize) == Some(&read.original),
                "SVP sound historical read identity"
            );
            prior = read.ordinal;
        }
        ensure!(
            self.any_bank_access == self.observed_bank_access.is_some(),
            "unidentified historical SVP sound access"
        );
        if let Some(observed) = self.observed_bank_access {
            ensure!(
                super::super::svp_access_allowed(cartridge, observed.address, observed.direction)
                    && observed.address >> SOUND_BANK_SHIFT == u32::from(self.memory.bank),
                "unsupported historical SVP sound continuation"
            );
            if observed.direction == Direction::Read {
                let first = history.reads.first().unwrap();
                ensure!(
                    observed.address == first.offset && observed.value == first.effective,
                    "historical SVP sound first read"
                );
            }
        }
        if let Some(access) = self.bank_access {
            ensure!(
                Some(access) == self.observed_bank_access,
                "SVP sound access identity mismatch"
            );
        }
        if let Some((address, direction)) = self.replay.and_then(|replay| replay.bank_identity) {
            ensure!(
                self.observed_bank_access.is_some_and(
                    |access| (access.address, access.direction) == (address, direction)
                ),
                "SVP sound replay identity mismatch"
            );
        }
        Ok(())
    }

    fn validate_history(&self, source: &Cpu, state: BusState, cartridge: &Cartridge) -> Result<()> {
        let Some(history) = &self.svp_history else {
            return Ok(());
        };
        self.validate_historical_svp_owner(cartridge, history)?;
        ensure!(
            history.preparation.state == state && history.preparation.opn2_status & !0x83 == 0,
            "historical sound preparation source"
        );
        history.source_memory.validate_state()?;
        let mut replayed = None;
        Self::prepare_into_history(
            &mut replayed,
            source,
            &history.source_memory,
            (cartridge, Some(&history.reads)),
            history.preparation,
            PreparationPolicy {
                irq_asserted: history.irq_asserted,
                legacy_relative: history.legacy_relative,
            },
        )
        .map_err(|fault| anyhow::anyhow!("historical sound preparation: {fault:?}"))?;
        let mut replayed = replayed.unwrap();
        ensure!(
            history
                .reads
                .iter()
                .any(|read| read.original != read.effective)
                || replayed.replay.is_some(),
            "all-original SVP sound history has no peripheral replay"
        );
        let waits = self
            .instruction
            .cycles
            .checked_sub(replayed.instruction.cycles)
            .ok_or_else(|| anyhow::anyhow!("historical sound negative waits"))?;
        ensure!(
            history.reservation_waits <= u64::from(waits),
            "historical sound reservation waits"
        );
        replayed
            .apply_reservation_waits(history.reservation_waits)
            .map_err(|fault| anyhow::anyhow!("{fault:?}"))?;
        replayed
            .extend_bus_segment(u64::from(waits) - history.reservation_waits)
            .map_err(|fault| anyhow::anyhow!("{fault:?}"))?;
        if history.reads_settled {
            ensure!(
                replayed.replay.is_some(),
                "historical sound settlement without replay"
            );
            replayed.replay = None;
            replayed.svp_history.as_mut().unwrap().reads_settled = true;
        }
        replayed.trace_origin = self.trace_origin;
        ensure!(
            replayed == *self,
            "historical sound semantic transaction differs"
        );
        Ok(())
    }
}

impl SoundSchedule {
    pub(crate) fn has_svp_history(&self) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|pending| pending.svp_history.is_some())
    }

    pub(crate) fn validate_svp_history(
        &self,
        source: &Cpu,
        cartridge: &Cartridge,
        now: u64,
    ) -> Result<()> {
        if let Some(pending) = &self.pending {
            if let Some(history) = &pending.svp_history {
                ensure!(
                    history.preparation.now <= now,
                    "future SVP sound preparation"
                );
            }
            pending.validate_history(source, self.state, cartridge)?;
        }
        Ok(())
    }

    pub(crate) fn write_svp_history(&self, source: &Cpu) -> Result<Vec<u8>> {
        let mut w = StateWriter::new();
        let Some(history) = self
            .pending
            .as_ref()
            .and_then(|pending| pending.svp_history.as_ref())
        else {
            return Ok(w.into_bytes());
        };
        w.write_bytes(b"ZSNDROM2");
        w.write_u64(history.preparation.now);
        source.write_continuation(&history.preparation.state, &mut w);
        w.write_bool(history.preparation.bounded);
        w.write_u8(history.preparation.opn2_status);
        w.write_bool(history.irq_asserted);
        w.write_u8(match history.legacy_relative {
            None => 0,
            Some(RelativeReplay::Jr) => 1,
            Some(RelativeReplay::Djnz) => 2,
        });
        w.write_bool(history.reads_settled);
        w.write_u64(history.reservation_waits);
        history.source_memory.write_state(&mut w)?;
        w.write_u8(history.reads.len() as u8);
        for read in &history.reads {
            w.write_u32(read.ordinal);
            w.write_u16(read.local);
            w.write_u32(read.offset);
            w.write_u8(read.original);
            w.write_bool(read.original != read.effective);
            if read.original != read.effective {
                w.write_u8(read.effective);
            }
        }
        Ok(w.into_bytes())
    }

    pub(crate) fn read_svp_history(&mut self, source: &Cpu, bytes: &[u8]) -> Result<()> {
        ensure!(bytes.len() <= 32 * 1024, "SVP sound history bound");
        if bytes.is_empty() {
            return Ok(());
        }
        let pending = self
            .pending
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("history without pending sound"))?;
        let mut r = StateReader::new(bytes);
        let mut tag = [0; 8];
        r.read_exact(&mut tag)?;
        ensure!(&tag == b"ZSNDROM2", "SVP sound historical schema");
        let now = r.read_u64()?;
        let mut cpu = source.clone();
        let mut state = BusState::default();
        cpu.read_continuation(&mut state, &mut r)?;
        ensure!(
            cpu == *source && state == self.state,
            "SVP sound source continuation"
        );
        let preparation = Preparation {
            now,
            state,
            bounded: r.read_bool()?,
            opn2_status: r.read_u8()?,
        };
        let irq_asserted = r.read_bool()?;
        let legacy_relative = match r.read_u8()? {
            0 => None,
            1 => Some(RelativeReplay::Jr),
            2 => Some(RelativeReplay::Djnz),
            _ => anyhow::bail!("SVP sound preparation policy"),
        };
        let reads_settled = r.read_bool()?;
        let reservation_waits = r.read_u64()?;
        let source_memory = SoundMemory::read_state(&mut r)?;
        let count = usize::from(r.read_u8()?);
        ensure!((1..=MAX_ROM_READS).contains(&count), "SVP sound read bound");
        let mut reads = Vec::with_capacity(count);
        for _ in 0..count {
            let ordinal = r.read_u32()?;
            let local = r.read_u16()?;
            let offset = r.read_u32()?;
            let original = r.read_u8()?;
            let effective = if r.read_bool()? {
                let value = r.read_u8()?;
                ensure!(value != original, "redundant SVP sound lane");
                value
            } else {
                original
            };
            reads.push(RomRead {
                ordinal,
                local,
                offset,
                original,
                effective,
            });
        }
        ensure!(r.is_exhausted(), "trailing SVP sound history");
        pending.svp_history = Some(Box::new(SoundHistory {
            preparation,
            irq_asserted,
            legacy_relative,
            source_memory,
            reads,
            reads_settled,
            reservation_waits,
        }));
        Ok(())
    }
}
