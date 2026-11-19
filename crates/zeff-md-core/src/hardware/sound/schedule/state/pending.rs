use super::*;

impl PendingSound {
    pub(in super::super) fn write_state(&self, source: &Cpu, w: &mut Writer) -> Result<()> {
        self.cpu.write_continuation(&self.state, w);
        self.memory.write_state(w)?;
        w.write_u16(self.instruction.pc);
        w.write_u8(self.instruction.opcode);
        w.write_u32(self.instruction.cycles);
        w.write_u64(self.deadline);
        w.write_u64(self.start);
        self.writes.write_state(w);
        self.psg_writes.write_state(w);
        self.opn2_writes.write_state(w);
        w.write_bool(self.bank_access.is_some());
        if let Some(access) = self.bank_access {
            access.write_state(w);
        }
        w.write_bool(self.any_bank_access);
        w.write_bool(self.replay.is_some());
        if let Some(replay) = self.replay {
            replay.write_state(source, w)?;
        }
        {
            self.sf002_writes.write_state(w);
        }
        self.register_writes.write_state(w);
        {
            self.sf001_writes.write_state(w);
        }
        {
            w.write_bool(self.observed_bank_access.is_some());
            if let Some(access) = self.observed_bank_access {
                access.write_state(w);
            }
        }
        Ok(())
    }

    pub(in super::super) fn read_state(source: &Cpu, r: &mut Reader<'_>) -> Result<Self> {
        let mut cpu = source.clone();
        let mut state = BusState::default();
        cpu.read_continuation(&mut state, r)?;
        let memory = SoundMemory::read_state(r)?;
        let instruction = FetchedInstruction {
            pc: r.read_u16()?,
            opcode: r.read_u8()?,
            cycles: r.read_u32()?,
        };
        let deadline = r.read_u64()?;
        let start = r.read_u64()?;
        let writes = WorkWrites::read_state(r)?;
        let psg_writes = PsgWrites::read_state(r)?;
        let opn2_writes = Opn2Writes::read_state(r)?;
        let bank_access = if r.read_bool()? {
            Some(BankAccess::read_state(r)?)
        } else {
            None
        };
        let any_bank_access = r.read_bool()?;
        let replay = if r.read_bool()? {
            Some(ReadReplay::read_state(source, r)?)
        } else {
            None
        };
        let sf002_writes = { Sf002Writes::read_state(r)? };
        let register_writes = { RegisterWrites::read_state(r)? };
        let sf001_writes = { Sf001Writes::read_state(r)? };
        let observed_bank_access = if r.read_bool()? {
            Some(BankAccess::read_state(r)?)
        } else {
            None
        };
        #[cfg(feature = "sound-shadow-observer")]
        let observer = observer::image(&memory.ram.1, None);
        Ok(Self {
            register_writes,
            sf001_writes,
            sf002_writes,
            observed_bank_access,
            cpu,
            state,
            memory,
            instruction,
            deadline,
            start,
            writes,
            psg_writes,
            opn2_writes,
            bank_access,
            any_bank_access,
            replay,
            trace_origin: None,
            svp_history: None,
            #[cfg(feature = "sound-shadow-observer")]
            observer,
        })
    }

    pub(in super::super) fn validate_state(&self, source: &Cpu, now: u64) -> Result<()> {
        validate_cpu(&self.cpu)?;
        self.memory.validate_state()?;
        ensure!(
            self.instruction.cycles != 0
                && source
                    .cycles()
                    .checked_add(u64::from(self.instruction.cycles))
                    == Some(self.cpu.cycles()),
            "invalid pending sound cycle count"
        );
        ensure!(
            self.start < self.deadline
                && self.deadline >= now
                && self.start.is_multiple_of(SOUND_CPU_DIVISOR)
                && self.deadline.is_multiple_of(SOUND_CPU_DIVISOR),
            "invalid pending sound deadline"
        );
        ensure!(
            self.deadline - self.start <= u64::from(self.instruction.cycles) * SOUND_CPU_DIVISOR,
            "invalid pending sound interval"
        );
        ensure!(
            self.bank_access.is_none() || self.any_bank_access,
            "invalid sound bank access summary"
        );
        if let Some(replay) = self.replay {
            ensure!(
                replay.bank < 1 << (SOUND_BANK_INPUT_SHIFT + 1),
                "invalid replay sound bank"
            );
            ensure!(
                replay.cycles != 0
                    && replay.cycles <= self.instruction.cycles
                    && replay.accesses != 0,
                "invalid sound replay accounting"
            );
            let origin = replay.preparation.now / SOUND_CPU_DIVISOR * SOUND_CPU_DIVISOR;
            ensure!(
                origin.checked_add(u64::from(self.instruction.cycles) * SOUND_CPU_DIVISOR)
                    == Some(self.deadline)
                    && origin <= self.start,
                "invalid sound replay deadline"
            );
            ensure!(
                replay.preparation.opn2_status & !0x83 == 0,
                "invalid sound replay status"
            );
            ensure!(
                replay.bank_identity.is_some() == self.any_bank_access,
                "invalid sound replay bank identity"
            );
            if let Some(access) = self.bank_access {
                ensure!(
                    replay.bank_identity == Some((access.address, access.direction)),
                    "invalid sound replay bank access"
                );
            }
        }
        Ok(())
    }
}

impl ReadReplay {
    pub(in super::super) fn write_state(self, source: &Cpu, w: &mut Writer) -> Result<()> {
        w.write_u64(self.preparation.now);
        source.write_continuation(&self.preparation.state, w);
        w.write_bool(self.preparation.bounded);
        w.write_u8(self.preparation.opn2_status);
        w.write_u16(self.bank);
        w.write_bool(self.irq_asserted);
        w.write_u32(self.cycles);
        w.write_u32(self.accesses);
        w.write_bool(self.bank_identity.is_some());
        if let Some((address, direction)) = self.bank_identity {
            w.write_u32(address);
            super::super::super::state::write_direction(direction, w);
        }
        Ok(())
    }

    pub(in super::super) fn read_state(source: &Cpu, r: &mut Reader<'_>) -> Result<Self> {
        let now = r.read_u64()?;
        let mut cpu = source.clone();
        let mut state = BusState::default();
        cpu.read_continuation(&mut state, r)?;
        ensure!(&cpu == source, "sound replay CPU source mismatch");
        let preparation = Preparation {
            now,
            state,
            bounded: r.read_bool()?,
            opn2_status: r.read_u8()?,
        };
        let bank = r.read_u16()?;
        let irq_asserted = r.read_bool()?;
        let cycles = r.read_u32()?;
        let accesses = r.read_u32()?;
        let bank_identity = if r.read_bool()? {
            let address = r.read_u32()?;
            ensure!(address <= BUS_ADDRESS_MASK, "invalid replay bank address");
            Some((address, super::super::super::state::read_direction(r)?))
        } else {
            None
        };
        Ok(Self {
            preparation,
            bank,
            irq_asserted,
            cycles,
            accesses,
            bank_identity,
        })
    }
}
