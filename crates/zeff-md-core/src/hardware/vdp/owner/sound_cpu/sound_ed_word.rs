use super::*;
use zeff_z80::EdWordPhase;

impl SoundOwner {
    pub(super) fn validate_ed_word(&self) -> Result<()> {
        if self.semantic.state.ed_word_pending() {
            let request = self
                .cpu()
                .next_ed_word_cycle(&self.semantic.state)
                .map_err(|error| {
                    anyhow::anyhow!("invalid ED arithmetic continuation: {error:?}")
                })?;
            let refresh = request.kind().phase == EdWordPhase::Internal4
                && matches!(self.bus.phase, Phase::Ready | Phase::GrantPending);
            ensure!(
                self.reset.is_none()
                    && self.released
                    && self.jr_tail.is_none()
                    && self.word_tail.is_none()
                    && self.djnz_tail.is_none()
                    && self.bus.held_address == request.kind().retained_address
                    && self.refresh_hold == refresh
                    && self.data_hold.is_none(),
                "invalid ED arithmetic ownership/history"
            );
            match self.bus.phase {
                Phase::Active | Phase::Ready | Phase::ReturnLow => {
                    ensure!(
                        self.bus.plan
                            == Some(Plan::prepare_ed_word(self.cpu(), self.semantic.state)?),
                        "ED arithmetic plan differs from continuation"
                    );
                }
                Phase::GrantPending | Phase::Granted | Phase::ReturnHigh => {
                    ensure!(
                        self.bus.plan.is_none(),
                        "ED arithmetic grant retains a plan"
                    );
                }
                Phase::ResetReady => anyhow::bail!("ED arithmetic at RESET release"),
            }
        } else if let Some(plan) = self.bus.plan {
            ensure!(
                plan.ed_word.is_none(),
                "ED arithmetic plan lacks continuation"
            );
        }
        Ok(())
    }
}
