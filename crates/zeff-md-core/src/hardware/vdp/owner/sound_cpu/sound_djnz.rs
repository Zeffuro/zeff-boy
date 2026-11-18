use super::super::sound_semantics::{DjnzPhase, RetirementInput};
use super::*;
use zeff_z80::DjnzPhase as TailPhase;

impl SoundOwner {
    pub(super) fn djnz_plan(&self) -> Result<Plan> {
        Plan::prepare_djnz(
            &self.semantic.cpu,
            self.semantic.state,
            false,
            self.djnz_tail.as_ref(),
        )
    }
    pub(super) fn retire_djnz<C: crate::sound_trace::physical::HalfCapture>(
        &mut self,
        plan: Plan,
        capture: &mut C,
    ) -> Result<(Cpu, BusState)> {
        let (cpu, state, tail) = if C::ENABLED {
            plan.retire_djnz_capturing(
                &self.semantic.cpu,
                self.semantic.state,
                self.djnz_tail.as_ref(),
                RetirementInput {
                    sample: self.bus.sample,
                    waits: self.bus.waits,
                },
                capture,
            )
        } else {
            plan.retire_djnz(
                &self.semantic.cpu,
                self.semantic.state,
                self.djnz_tail.as_ref(),
                self.bus.sample,
                self.bus.waits,
            )
        }?;
        self.djnz_tail = tail;
        Ok((cpu, state))
    }
    pub(super) fn validate_djnz(&self) -> Result<()> {
        ensure!(
            self.bus.djnz_pending == self.djnz_tail.is_some(),
            "DJNZ tail presence differs from bus"
        );
        if let Some(tail) = &self.djnz_tail {
            tail.validate(&self.semantic.cpu, &self.semantic.state)
                .map_err(|e| anyhow::anyhow!("invalid DJNZ tail: {e:?}"))?;
            let operand_started =
                tail.phase() == TailPhase::Internal || self.bus.phase == Phase::Active;
            let address = if operand_started {
                tail.address()
            } else {
                tail.refresh_address()
            };
            ensure!(
                self.reset.is_none()
                    && self.released
                    && self.jr_tail.is_none()
                    && self.word_tail.is_none()
                    && self.bus.held_address == address
                    && self.data_hold.is_none()
                    && !self.refresh_hold
                    && self
                        .cpu()
                        .cycles()
                        .checked_add(tail.elapsed())
                        .is_some_and(|cycles| cycles <= self.clock.rises()),
                "invalid DJNZ ownership/history"
            );
            match self.bus.phase {
                Phase::Active | Phase::Ready | Phase::ReturnLow => {
                    ensure!(
                        self.bus.plan == Some(self.djnz_plan()?),
                        "DJNZ plan differs from frozen M1"
                    );
                }
                Phase::GrantPending | Phase::Granted | Phase::ReturnHigh => {
                    ensure!(self.bus.plan.is_none(), "DJNZ grant retains a driven plan");
                }
                Phase::ResetReady => anyhow::bail!("DJNZ tail at RESET release"),
            }
        } else if let Some(plan) = self.bus.plan {
            if plan.djnz == Some(DjnzPhase::M1) {
                ensure!(
                    self.jr_tail.is_none()
                        && self.word_tail.is_none()
                        && plan
                            == Plan::prepare_djnz(
                                &self.semantic.cpu,
                                self.semantic.state,
                                plan.irq,
                                None
                            )?,
                    "DJNZ M1 binding differs"
                );
                if self.bus.sample.is_some() {
                    ensure!(self.bus.sample == Some(0x10), "DJNZ frozen opcode differs");
                } else {
                    ensure!(
                        self.semantic.memory.read(plan.address) == Some(0x10),
                        "DJNZ live opcode differs"
                    );
                }
            } else {
                ensure!(plan.djnz.is_none(), "DJNZ continuation has no tail");
            }
        }
        Ok(())
    }
}
