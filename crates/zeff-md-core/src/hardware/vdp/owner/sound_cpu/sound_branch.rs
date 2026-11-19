use super::super::sound_semantics::{JrPhase, RetirementInput, RetirementMemory, RetirementState};
use super::*;

impl SoundOwner {
    pub(super) fn prepare_plan(&self, audio: &Audio, tick: u64) -> Result<Plan> {
        if self.djnz_tail.is_some() {
            return self.djnz_plan();
        }
        if self.word_tail.is_some() {
            return self.word_plan();
        }
        if let Some(tail) = &self.jr_tail {
            Plan::prepare_jr(
                &self.semantic.cpu,
                self.semantic.state,
                &self.semantic.memory,
                false,
                Some(tail),
            )
        } else {
            Plan::prepare(
                &self.semantic.cpu,
                self.semantic.state,
                &self.semantic.memory,
                audio,
                tick,
                self.boundary_irq,
            )
        }
    }

    pub(super) fn retire_plan<C: crate::sound_trace::physical::HalfCapture>(
        &mut self,
        plan: Plan,
        audio: &Audio,
        tick: u64,
        capture: &mut C,
    ) -> Result<(Cpu, BusState)> {
        if plan.djnz.is_some() {
            return self.retire_djnz(plan, capture);
        }
        ensure!(self.djnz_tail.is_none(), "non-DJNZ cycle lost frozen M1");
        if plan.word.is_some() {
            return self.retire_word(plan, capture);
        }
        ensure!(self.word_tail.is_none(), "non-ADD16 cycle lost frozen M1");
        if plan.jr.is_some() {
            let (cpu, state, tail) = if C::ENABLED {
                plan.retire_jr_capturing(
                    RetirementState {
                        cpu: &self.semantic.cpu,
                        state: self.semantic.state,
                    },
                    &self.semantic.memory,
                    self.jr_tail.as_ref(),
                    RetirementInput {
                        sample: self.bus.sample,
                        waits: self.bus.waits,
                    },
                    capture,
                )
            } else {
                plan.retire_jr(
                    &self.semantic.cpu,
                    self.semantic.state,
                    &self.semantic.memory,
                    self.jr_tail.as_ref(),
                    self.bus.sample,
                    self.bus.waits,
                )
            }?;
            self.jr_tail = tail;
            Ok((cpu, state))
        } else {
            ensure!(
                self.jr_tail.is_none(),
                "non-JR retirement lost pending tail"
            );
            if C::ENABLED {
                plan.retire_capturing(
                    RetirementState {
                        cpu: &self.semantic.cpu,
                        state: self.semantic.state,
                    },
                    RetirementMemory {
                        memory: &self.semantic.memory,
                        audio,
                        tick,
                    },
                    RetirementInput {
                        sample: self.bus.sample,
                        waits: self.bus.waits,
                    },
                    capture,
                )
            } else {
                plan.retire(
                    &self.semantic.cpu,
                    self.semantic.state,
                    &self.semantic.memory,
                    audio,
                    tick,
                    self.bus.sample,
                    self.bus.waits,
                )
            }
        }
    }

    pub(super) fn validate_jr(&self) -> Result<()> {
        ensure!(
            self.bus.jr_pending == self.jr_tail.is_some(),
            "JR tail presence differs from physical boundary"
        );
        if let Some(tail) = &self.jr_tail {
            tail.validate(&self.semantic.cpu, &self.semantic.state)
                .map_err(|e| anyhow::anyhow!("stale SOUND JR tail: {e:?}"))?;
            ensure!(
                self.reset.is_none()
                    && self.released
                    && self.bus.held_address == tail.address()
                    && !self.refresh_hold
                    && self.data_hold.is_none(),
                "invalid JR tail ownership"
            );
            match self.bus.phase {
                Phase::Active | Phase::Ready | Phase::ReturnLow => {
                    let plan = self
                        .bus
                        .plan
                        .ok_or_else(|| anyhow::anyhow!("JR tail lost internal plan"))?;
                    ensure!(
                        plan == Plan::prepare_jr(
                            &self.semantic.cpu,
                            self.semantic.state,
                            &self.semantic.memory,
                            false,
                            Some(tail)
                        )?,
                        "JR internal plan differs from frozen receipt"
                    );
                }
                Phase::GrantPending | Phase::Granted | Phase::ReturnHigh => {
                    ensure!(self.bus.plan.is_none(), "JR grant retains an active plan");
                }
                Phase::ResetReady => anyhow::bail!("JR tail at reset release"),
            }
        } else if let Some(plan) = self.bus.plan {
            ensure!(
                plan.jr != Some(JrPhase::Internal),
                "JR internal plan lacks frozen read"
            );
        }
        Ok(())
    }
}
