use super::super::sound_semantics::{RetirementInput, RetirementState, WordPhase};
use super::*;
use zeff_z80::WordAddPhase;

impl SoundOwner {
    pub(super) fn word_plan(&self) -> Result<Plan> {
        Plan::prepare_word(
            &self.semantic.cpu,
            self.semantic.state,
            &self.semantic.memory,
            false,
            self.word_tail.as_ref(),
        )
    }
    pub(super) fn retire_word<C: crate::sound_trace::physical::HalfCapture>(
        &mut self,
        plan: Plan,
        capture: &mut C,
    ) -> Result<(Cpu, BusState)> {
        let (cpu, state, tail) = if C::ENABLED {
            plan.retire_word_capturing(
                RetirementState {
                    cpu: &self.semantic.cpu,
                    state: self.semantic.state,
                },
                &self.semantic.memory,
                self.word_tail.as_ref(),
                RetirementInput {
                    sample: self.bus.sample,
                    waits: self.bus.waits,
                },
                capture,
            )
        } else {
            plan.retire_word(
                &self.semantic.cpu,
                self.semantic.state,
                &self.semantic.memory,
                self.word_tail.as_ref(),
                self.bus.sample,
                self.bus.waits,
            )
        }?;
        self.word_tail = tail;
        Ok((cpu, state))
    }
    pub(super) fn validate_word(&self) -> Result<()> {
        ensure!(
            self.bus.word_pending == self.word_tail.is_some(),
            "ADD16 tail presence differs from bus"
        );
        if let Some(tail) = &self.word_tail {
            tail.validate(&self.semantic.cpu, &self.semantic.state)
                .map_err(|e| anyhow::anyhow!("invalid ADD16 tail: {e:?}"))?;
            let refresh = tail.phase() == WordAddPhase::Internal4
                && matches!(self.bus.phase, Phase::Ready | Phase::GrantPending);
            let elapsed = 4
                + u64::from(tail.m1_waits())
                + if tail.phase() == WordAddPhase::Internal3 {
                    4
                } else {
                    0
                };
            ensure!(
                self.reset.is_none()
                    && self.released
                    && self.jr_tail.is_none()
                    && self.bus.held_address == tail.retained_address()
                    && self.data_hold.is_none()
                    && self.refresh_hold == refresh
                    && self
                        .cpu()
                        .cycles()
                        .checked_add(elapsed)
                        .is_some_and(|cycles| cycles <= self.clock.rises()),
                "invalid ADD16 ownership/history"
            );
            match self.bus.phase {
                Phase::Active | Phase::Ready | Phase::ReturnLow => {
                    ensure!(
                        self.bus.plan == Some(self.word_plan()?),
                        "ADD16 internal plan differs from frozen M1"
                    );
                }
                Phase::GrantPending | Phase::Granted | Phase::ReturnHigh => {
                    ensure!(self.bus.plan.is_none(), "ADD16 grant retains a driven plan");
                }
                Phase::ResetReady => anyhow::bail!("ADD16 tail at RESET release"),
            }
        } else if let Some(plan) = self.bus.plan {
            if plan.word == Some(WordPhase::M1) {
                ensure!(
                    self.jr_tail.is_none()
                        && plan
                            == Plan::prepare_word(
                                &self.semantic.cpu,
                                self.semantic.state,
                                &self.semantic.memory,
                                plan.irq,
                                None
                            )?,
                    "ADD16 M1 binding differs"
                );
            } else {
                ensure!(plan.word.is_none(), "ADD16 internal has no tail");
            }
        }
        Ok(())
    }
}
