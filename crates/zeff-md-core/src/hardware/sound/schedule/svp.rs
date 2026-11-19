use super::*;
use anyhow::{Result, ensure};

impl PendingSound {
    pub(super) fn validate_svp(&self, cartridge: &Cartridge) -> Result<()> {
        if cartridge.svp.is_none() {
            ensure!(self.svp_history.is_none(), "history without SVP attachment");
            return Ok(());
        }
        if let Some(history) = &self.svp_history {
            return self.validate_historical_svp_owner(cartridge, history);
        }
        ensure!(
            self.any_bank_access == self.observed_bank_access.is_some(),
            "unidentified SVP sound access"
        );
        if let Some(observed) = self.observed_bank_access {
            ensure!(
                super::super::svp_access_allowed(cartridge, observed.address, observed.direction)
                    && observed.address >> SOUND_BANK_SHIFT == u32::from(self.memory.bank),
                "unsupported SVP sound continuation"
            );
            if observed.direction == Direction::Read {
                ensure!(
                    cartridge.bytes.get(observed.address as usize) == Some(&observed.value),
                    "SVP sound ROM value mismatch"
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
}
