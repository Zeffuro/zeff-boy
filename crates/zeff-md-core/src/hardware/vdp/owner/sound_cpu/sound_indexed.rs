use super::*;
use zeff_z80::IndexedMemoryKind;

impl SoundOwner {
    pub(super) fn validate_indexed(&self) -> Result<()> {
        let cb = self.cpu().indexed_cb_cycle_pending(&self.semantic.state);
        if self.semantic.state.indexed_memory_pending() || cb {
            let (active, preceding, displacement) = if cb {
                let request = self
                    .cpu()
                    .next_indexed_cb_cycle(&self.semantic.state)
                    .map_err(|e| anyhow::anyhow!("invalid indexed CB continuation: {e:?}"))?;
                (
                    request.kind().address(),
                    request.preceding_address(),
                    matches!(
                        request.kind(),
                        zeff_z80::IndexedCbCycleKind::Displacement { .. }
                    ),
                )
            } else {
                let request = self
                    .cpu()
                    .next_indexed_memory_cycle(&self.semantic.state)
                    .map_err(|e| anyhow::anyhow!("invalid indexed continuation: {e:?}"))?;
                (
                    request.kind().address(),
                    request.preceding_address(),
                    matches!(request.kind(), IndexedMemoryKind::Displacement { .. }),
                )
            };
            let address = if self.bus.phase == Phase::Active {
                active
            } else {
                preceding
            };
            let refresh =
                displacement && matches!(self.bus.phase, Phase::Ready | Phase::GrantPending);
            ensure!(
                self.reset.is_none()
                    && self.released
                    && self.jr_tail.is_none()
                    && if cb {
                        self.cpu().last_opcode_pc() < 0x7efd
                    } else {
                        Plan::indexed_origin_local(self.cpu())
                    }
                    && self.word_tail.is_none()
                    && self.djnz_tail.is_none()
                    && self.bus.held_address == address
                    && self.refresh_hold == refresh,
                "invalid indexed ownership/history"
            );
            match self.bus.phase {
                Phase::Active | Phase::Ready | Phase::ReturnLow => {
                    let expected = if cb {
                        Plan::prepare_indexed_cb(self.cpu(), self.semantic.state, self.memory())?
                    } else {
                        Plan::prepare_indexed(self.cpu(), self.semantic.state, self.memory())?
                    };
                    ensure!(
                        self.bus.plan == Some(expected),
                        "indexed plan differs from continuation"
                    );
                }
                Phase::GrantPending | Phase::Granted | Phase::ReturnHigh => {
                    ensure!(
                        self.bus.plan.is_none(),
                        "indexed grant retains a driven plan"
                    );
                }
                Phase::ResetReady => anyhow::bail!("indexed continuation at RESET release"),
            }
        } else if let Some(plan) = self.bus.plan {
            ensure!(plan.indexed.is_none(), "indexed plan lacks continuation");
        }
        Ok(())
    }
}
