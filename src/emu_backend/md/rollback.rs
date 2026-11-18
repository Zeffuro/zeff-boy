use super::ownership::MdMachine;
use super::{MdBackend, MdBackendRollbackLease, MdBackendRollbackSnapshot};
use anyhow::{Result, ensure};
use std::sync::Arc;
use zeff_md_core::emulator::rollback::{MdRollbackFrame, MdRollbackInputs};

impl MdBackend {
    fn validate_rollback_owner(&self, lease: &MdBackendRollbackLease) -> Result<()> {
        let owned = match &self.machine {
            MdMachine::Leased(owned) => owned,
            MdMachine::Restoring { owner, .. } => {
                ensure!(
                    lease.owner.ptr_eq(&Arc::downgrade(owner)),
                    "stale or foreign MD backend rollback token"
                );
                anyhow::bail!(
                    "MD original is installed but restoration verification is incomplete"
                );
            }
            _ => anyhow::bail!("MD backend does not have an active rollback owner"),
        };
        ensure!(
            lease.owner.ptr_eq(&Arc::downgrade(&owned.owner)),
            "stale or foreign MD backend rollback token"
        );
        Ok(())
    }

    pub(crate) fn rollback_hashes(
        &self,
        lease: &MdBackendRollbackLease,
    ) -> Result<zeff_md_core::emulator::rollback::MdRollbackHashes> {
        self.validate_rollback_owner(lease)?;
        let MdMachine::Leased(owned) = &self.machine else {
            anyhow::bail!("MD rollback owner unavailable");
        };
        owned.session.hashes()
    }

    pub(crate) fn rollback_snapshot_hashes(
        &self,
        lease: &MdBackendRollbackLease,
        snapshot: &MdBackendRollbackSnapshot,
    ) -> Result<zeff_md_core::emulator::rollback::MdRollbackHashes> {
        self.validate_rollback_owner(lease)?;
        ensure!(
            snapshot.owner.ptr_eq(&lease.owner),
            "stale or foreign MD backend snapshot"
        );
        snapshot.core.hashes()
    }

