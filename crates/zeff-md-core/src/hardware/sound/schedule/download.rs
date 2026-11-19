use super::*;
use crate::cartridge::download;
use anyhow::{Result, ensure};

impl SoundSchedule {
    pub(super) fn validate_download(&self, cartridge: &Cartridge) -> Result<()> {
        if cartridge.download.is_none() {
            return Ok(());
        }
        let Some(pending) = &self.pending else {
            return Ok(());
        };
        for (address, direction) in pending
            .bank_access
            .map(|access| (access.address, access.direction))
            .into_iter()
            .chain(
                pending
                    .observed_bank_access
                    .map(|access| (access.address, access.direction)),
            )
            .chain(pending.replay.and_then(|replay| replay.bank_identity))
        {
            ensure!(
                !download::time(address),
                "unsupported download RAM SOUND control"
            );
            if !download::memory(address) {
                continue;
            }
            if direction == Direction::Write {
                ensure!(
                    pending.bank_access == pending.observed_bank_access
                        && pending.bank_access.is_some_and(|access| {
                            access.address == address && access.direction == Direction::Write
                        })
                        && pending.instruction.pc <= SOUND_RAM_END
                        && pending.replay.is_some_and(|replay| {
                            replay.preparation.bounded
                                && replay.accesses == 1
                                && replay.cycles == 3
                                && replay.preparation.state.next_memory_cycle() == Some(3)
                                && !replay.preparation.state.next_segment_reads_opcode()
                                && replay.bank_identity == Some((address, Direction::Write))
                                && u32::from(replay.bank) == address >> SOUND_BANK_SHIFT
                        })
                        && pending.writes == WorkWrites::default()
                        && pending.register_writes == RegisterWrites::default()
                        && pending.sf001_writes == Sf001Writes::default()
                        && pending.sf002_writes == Sf002Writes::default()
                        && pending.psg_writes == PsgWrites::default()
                        && pending.opn2_writes == Opn2Writes::default(),
                    "invalid download RAM write witness"
                );
                continue;
            }
            ensure!(
                pending
                    .replay
                    .is_some_and(|replay| replay.preparation.bounded
                        && replay.accesses == 1
                        && matches!(replay.cycles, 3..=5)
                        && u32::from(replay.bank) == address >> SOUND_BANK_SHIFT),
                "invalid download RAM read replay"
            );
            ensure!(
                pending.bank_access.is_some_and(
                    |access| access.address == address && access.direction == Direction::Read
                ),
                "missing download RAM read reservation"
            );
        }
        Ok(())
    }
}

impl PendingSound {
    pub(crate) fn validate_download_source(
        &self,
        cpu: &Cpu,
        memory: &SoundMemory,
        cartridge: &Cartridge,
    ) -> Result<()> {
        if cartridge.download.is_none()
            || !self.bank_access.is_some_and(|access| {
                download::memory(access.address) && access.direction == Direction::Write
            })
        {
            return Ok(());
        }
        let replay = self
            .replay
            .ok_or_else(|| anyhow::anyhow!("missing Download write source"))?;
        let access = self.bank_access.unwrap();
        let request = cpu
            .next_physical_cycle(&replay.preparation.state)
            .map_err(|error| anyhow::anyhow!("Download write cycle: {error:?}"))?;
        let address = SOUND_BANK_WINDOW | (access.address & SOUND_MAIN_MASK) as u16;
        ensure!(
            request.kind()
                == (zeff_z80::CycleKind::MemoryWrite {
                    address,
                    value: access.value
                }),
            "Download write source transfer mismatch"
        );
        let mut source = memory.snapshot();
        source.bank = replay.bank;
        let mut verified = Self::prepare_with(
            cpu,
            &source,
            cartridge,
            replay.preparation,
            replay.irq_asserted,
            None,
        )
        .map_err(|error| anyhow::anyhow!("Download write reconstruction: {error:?}"))?;
        let waits = self
            .instruction
            .cycles
            .checked_sub(replay.cycles)
            .ok_or_else(|| anyhow::anyhow!("Download write wait accounting"))?;
        ensure!(
            verified
                .start
                .checked_add(u64::from(waits) * SOUND_CPU_DIVISOR)
                == Some(self.start),
            "Download write reservation start mismatch"
        );
        verified
            .extend_bus_segment(u64::from(waits))
            .map_err(|error| anyhow::anyhow!("Download write waits: {error:?}"))?;
        ensure!(
            self.cpu == verified.cpu
                && self.state == verified.state
                && self.instruction == verified.instruction
                && self.deadline == verified.deadline
                && self.memory.ram == verified.memory.ram
                && self.memory.bank == verified.memory.bank
                && self.memory.irq_asserted == verified.memory.irq_asserted
                && self.bank_access == verified.bank_access
                && self.observed_bank_access == verified.observed_bank_access
                && self.replay == verified.replay,
            "Download write continuation mismatch"
        );
        Ok(())
    }
}
