mod pending;
use super::*;
use anyhow::{Result, ensure};
use zeff_emu_common::save_state::{StateReader as Reader, StateWriter as Writer};
use zeff_z80::CpuState;

impl SoundSchedule {
    pub(crate) fn write_state(&self, cpu: &Cpu, w: &mut Writer) -> Result<()> {
        self.validate_state(cpu, 0)?;

        cpu.write_continuation(&self.state, w);
        w.write_bool(self.pending.is_some());
        if let Some(pending) = &self.pending {
            pending.write_state(cpu, w)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn write_malformed_lock_on_state(
        &self,
        cpu: &Cpu,
        w: &mut Writer,
        replay_bank: Option<u16>,
        keep_access: bool,
    ) -> Result<()> {
        let mut malformed = self.clone();
        let pending = malformed.pending.as_mut().unwrap();
        if let Some(bank) = replay_bank {
            pending.replay.as_mut().unwrap().bank = bank;
        } else {
            pending.replay = None;
        }
        if !keep_access {
            pending.bank_access = None;
        }
        malformed.write_state(cpu, w)
    }

    pub(crate) fn read_state(&mut self, cpu: &mut Cpu, r: &mut Reader<'_>) -> Result<()> {
        let mut source = { cpu.clone() };
        let mut state = BusState::default();
        source.read_continuation(&mut state, r)?;
        let pending = if r.read_bool()? {
            Some(PendingSound::read_state(&source, r)?)
        } else {
            None
        };
        let schedule = Self {
            pending,
            rmw_checkpoint: None,
            fault: None,
            state,
            epoch: None,
        };
        schedule.validate_state(&source, 0)?;
        *self = schedule;
        *cpu = source;
        Ok(())
    }

    pub(crate) fn validate_cartridge(&self, cartridge: &Cartridge) -> Result<()> {
        self.validate_download(cartridge)?;
        if let Some(pending) = &self.pending {
            pending.validate_svp(cartridge)?;
        }
        if let Some(pending) = &self.pending {
            for address in pending
                .bank_access
                .map(|a| a.address)
                .into_iter()
                .chain(pending.observed_bank_access.map(|a| a.address))
                .chain(pending.replay.and_then(|r| r.bank_identity).map(|(a, _)| a))
            {
                ensure!(
                    !cartridge.bounded_odd_guarded(address)
                        && !cartridge.bounded_switched_guarded(address)
                        && !cartridge.expansion_save_contains(address),
                    "unsupported bounded save sound continuation"
                );
            }
            ensure!(
                !(pending.any_bank_access
                    && cartridge.expansion_save_contains(
                        u32::from(pending.memory.bank) << SOUND_BANK_SHIFT
                    )
                    && pending.bank_access.is_none()
                    && pending.observed_bank_access.is_none()
                    && pending
                        .replay
                        .and_then(|replay| replay.bank_identity)
                        .is_none()),
                "unidentified expansion save SOUND continuation"
            );
        }
        if (cartridge.sf004.is_some()
            || cartridge.extended_ssf.is_some()
            || cartridge.realtec.is_some())
            && let Some(pending) = &self.pending
        {
            let identity = pending.replay.and_then(|r| r.bank_identity);
            ensure!(
                pending.any_bank_access == pending.observed_bank_access.is_some(),
                "missing mapped-ROM observed access"
            );
            if let Some(observed) = pending.observed_bank_access {
                ensure!(
                    observed.direction == Direction::Read
                        && cartridge
                            .rom_source_offset(observed.address)
                            .is_some_and(|offset| cartridge.bytes.get(offset).is_some())
                        && observed.address >> SOUND_BANK_SHIFT == u32::from(pending.memory.bank)
                        && pending.bank_access == Some(observed),
                    "unsupported mapped-ROM pending read"
                );
                ensure!(
                    pending.replay.is_some_and(|r| {
                        r.preparation.bounded
                            && r.accesses == 1
                            && matches!(r.cycles, 3..=5)
                            && u32::from(r.bank) == observed.address >> SOUND_BANK_SHIFT
                    }) && identity == Some((observed.address, Direction::Read)),
                    "missing mapped-ROM read replay"
                );
            } else {
                ensure!(
                    pending.bank_access.is_none() && identity.is_none(),
                    "unexpected mapped-ROM bank identity"
                );
            }
        }
        if cartridge.parallel_adapter.is_some()
            && let Some(pending) = &self.pending
        {
            for address in pending
                .bank_access
                .map(|access| access.address)
                .into_iter()
                .chain(pending.observed_bank_access.map(|access| access.address))
                .chain(
                    pending
                        .replay
                        .and_then(|replay| replay.bank_identity)
                        .map(|(address, _)| address),
                )
            {
                ensure!(
                    !cartridge.parallel_adapter_domain(address),
                    "unsupported parallel adapter sound continuation"
                );
            }
        }
        if let Some(pending) = &self.pending {
            if let Some(board) = &cartridge.registers
                && board.is_banked()
            {
                for (address, direction) in pending
                    .bank_access
                    .map(|a| (a.address, a.direction))
                    .into_iter()
                    .chain(
                        pending
                            .observed_bank_access
                            .map(|a| (a.address, a.direction)),
                    )
                    .chain(pending.replay.and_then(|r| r.bank_identity))
                {
                    ensure!(
                        !board.contains(address),
                        "unsupported bank-only sound control continuation"
                    );
                    if direction == Direction::Read && cartridge.mode_sensitive_read(address) {
                        ensure!(
                            pending.replay.is_some_and(|r| {
                                r.preparation.bounded
                                    && r.accesses == 1
                                    && matches!(r.cycles, 3..=5)
                                    && u32::from(r.bank) == address >> SOUND_BANK_SHIFT
                            }),
                            "invalid bank-only ROM read replay phase"
                        );
                    }
                }
            }
            if cartridge.bounded_split() {
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
                        !cartridge.bounded_split_hole(address)
                            && !(direction == Direction::Write
                                && (0x200000..0x210000).contains(&address)),
                        "unsupported split save sound continuation"
                    );
                }
            }
            if cartridge.linear_rom.is_some() {
                ensure!(
                    !pending.any_bank_access
                        || pending.bank_access.is_some()
                        || pending
                            .replay
                            .is_some_and(|replay| replay.bank_identity.is_some()),
                    "missing bounded ROM bank identity"
                );
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
                        address > CARTRIDGE_UNUSED_END
                            || direction == Direction::Read && cartridge.read(address).is_some(),
                        "unsupported bounded ROM sound access"
                    );
                }
            }
            pending.sf002_writes.validate(
                cartridge,
                pending.any_bank_access,
                pending.bank_access,
            )?;
            pending.register_writes.validate(
                cartridge,
                pending.any_bank_access,
                pending.bank_access,
            )?;
            pending.sf001_writes.validate(
                cartridge,
                pending.any_bank_access,
                pending.bank_access,
            )?;
            let bank_base = u32::from(pending.memory.bank) << SOUND_BANK_SHIFT;
            let observed_fetch = pending.observed_bank_access.filter(|access| {
                access.direction == Direction::Read
                    && !self.state.pending()
                    && pending.instruction.cycles == 4
                    && pending.instruction.pc >= SOUND_BANK_WINDOW
                    && access.address
                        == bank_base | (u32::from(pending.instruction.pc) & SOUND_MAIN_MASK)
            });
            ensure!(
                !(pending.any_bank_access
                    && pending.bank_access.is_none()
                    && pending.replay.is_none()
                    && !((cartridge.ssf2.is_some()
                        || cartridge
                            .registers
                            .as_ref()
                            .is_some_and(|board| board.is_banked()))
                        && pending
                            .observed_bank_access
                            .is_some_and(|access| access.direction == Direction::Write))
                    && cartridge.mode_sensitive_read(
                        observed_fetch.map_or(bank_base, |access| access.address)
                    )),
                "missing ambiguous lock-on read replay"
            );
            if let Some(access) = pending.bank_access
                && access.direction == Direction::Read
                && cartridge.mode_sensitive_read(access.address)
            {
                ensure!(pending.replay.is_some(), "missing lock-on read replay");
            }
            if let Some(replay) = pending.replay
                && let Some((address, Direction::Read)) = replay.bank_identity
                && cartridge.mode_sensitive_read(address)
            {
                ensure!(
                    u32::from(replay.bank) == address >> SOUND_BANK_SHIFT,
                    "lock-on replay bank mismatch"
                );
                ensure!(
                    pending.bank_access.is_some_and(|access| {
                        access.address == address && access.direction == Direction::Read
                    }),
                    "missing lock-on read reservation"
                );
            }
            if let Some(save) = &cartridge.lock_on_save {
                ensure!(
                    pending.any_bank_access == pending.observed_bank_access.is_some(),
                    "missing inserted-save observed access"
                );
                if let Some(observed) = pending.observed_bank_access {
                    ensure!(
                        observed.address >> SOUND_BANK_SHIFT == u32::from(pending.memory.bank)
                            && pending.bank_access.is_none_or(|access| {
                                (access.address, access.direction)
                                    == (observed.address, observed.direction)
                            })
                            && pending.replay.is_none_or(|replay| {
                                replay.bank_identity == Some((observed.address, observed.direction))
                            }),
                        "inserted-save observed access mismatch"
                    );
                    if save.contains(observed.address) {
                        ensure!(
                            observed.direction == Direction::Read
                                && pending.bank_access.is_some()
                                && pending.replay.is_some(),
                            "missing inserted-save observed read replay"
                        );
                    }
                }
                ensure!(
                    !pending.bank_access.is_some_and(|access| {
                        access.direction == Direction::Write && save.contains(access.address)
                    }) && !pending.replay.is_some_and(|replay| {
                        replay.bank_identity.is_some_and(|(address, direction)| {
                            direction == Direction::Write && save.contains(address)
                        })
                    }),
                    "unsupported inserted-save sound write"
                );
            }
            if cartridge.ssf2.is_some()
                || cartridge.download.is_some()
                || cartridge.sf002.is_some()
                || cartridge.bounded_split()
                || cartridge
                    .registers
                    .as_ref()
                    .is_some_and(|board| board.is_banked())
            {
                ensure!(
                    pending.any_bank_access == pending.observed_bank_access.is_some(),
                    "missing banked ROM observed access"
                );
                if let Some(observed) = pending.observed_bank_access {
                    ensure!(
                        observed.address >> SOUND_BANK_SHIFT == u32::from(pending.memory.bank)
                            && pending.bank_access.is_none_or(|access| {
                                (access.address, access.direction)
                                    == (observed.address, observed.direction)
                            })
                            && pending.replay.is_none_or(|replay| {
                                replay.bank_identity == Some((observed.address, observed.direction))
                            }),
                        "banked ROM observed access mismatch"
                    );
                    if cartridge.sf002.is_some()
                        && observed.direction == Direction::Write
                        && (observed.address <= CARTRIDGE_END
                            || crate::cartridge::sf002::Sf002::rejects_time_write(observed.address))
                    {
                        ensure!(
                            pending.bank_access == Some(observed),
                            "missing SF-002 write reservation"
                        );
                        pending.sf002_writes.validate(
                            cartridge,
                            pending.any_bank_access,
                            Some(observed),
                        )?;
                    }
                    if cartridge.sf002.is_some()
                        && observed.direction == Direction::Read
                        && observed.address <= CARTRIDGE_END
                    {
                        ensure!(
                            cartridge.read(observed.address).is_some(),
                            "unsupported SF-002 pending read"
                        );
                    }
                    if observed.direction == Direction::Read
                        && cartridge.mode_sensitive_read(observed.address)
                    {
                        ensure!(
                            pending.bank_access.is_some()
                                && pending.replay.is_some()
                                && cartridge.read(observed.address).is_some(),
                            "missing banked ROM read replay"
                        );
                    }
                    ensure!(
                        observed.direction != Direction::Write
                            || !(0xa130f0..=0xa130ff).contains(&observed.address),
                        "unsupported banked ROM sound control write"
                    );
                }
            }
            if cartridge.sf001.is_some()
                && let Some(access) = pending.bank_access
                && access.direction == Direction::Read
                && access.address <= CARTRIDGE_UNUSED_END
            {
                ensure!(pending.replay.is_some(), "missing SF read replay");
            }
            if let Some(access) = pending.bank_access
                && let Some(registers) = &cartridge.registers
                && registers.contains(access.address)
            {
                match access.direction {
                    Direction::Read => ensure!(
                        registers.read(access.address).is_some() && pending.replay.is_some(),
                        "invalid cartridge register read replay"
                    ),
                    Direction::Write => pending.register_writes.validate_access(access)?,
                }
            }
            if let Some(replay) = pending.replay
                && let Some((address, Direction::Read)) = replay.bank_identity
                && (0x400000..=0x40ffff).contains(&address)
                && cartridge.sf001.is_none()
            {
                ensure!(
                    cartridge
                        .registers
                        .as_ref()
                        .and_then(|registers| registers.read(address))
                        .is_some(),
                    "cartridge register replay board mismatch"
                );
            }
        }
        Ok(())
    }

    pub(crate) fn validate_state(&self, cpu: &Cpu, now: u64) -> Result<()> {
        ensure!(
            self.fault.is_none(),
            "faulted sound schedule is unsupported"
        );
        ensure!(
            self.rmw_checkpoint.is_none(),
            "unfinished sound RMW exclusion"
        );
        validate_cpu(cpu)?;
        if let Some(pending) = &self.pending {
            pending.validate_state(cpu, now)?;
            if let Some(replay) = pending.replay {
                ensure!(
                    replay.preparation.state == self.state,
                    "sound replay source continuation mismatch"
                );
            }
        }
        Ok(())
    }
}

fn validate_cpu(cpu: &Cpu) -> Result<()> {
    ensure!(
        cpu.state() != CpuState::Suspended && cpu.trap().is_none(),
        "unsupported sound CPU state"
    );
    Ok(())
}

#[cfg(test)]
mod tests;
