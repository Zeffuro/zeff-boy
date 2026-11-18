use super::*;
use crate::emu_backend::loader::md_tas::MAX_MD_TAS_OWNED_PAYLOAD;
use zeff_md_core::emulator::rollback::{MdRollbackFrame, MdRollbackInputs};

impl MdBackend {
    pub(crate) fn install_authenticated_tas_seed(
        &mut self,
        lease: &MdTasLiveLease,
        seed: MdTasSeed,
    ) -> Result<()> {
        self.validate_tas_live_owner(lease)?;
        let MdMachine::TasLeased(state) = &self.machine else {
            anyhow::bail!("MD TAS seed requires leased candidate")
        };
        seed.validate_for(&state.baseline.load)?;
        state.baseline.load.validate_source(self)?;
        self.verify_tas_live_original(state.session.original(), &state.baseline)?;
        let MdMachine::TasLeased(state) = &mut self.machine else {
            unreachable!()
        };
        seed.install(&mut state.session, &state.baseline.load)?;
        state.seed = Some(seed);
        Ok(())
    }

    pub(crate) fn rebuild_tas_live_candidate(&mut self, lease: &MdTasLiveLease) -> Result<()> {
        self.validate_tas_live_owner(lease)?;
        let MdMachine::TasLeased(state) = &self.machine else {
            anyhow::bail!("MD TAS rebuild requires leased candidate")
        };
        let seed = state
            .seed
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("MD TAS trusted seed is unavailable"))?;
        seed.validate_for(&state.baseline.load)?;
        state.baseline.load.validate_source(self)?;
        self.verify_tas_live_original(state.session.original(), &state.baseline)?;
        let MdMachine::TasLeased(state) = &mut self.machine else {
            unreachable!()
        };
        let Some(seed) = &state.seed else {
            unreachable!()
        };
        seed.install(&mut state.session, &state.baseline.load)
    }

    pub(crate) fn capture_tas_live(&self, lease: &MdTasLiveLease) -> Result<MdTasLiveSnapshot> {
        self.validate_tas_live_owner(lease)?;
        let MdMachine::TasLeased(state) = &self.machine else {
            anyhow::bail!("MD TAS capture requires leased candidate")
        };
        ensure!(state.seed.is_some(), "MD TAS seed is not installed");
        let snapshot = MdTasLiveSnapshot {
            owner: Arc::downgrade(&state.owner),
            core: state.session.capture()?,
            proof: MdTasLiveProof::capture(state.session.core())?,
        };
        let mut retained = MdRetainedAllocations::new();
        snapshot.core.visit_retained_allocations(&mut retained)?;
        ensure!(
            retained.finish()?.requested_payload_bytes()
                + (size_of::<MdTasLiveSnapshot>() - size_of::<MdRollbackSnapshot>()) as u64
                <= MAX_MD_TAS_OWNED_PAYLOAD,
            "MD TAS snapshot exceeds retained payload bound"
        );
        Ok(snapshot)
    }

    pub(crate) fn restore_tas_live(
        &mut self,
        lease: &MdTasLiveLease,
        snapshot: &MdTasLiveSnapshot,
    ) -> Result<()> {
        self.validate_tas_live_owner(lease)?;
        ensure!(
            snapshot.owner.ptr_eq(&lease.owner),
            "foreign or stale MD TAS snapshot"
        );
        ensure!(
            MdTasLiveProof::capture(snapshot.core.core())? == snapshot.proof,
            "MD TAS snapshot proof differs"
        );
        let MdMachine::TasLeased(state) = &mut self.machine else {
            anyhow::bail!("MD TAS restore requires leased candidate")
        };
        ensure!(state.seed.is_some(), "MD TAS seed is not installed");
        state
            .session
            .restore(&snapshot.core, &snapshot.proof.config)
    }

    pub(crate) fn advance_tas_live(
        &mut self,
        lease: &MdTasLiveLease,
        ports: [u16; 2],
    ) -> Result<MdRollbackFrame> {
        self.validate_tas_live_owner(lease)?;
        let MdMachine::TasLeased(state) = &mut self.machine else {
            anyhow::bail!("MD TAS advance requires leased candidate")
        };
        ensure!(state.seed.is_some(), "MD TAS seed is not installed");
        let config = state.session.config().clone();
        let input = MdRollbackInputs::new(ports, &config)?;
        state.session.advance_frame(input, &config)
    }
}

impl MdTasLiveSnapshot {
    pub(crate) fn proof(&self) -> &MdTasLiveProof {
        &self.proof
    }

    pub(crate) fn visit_retained<'a>(&'a self, into: &mut MdRetainedAllocations<'a>) -> Result<()> {
        self.core.visit_retained_allocations(into)
    }
}