    pub(crate) fn visit_rollback_core_allocations<'a>(
        &'a self,
        into: &mut zeff_md_core::emulator::rollback::MdRetainedAllocations<'a>,
    ) -> Result<()> {
        match &self.machine {
            MdMachine::Leased(owned) => owned.session.visit_retained_allocations(into),
            MdMachine::Normal(core) | MdMachine::Transfer(core) => into.visit_emulator(core),
            MdMachine::Restoring { original, .. } => into.visit_emulator(original),
            #[cfg(not(target_arch = "wasm32"))]
            MdMachine::TasLeased(state) => state.visit_retained(into),
            #[cfg(not(target_arch = "wasm32"))]
            MdMachine::TasCommitting(state) => state.visit_retained(into),
            #[cfg(not(target_arch = "wasm32"))]
            MdMachine::TasRestoring(state) => state.visit_retained(into),
        }
    }

    pub(crate) fn capture_rollback(
        &self,
        lease: &MdBackendRollbackLease,
    ) -> Result<MdBackendRollbackSnapshot> {
        self.validate_rollback_owner(lease)?;
        let MdMachine::Leased(owned) = &self.machine else {
            anyhow::bail!("MD rollback owner unavailable");
        };
        Ok(MdBackendRollbackSnapshot {
            owner: Arc::downgrade(&owned.owner),
            core: owned.session.capture()?,
        })
    }

    pub(crate) fn advance_rollback(
        &mut self,
        lease: &MdBackendRollbackLease,
        input: MdRollbackInputs,
    ) -> Result<MdRollbackFrame> {
        self.validate_rollback_owner(lease)?;
        let MdMachine::Leased(owned) = &mut self.machine else {
            anyhow::bail!("MD rollback owner unavailable");
        };
        let config = owned.session.config().clone();
        owned.session.advance_frame(input, &config)
    }

    pub(crate) fn begin_rollback_frame(
        &mut self,
        lease: &MdBackendRollbackLease,
        input: MdRollbackInputs,
    ) -> Result<()> {
        self.validate_rollback_owner(lease)?;
        let MdMachine::Leased(owned) = &mut self.machine else {
            anyhow::bail!("MD rollback owner unavailable");
        };
        let config = owned.session.config().clone();
        owned.session.begin_frame(input, &config)
    }

    pub(crate) fn resume_rollback_frame(
        &mut self,
        lease: &MdBackendRollbackLease,
        budget: std::num::NonZeroUsize,
    ) -> Result<Option<MdRollbackFrame>> {
        self.validate_rollback_owner(lease)?;
        let MdMachine::Leased(owned) = &mut self.machine else {
            anyhow::bail!("MD rollback owner unavailable");
        };
        let config = owned.session.config().clone();
        owned.session.resume_frame(&config, budget)
    }

    pub(crate) fn restore_rollback(
        &mut self,
        lease: &MdBackendRollbackLease,
        snapshot: &MdBackendRollbackSnapshot,
    ) -> Result<()> {
        self.validate_rollback_owner(lease)?;
        ensure!(
            snapshot.owner.ptr_eq(&lease.owner),
            "stale or foreign MD backend snapshot"
        );
        let MdMachine::Leased(owned) = &mut self.machine else {
            anyhow::bail!("MD rollback owner unavailable");
        };
        let config = owned.session.config().clone();
        owned.session.restore(&snapshot.core, &config)
    }

    pub(crate) fn retire_rollback(&mut self, lease: &MdBackendRollbackLease) -> Result<()> {
        self.validate_rollback_owner(lease)?;
        let MdMachine::Leased(owned) = &mut self.machine else {
            anyhow::bail!("MD rollback owner unavailable");
        };
        owned.session.retire();
        Ok(())
    }

    pub(crate) fn release_rollback_after_session(
        &mut self,
        lease: &MdBackendRollbackLease,
        snapshot: &MdBackendRollbackSnapshot,
        expected: &super::MdSessionRestore<'_>,
        worker: Result<()>,
    ) -> Result<()> {
        self.install_rollback_original(lease)?;
        let snapshot = (|| {
            ensure!(
                snapshot.owner.ptr_eq(&lease.owner)
                    && snapshot.core.field() == expected.initial_field,
                "MD session baseline snapshot differs"
            );
            ensure!(
                snapshot.core.hashes()?.eq(expected.hashes),
                "MD baseline snapshot hashes differ"
            );
            Ok(())
        })();
        self.complete_rollback_restore(Some(expected), worker.and(snapshot))
    }

    pub(crate) fn abort_rollback_start(
        &mut self,
        lease: &MdBackendRollbackLease,
        expected: &super::MdSessionRestore<'_>,
    ) -> Result<()> {
        self.release_rollback_checked(lease, Some(expected))
    }

    fn release_rollback_checked(
        &mut self,
        lease: &MdBackendRollbackLease,
        expected: Option<&super::MdSessionRestore<'_>>,
    ) -> Result<()> {
        self.install_rollback_original(lease)?;
        self.complete_rollback_restore(expected, Ok(()))
    }

    fn install_rollback_original(&mut self, lease: &MdBackendRollbackLease) -> Result<()> {
        self.validate_rollback_owner(lease)?;
        let observation = Box::new(self.core().clone());
        let moved = std::mem::replace(&mut self.machine, MdMachine::Transfer(observation));
        let MdMachine::Leased(owned) = moved else {
            self.machine = moved;
            anyhow::bail!("MD rollback owner unavailable");
        };
        let original = owned.session.release();
        self.machine = MdMachine::Restoring {
            original,
            owner: owned.owner,
            host: owned.host,
        };
        Ok(())
    }

    fn complete_rollback_restore(
        &mut self,
        expected: Option<&super::MdSessionRestore<'_>>,
        worker: Result<()>,
    ) -> Result<()> {
        let MdMachine::Restoring { original, host, .. } = &self.machine else {
            anyhow::bail!("MD restoration unavailable");
        };
        let verification = self.verify_rollback_host(original, host).and_then(|()| {
            expected.map_or(Ok(()), |expected| {
                self.verify_session_original(original, expected)
            })
        });
        if let Err(error) = worker {
            return Err(match verification {
                Ok(()) => error,
                Err(original) => error.context(format!("MD original verification: {original}")),
            });
        }
        verification?;
        let observation = Box::new(self.core().clone());
        let moved = std::mem::replace(&mut self.machine, MdMachine::Transfer(observation));
        let MdMachine::Restoring { original, host, .. } = moved else {
            self.machine = moved;
            anyhow::bail!("MD restoration unavailable");
        };
        self.machine = MdMachine::Normal(original);
        self.host_persistence_enabled = host.publication;
        self.rotate_trace_stream();
        self.replace_sound_unwitnessed(
            "Execution owner installed a machine; earlier SOUND history is unavailable",
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;

impl MdBackendRollbackSnapshot {
    pub(crate) fn frame(&self) -> u64 {
        self.core.field()
    }
    pub(crate) fn visit_retained_allocations<'a>(
        &'a self,
        into: &mut zeff_md_core::emulator::rollback::MdRetainedAllocations<'a>,
    ) -> Result<()> {
        self.core.visit_retained_allocations(into)
    }
}
