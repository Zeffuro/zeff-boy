use super::memory::{MdAllocationKind, MdRetainedAllocations};
use super::{Emulator, MdRollbackConfig, MdRollbackHashes, MdRollbackSession, MdRollbackSnapshot};
use anyhow::{Result, ensure};
use std::sync::Arc;

#[must_use]
pub struct MdCandidatePromotion {
    owner: Arc<()>,
    original: Box<Emulator>,
    candidate: Box<Emulator>,
    config: MdRollbackConfig,
}

#[must_use]
pub struct MdPreparedTasAcquisition {
    original_address: usize,
    owner: Arc<()>,
    current: Box<Emulator>,
    config: MdRollbackConfig,
}

impl MdRollbackSession {
    pub fn prepare_tas_acquisition(
        original: &Emulator,
        expected: &MdRollbackConfig,
    ) -> Result<MdPreparedTasAcquisition> {
        ensure!(
            MdRollbackConfig::capture(original)? == *expected,
            "MD TAS acquisition configuration differs"
        );
        expected.boundary_hashes(original)?;
        Ok(MdPreparedTasAcquisition {
            original_address: original as *const Emulator as usize,
            owner: Arc::new(()),
            current: Box::new(original.clone()),
            config: expected.clone(),
        })
    }

    pub fn original(&self) -> &Emulator {
        &self.baseline
    }

    pub fn install_tas_seed(
        &mut self,
        seed: Box<Emulator>,
        expected: &MdRollbackConfig,
        hashes: &MdRollbackHashes,
    ) -> Result<()> {
        self.validate(expected)?;
        ensure!(
            MdRollbackConfig::capture(&seed)? == self.config
                && seed.pacing_field_count() == 0
                && seed.pending_audio_frames() == 0,
            "MD TAS seed is not a fresh matching runtime"
        );
        ensure!(
            self.config.boundary_hashes(&seed)? == *hashes,
            "MD TAS seed runtime hashes differ"
        );
        self.current = seed;
        Ok(())
    }

    pub fn prepare_candidate_promotion(
        self,
        expected: &MdRollbackConfig,
        hashes: &MdRollbackHashes,
    ) -> std::result::Result<MdCandidatePromotion, (Self, anyhow::Error)> {
        let validation = self.validate(expected).and_then(|()| {
            ensure!(
                self.hashes()? == *hashes,
                "MD promotion candidate hashes differ"
            );
            Ok(())
        });
        if let Err(error) = validation {
            return Err((self, error));
        }
        Ok(MdCandidatePromotion {
            owner: self.owner,
            original: self.baseline,
            candidate: self.current,
            config: self.config,
        })
    }
}

impl MdPreparedTasAcquisition {
    // The caller immediately moves this same unmodified allocation after preparation.
    pub fn acquire_original(
        self,
        original: Box<Emulator>,
    ) -> std::result::Result<MdRollbackSession, (Box<Emulator>, anyhow::Error)> {
        if original.as_ref() as *const Emulator as usize != self.original_address {
            return Err((
                original,
                anyhow::anyhow!("MD TAS prepared allocation differs"),
            ));
        }
        Ok(MdRollbackSession {
            owner: self.owner,
            baseline: original,
            current: self.current,
            config: self.config,
            retired: false,
            pending: None,
        })
    }
}

impl MdRollbackSnapshot {
    pub fn core(&self) -> &Emulator {
        &self.runtime
    }
}

impl MdCandidatePromotion {
    pub fn original(&self) -> &Emulator {
        &self.original
    }

    pub fn candidate(&self) -> &Emulator {
        &self.candidate
    }

    pub fn config(&self) -> &MdRollbackConfig {
        &self.config
    }

    pub fn finish_candidate(self) -> Box<Emulator> {
        self.candidate
    }

    pub fn abort_original(self) -> Box<Emulator> {
        self.original
    }

    pub fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        if into.layout(self, MdAllocationKind::SessionLayout)? {
            into.arc(&self.owner, MdAllocationKind::OwnerToken)?;
            into.visit_emulator(&self.original)?;
            into.visit_emulator(&self.candidate)?;
        }
        Ok(())
    }
}
