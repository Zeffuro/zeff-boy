use std::mem::size_of;
use std::sync::{Arc, Weak};

use anyhow::{Result, ensure};
use sha2::{Digest, Sha256};
use zeff_emu_common::time::MachineTiming;
use zeff_md_core::Emulator;
use zeff_md_core::emulator::rollback::{
    MdCandidatePromotion, MdRetainedAllocations, MdRollbackConfig, MdRollbackHashes,
    MdRollbackSession, MdRollbackSnapshot,
};

use crate::emu_backend::loader::md_tas::{MdTasLoad, MdTasSeed};
use crate::emu_backend::md::MdBackend;
use crate::emu_backend::md::ownership::{MdHostBaseline, MdMachine};

mod guard;
pub(crate) use guard::MdTasLiveGuard;
mod operations;
mod transitions;

#[derive(Debug)]
pub(crate) struct MdTasLiveLease {
    owner: Weak<()>,
}

#[derive(Debug)]
pub(crate) struct MdTasLiveBeginError {
    pub(crate) error: anyhow::Error,
    pub(crate) fenced_lease: Option<MdTasLiveLease>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MdTasLiveProof {
    config: MdRollbackConfig,
    hashes: MdRollbackHashes,
    raw: [u8; 32],
    field: u64,
    master: u64,
    queued: usize,
}

impl MdTasLiveProof {
    fn capture(core: &Emulator) -> Result<Self> {
        let config = MdRollbackConfig::capture(core)?;
        Ok(Self {
            hashes: config.boundary_hashes(core)?,
            config,
            raw: Sha256::digest(core.encode_state()?).into(),
            field: core.pacing_field_count(),
            master: core.timing_snapshot().now().get(),
            queued: core.pending_audio_frames(),
        })
    }

    pub(crate) fn frame(&self) -> u64 {
        self.field
    }
    pub(crate) fn raw_digest(&self) -> [u8; 32] {
        self.raw
    }
    pub(crate) fn master_tick(&self) -> u64 {
        self.master
    }
    pub(crate) fn queued_audio_frames(&self) -> usize {
        self.queued
    }
}

pub(crate) struct MdTasLiveSnapshot {
    owner: Weak<()>,
    core: MdRollbackSnapshot,
    proof: MdTasLiveProof,
}

struct LiveBaseline {
    host: MdHostBaseline,
    load: MdTasLoad,
    proof: MdTasLiveProof,
}

pub(in crate::emu_backend::md) struct LiveState {
    session: MdRollbackSession,
    owner: Arc<()>,
    baseline: LiveBaseline,
    seed: Option<MdTasSeed>,
}

pub(in crate::emu_backend::md) struct LiveCommit {
    promotion: MdCandidatePromotion,
    owner: Arc<()>,
    baseline: LiveBaseline,
    seed: MdTasSeed,
}

pub(in crate::emu_backend::md) struct LiveRestore {
    original: Box<Emulator>,
    owner: Arc<()>,
    baseline: LiveBaseline,
}

impl LiveState {
    pub(in crate::emu_backend::md) fn core(&self) -> &Emulator {
        self.session.core()
    }
    pub(in crate::emu_backend::md) fn visit_retained<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        self.session.visit_retained_allocations(into)?;
        if let Some(seed) = &self.seed {
            seed.visit_retained(into)?;
        }
        Ok(())
    }
}

impl LiveCommit {
    pub(in crate::emu_backend::md) fn core(&self) -> &Emulator {
        self.promotion.candidate()
    }
    pub(in crate::emu_backend::md) fn visit_retained<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        self.promotion.visit_retained_allocations(into)?;
        self.seed.visit_retained(into)
    }
}

impl LiveRestore {
    pub(in crate::emu_backend::md) fn core(&self) -> &Emulator {
        &self.original
    }
    pub(in crate::emu_backend::md) fn visit_retained<'a>(
        &'a self,
        into: &mut MdRetainedAllocations<'a>,
    ) -> Result<()> {
        into.visit_emulator(&self.original)
    }
}

