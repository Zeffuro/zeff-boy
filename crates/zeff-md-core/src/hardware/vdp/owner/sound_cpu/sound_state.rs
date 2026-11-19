use super::*;
use crate::hardware::vdp::state::{StateReader, StateWriter};

impl SoundOwner {
    pub(super) fn validate_irq(&self) -> Result<()> {
        let cpu = &self.semantic.cpu;
        let terminal_high = self.bus.phase == Phase::Active
            && self.bus.high
            && !self.bus.waiting
            && self
                .bus
                .plan
                .is_some_and(|p| p.finishes && self.bus.t == p.cycles);
        ensure!(
            !terminal_high || self.boundary_irq == self.irq_low,
            "final-high IRQ differs from its held sample"
        );
        if self.reset.is_some()
            || !self.boundary_irq
            || self.semantic.state.pending()
            || self.word_tail.is_some()
            || self.djnz_tail.is_some()
            || !cpu.interrupts_enabled()
            || cpu.interrupt_enable_pending()
        {
            return Ok(());
        }
        // The final high has sampled INT before this instruction retires.
        if terminal_high {
            return Ok(());
        }
        cpu.next_im1_cycle(&self.semantic.state, true)
            .map_err(|e| anyhow::anyhow!("invalid reserved SOUND interrupt: {e:?}"))?;
        if matches!(
            self.bus.phase,
            Phase::GrantPending | Phase::Granted | Phase::ReturnHigh
        ) {
            ensure!(self.bus.plan.is_none(), "IRQ grant retains a driven plan");
        } else {
            ensure!(
                self.bus
                    .plan
                    .is_some_and(|p| p.shape == Shape::InterruptAck && p.irq),
                "reserved SOUND interrupt lost its acknowledge"
            );
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.cpu().memptr_profile() == zeff_z80::MemptrProfile::Nmos,
            "physical SOUND requires NMOS hidden-address history"
        );
        self.bus.validate()?;
        self.bus.validate_request(self.request_low)?;
        self.validate_jr()?;
        self.validate_word()?;
        self.validate_djnz()?;
        self.validate_indexed()?;
        self.validate_ed_word()?;
        self.validate_irq()?;
        self.copy_history.validate(
            &self.bus,
            &self.semantic.cpu,
            self.semantic.state,
            &self.semantic.memory,
            self.reset.as_ref(),
        )?;
        if let Some(tail) = &self.jr_tail {
            ensure!(
                self.semantic
                    .cpu
                    .cycles()
                    .checked_add(3 + u64::from(tail.read_waits()))
                    .is_some_and(|cycles| cycles <= self.clock.rises()),
                "JR elapsed receipt exceeds observed clock history"
            );
        }
        self.m1_history
            .validate(&self.bus, &self.semantic.cpu, self.semantic.state)?;
        if let Some((address, value)) = self.m1_history.completed_write() {
            ensure!(
                self.semantic.memory.read(address) == Some(value),
                "memory predecessor committed byte differs"
            );
        }
        if let Some(expected) = self.m1_history.boundary_data() {
            ensure!(
                self.data_hold == expected,
                "memory predecessor DATA hold differs"
            );
        }
        if self.reset.is_some() {
            ensure!(self.data_hold.is_none(), "RESET retains a DATA driver");
        } else if self.bus.phase == Phase::Active {
            ensure!(
                self.data_hold == self.bus.pins(false).data,
                "active SOUND DATA hold differs"
            );
        } else if self.data_hold.is_some() {
            ensure!(
                !self.clock.high() && matches!(self.bus.phase, Phase::Ready | Phase::GrantPending),
                "SOUND DATA hold outside completed write aperture"
            );
        }
        ensure!(
            !self.refresh_hold || self.pins().address.is_some(),
            "held RFSH lacks an address driver"
        );
        if let Some(reset) = &self.reset {
            ensure!(
                reset.waiting_repeat() || self.refresh_hold == reset.pins().refresh,
                "RESET RFSH hold differs"
            );
        } else if self.clock.high() {
            ensure!(
                self.refresh_hold == self.bus.pins(false).refresh,
                "high-phase RFSH hold differs"
            );
        }
        ensure!(
            self.bus.high == self.clock.high(),
            "SOUND cycle and observed clock differ"
        );
        ensure!(
            self.clock.high() || self.request_low == self.semantic.memory.requested,
            "stale low SOUND request aperture"
        );
        ensure!(self.semantic.memory.bank < 512, "invalid SOUND bank");
        ensure!(
            self.released || self.reset.is_some() && self.semantic.memory.reset_asserted,
            "invalid SOUND reset origin history"
        );
        ensure!(
            (self.completed[1] != 0) == self.last_external.is_some(),
            "SOUND external receipt count differs"
        );
        ensure!(
            self.completed[0]
                .checked_add(self.completed[1])
                .is_some_and(|n| n <= self.clock.rises()),
            "invalid SOUND completion count"
        );
        if let Some(r) = self.last_external {
            ensure!(
                r.bank < 512
                    && r.half <= self.half()
                    && super::super::sound_semantics::external(r.address),
                "invalid SOUND external receipt"
            );
        }
        ensure!(
            !self.semantic.cpu.is_suspended() && self.semantic.cpu.trap().is_none(),
            "invalid SOUND CPU state"
        );
        ensure!(
            self.semantic.memory.granted
                == (self.pins().bus_ack
                    && self.semantic.memory.requested
                    && !self.semantic.memory.reset_asserted),
            "SOUND semantic grant differs from pins"
        );
        ensure!(
            (!self.semantic.memory.reset_asserted || self.reset.is_some())
                && (self.reset.is_none()
                    || self.bus.phase == Phase::Ready && self.bus.plan.is_none()
                    || self.reset.as_ref().is_some_and(|r| r.waiting_repeat())),
            "reset SOUND has a pending cycle"
        );
        ensure!(
            !self.stable_grant || self.reset.is_none() && self.bus.phase == Phase::Granted,
            "invalid stable SOUND grant history"
        );
        if self.bus.phase == Phase::ResetReady {
            let cpu = &self.semantic.cpu;
            ensure!(
                self.released
                    && self.reset.is_none()
                    && !self.boundary_irq
                    && !self.semantic.state.pending()
                    && cpu.regs().pc == 0
                    && cpu.regs().i == 0
                    && cpu.regs().r == 0
                    && !cpu.is_halted()
                    && !cpu.interrupts_enabled()
                    && !cpu.interrupt_enable_pending()
                    && self.bus.plan.is_some_and(|p| p.shape == Shape::Fetch),
                "invalid post-reset SOUND fetch boundary"
            );
        }
        if let Some(reset) = &self.reset {
            reset.validate(
                &self.clock,
                self.semantic.memory.requested,
                self.semantic.memory.reset_asserted,
            )?;
            ensure!(
                self.released == (!reset.initial() || !self.semantic.memory.reset_asserted),
                "SOUND reset qualification history differs"
            );
            if let Some(plan) = reset.running_plan() {
                plan.validate_binding(
                    &self.semantic.cpu,
                    self.semantic.state,
                    &self.semantic.memory,
                )?;
                if reset.is_repeat_origin() {
                    plan.reset_after_copy(
                        &self.semantic.cpu,
                        self.semantic.state,
                        &self.semantic.memory,
                    )?;
                } else {
                    plan.reset_compatible(
                        &self.semantic.cpu,
                        self.semantic.state,
                        &self.semantic.memory,
                    )?;
                }
            }
        }
        if self.semantic.state.interrupt_pending() {
            self.semantic
                .cpu
                .next_im1_cycle(&self.semantic.state, false)
                .map_err(|e| anyhow::anyhow!("invalid saved IM1 service: {e:?}"))?;
        }
        if let Some(p) = self.bus.plan {
            ensure!(
                p.shape != Shape::InterruptAck || self.boundary_irq && p.irq,
                "IM1 acknowledge lacks accepted IRQ"
            );
            if p.jr != Some(super::super::sound_semantics::JrPhase::Internal)
                && p.word.is_none()
                && p.djnz.is_none()
            {
                p.validate_binding(
                    &self.semantic.cpu,
                    self.semantic.state,
                    &self.semantic.memory,
                )?;
            }
            if p.shape == Shape::Fetch {
                ensure!(
                    p.address == self.semantic.cpu.regs().pc,
                    "SOUND fetch address differs from CPU"
                );
            }
            if p.external
                && p.im1.is_none()
                && p.jr.is_none()
                && p.djnz.is_none()
                && p.copy.is_none()
                && p.indexed.is_none()
            {
                let r = self
                    .semantic
                    .cpu
                    .next_physical_cycle(&self.semantic.state)
                    .map_err(|e| anyhow::anyhow!("invalid pending SOUND cycle: {e:?}"))?;
                ensure!(
                    r.kind().address() == p.address
                        && r.kind().is_write() == (p.shape == Shape::Write),
                    "SOUND external request differs from continuation"
                );
            }
        }
        if self.half() == 0 {
            ensure!(
                *self == Self::software_origin(),
                "SOUND owner origin differs"
            );
        }
        Ok(())
    }