impl MdBackend {
    fn validate_tas_live_owner(&self, lease: &MdTasLiveLease) -> Result<()> {
        let owner = match &self.machine {
            MdMachine::TasLeased(state) => &state.owner,
            MdMachine::TasCommitting(state) => &state.owner,
            MdMachine::TasRestoring(state) => &state.owner,
            _ => anyhow::bail!("MD backend has no TAS live owner"),
        };
        ensure!(
            lease.owner.ptr_eq(&Arc::downgrade(owner)),
            "stale or foreign MD TAS live lease"
        );
        Ok(())
    }

    fn verify_tas_live_original(&self, original: &Emulator, baseline: &LiveBaseline) -> Result<()> {
        self.verify_rollback_host(original, &baseline.host)?;
        ensure!(
            self.tas_load.as_ref() == Some(&baseline.load),
            "MD frozen TAS provenance changed"
        );
        ensure!(
            MdTasLiveProof::capture(original)? == baseline.proof,
            "MD original TAS runtime, progress or queued PCM changed"
        );
        Ok(())
    }

    pub(in crate::emu_backend::md) fn tas_live_publication(&self) -> Result<bool> {
        match &self.machine {
            MdMachine::TasLeased(state) => Ok(state.baseline.host.publication),
            MdMachine::TasCommitting(state) => Ok(state.baseline.host.publication),
            MdMachine::TasRestoring(state) => Ok(state.baseline.host.publication),
            _ => anyhow::bail!("MD TAS publication owner unavailable"),
        }
    }

    pub(crate) fn tas_live_original(&self, lease: &MdTasLiveLease) -> Result<&Emulator> {
        self.validate_tas_live_owner(lease)?;
        match &self.machine {
            MdMachine::TasLeased(state) => Ok(state.session.original()),
            MdMachine::TasCommitting(state) => Ok(state.promotion.original()),
            MdMachine::TasRestoring(state) => Ok(&state.original),
            _ => unreachable!(),
        }
    }

    pub(crate) fn tas_live_proof(&self, lease: &MdTasLiveLease) -> Result<MdTasLiveProof> {
        self.validate_tas_live_owner(lease)?;
        ensure!(
            !matches!(self.machine, MdMachine::TasRestoring(_)),
            "MD TAS restoration is fenced"
        );
        MdTasLiveProof::capture(self.core())
    }

    pub(crate) fn tas_live_load(&self, lease: &MdTasLiveLease) -> Result<&MdTasLoad> {
        self.validate_tas_live_owner(lease)?;
        let baseline = match &self.machine {
            MdMachine::TasLeased(state) => &state.baseline,
            MdMachine::TasCommitting(state) => &state.baseline,
            _ => anyhow::bail!("MD TAS authenticated candidate view is unavailable"),
        };
        ensure!(
            self.tas_load.as_ref() == Some(&baseline.load),
            "MD TAS frozen load binding changed"
        );
        baseline.load.validate_fixed(self)?;
        Ok(&baseline.load)
    }

    pub(crate) fn tas_live_metadata_bytes(&self, lease: &MdTasLiveLease) -> Result<usize> {
        self.validate_tas_live_owner(lease)?;
        let (baseline, seed, layout) = match &self.machine {
            MdMachine::TasLeased(state) => (
                &state.baseline,
                state.seed.as_ref(),
                size_of::<LiveState>() - size_of::<MdRollbackSession>(),
            ),
            MdMachine::TasCommitting(state) => (
                &state.baseline,
                Some(&state.seed),
                size_of::<LiveCommit>() - size_of::<MdCandidatePromotion>(),
            ),
            MdMachine::TasRestoring(state) => (&state.baseline, None, size_of::<LiveRestore>()),
            _ => unreachable!(),
        };
        Ok(layout
            + baseline.host.state.capacity()
            + baseline.host.frontend.capacity()
            + baseline.load.retained_source_bytes()
            + seed.map_or(0, MdTasSeed::metadata_bytes))
    }
}

#[cfg(test)]
mod tests;