    pub fn write_state(&self, w: &mut StateWriter) -> Result<()> {
        self.validate()?;
        ensure!(self.effects.is_empty(), "unretired SOUND audio effects");
        w.write_u8(10);
        self.clock.write_state(w);
        self.bus.write_state(w)?;
        self.semantic
            .cpu
            .write_continuation(&self.semantic.state, w);
        w.write_bool(self.jr_tail.is_some());
        if let Some(tail) = &self.jr_tail {
            tail.write_state(w)
                .map_err(|e| anyhow::anyhow!("JR tail encode: {e:?}"))?;
        }
        w.write_bool(self.word_tail.is_some());
        if let Some(tail) = &self.word_tail {
            tail.write_state(w)
                .map_err(|e| anyhow::anyhow!("ADD16 tail encode: {e:?}"))?;
        }
        w.write_bool(self.djnz_tail.is_some());
        if let Some(tail) = &self.djnz_tail {
            tail.write_state(w)
                .map_err(|e| anyhow::anyhow!("DJNZ tail encode: {e:?}"))?;
        }
        self.semantic.memory.write_state(w)?;
        w.write_bool(self.released);
        for count in self.completed {
            w.write_u64(count);
        }
        w.write_bool(self.last_external.is_some());
        if let Some(r) = self.last_external {
            w.write_u16(r.address);
            w.write_u16(r.bank);
            w.write_bool(r.write);
            w.write_u8(r.value);
            w.write_u64(r.half);
            w.write_u32(r.waits);
        }
        for value in [
            self.request_low,
            self.wait_high,
            self.irq_low,
            self.boundary_irq,
        ] {
            w.write_bool(value);
        }
        w.write_bool(self.reset.is_some());
        if let Some(reset) = &self.reset {
            reset.write_state(w);
        }
        w.write_bool(self.stable_grant);
        self.m1_history.write_state(w);
        self.copy_history.write_state(w);
        w.write_bool(self.refresh_hold);
        w.write_bool(self.data_hold.is_some());
        if let Some(value) = self.data_hold {
            w.write_u8(value);
        }
        Ok(())
    }
    pub fn read_state(r: &mut StateReader<'_>) -> Result<Self> {
        ensure!(r.read_u8()? == 10, "unsupported physical SOUND owner state");
        let clock = SoundClock::read_state(r)?;
        let bus = SoundBus::read_state(r)?;
        let mut cpu = Cpu::new();
        let mut state = BusState::default();
        cpu.read_continuation(&mut state, r)?;
        let jr_tail = if r.read_bool()? {
            Some(JrTail::read_state(r)?)
        } else {
            None
        };
        let word_tail = if r.read_bool()? {
            Some(WordAddTail::read_state(r)?)
        } else {
            None
        };
        let djnz_tail = if r.read_bool()? {
            Some(DjnzTail::read_state(r)?)
        } else {
            None
        };
        let memory = SoundMemory::read_state(r)?;
        let released = r.read_bool()?;
        let completed = [r.read_u64()?, r.read_u64()?];
        let last_external = if r.read_bool()? {
            Some(ExternalReceipt {
                address: r.read_u16()?,
                bank: r.read_u16()?,
                write: r.read_bool()?,
                value: r.read_u8()?,
                half: r.read_u64()?,
                waits: r.read_u32()?,
            })
        } else {
            None
        };
        let request_low = r.read_bool()?;
        let wait_high = r.read_bool()?;
        let irq_low = r.read_bool()?;
        let boundary_irq = r.read_bool()?;
        let reset = if r.read_bool()? {
            Some(SoundReset::read_state(r)?)
        } else {
            None
        };
        let stable_grant = r.read_bool()?;
        let m1_history = M1History::read_state(r)?;
        let copy_history = CopyHistory::read_state(r)?;
        let refresh_hold = r.read_bool()?;
        let data_hold = if r.read_bool()? {
            Some(r.read_u8()?)
        } else {
            None
        };
        let value = Self {
            clock,
            bus,
            semantic: Arc::new(Semantic { cpu, state, memory }),
            effects: Vec::new(),
            released,
            completed,
            last_external,
            request_low,
            wait_high,
            irq_low,
            boundary_irq,
            reset,
            stable_grant,
            m1_history,
            refresh_hold,
            data_hold,
            jr_tail,
            word_tail,
            djnz_tail,
            copy_history,
        };
        value.validate()?;
        ensure!(
            value.clock.pass() == 1,
            "SOUND save is not at paired board frontier"
        );
        Ok(value)
    }
}
