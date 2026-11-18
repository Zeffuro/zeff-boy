use super::*;
use crate::emu_backend::md::{MdBackendRollbackLease, MdBackendRollbackSnapshot, MdSessionRestore};
use crate::emu_core_trait::EmulatorCore;
use zeff_md_core::emulator::rollback::{MdRollbackConfig, MdRollbackHashes, MdRollbackInputs};

pub(super) struct MdLease {
    owner: MdBackendRollbackLease,
    config: MdRollbackConfig,
    baseline: MdRollbackHashes,
    persistent: [u8; 32],
    initial_field: u64,
}
impl MdLease {
    pub(super) fn begin(md: &mut crate::emu_backend::md::MdBackend) -> Result<Self> {
        let admission = md.netplay_admission()?;
        let raw = md.encode_state_bytes()?;
        let owner = md.begin_rollback(&admission.config)?;
        let lease = Self {
            owner,
            config: admission.config,
            baseline: admission.hashes,
            persistent: admission.persistent,
            initial_field: admission.initial_field,
        };
        let validation = md.rollback_hashes(&lease.owner).and_then(|hashes| {
            ensure!(hashes == lease.baseline, "MD acquired Core runtime differs");
            Ok(())
        });
        if let Err(error) = validation {
            lease
                .abort(md, &raw)
                .context("MD acquisition verification restoration")?;
            return Err(error);
        }
        Ok(lease)
    }
    pub(super) fn profile(
        &self,
        player: Player,
    ) -> crate::netplay::md_input::MdNetplayInputProfile {
        crate::netplay::md_input::MdNetplayInputProfile::from_config(player, &self.config)
    }
    pub(super) fn validate(&self, player: Player, word: u16) -> Result<()> {
        crate::netplay::md_input::validate(&self.config, player, word)
    }
    pub(super) fn validate_frame(
        &self,
        md: &crate::emu_backend::md::MdBackend,
        frame: u64,
    ) -> Result<()> {
        ensure!(
            self.initial_field.checked_add(frame) == Some(md.core().pacing_field_count()),
            "MD rollback core field differs"
        );
        Ok(())
    }
    fn validate_snapshot(&self, state: &MdBackendRollbackSnapshot, frame: u64) -> Result<()> {
        ensure!(
            self.initial_field.checked_add(frame) == Some(state.frame()),
            "MD rollback snapshot field differs"
        );
        Ok(())
    }
    pub(super) fn capture(
        &self,
        md: &crate::emu_backend::md::MdBackend,
    ) -> Result<(MdBackendRollbackSnapshot, u64)> {
        let snapshot = md.capture_rollback(&self.owner)?;
        let frame = snapshot
            .frame()
            .checked_sub(self.initial_field)
            .context("MD snapshot precedes loaded origin")?;
        self.validate_snapshot(&snapshot, frame)?;
        if frame == 0 {
            ensure!(
                md.rollback_snapshot_hashes(&self.owner, &snapshot)? == self.baseline,
                "MD initial snapshot runtime differs"
            );
        }
        Ok((snapshot, frame))
    }
    pub(super) fn advance(
        &self,
        md: &mut crate::emu_backend::md::MdBackend,
        ports: [u16; 2],
    ) -> Result<Vec<f32>> {
        Ok(md
            .advance_rollback(&self.owner, MdRollbackInputs::new(ports, &self.config)?)?
            .audio)
    }
    pub(super) fn begin_frame(
        &self,
        md: &mut crate::emu_backend::md::MdBackend,
        ports: [u16; 2],
    ) -> Result<()> {
        md.begin_rollback_frame(&self.owner, MdRollbackInputs::new(ports, &self.config)?)
    }
    pub(super) fn resume_frame(
        &self,
        md: &mut crate::emu_backend::md::MdBackend,
        budget: std::num::NonZeroUsize,
    ) -> Result<Option<Vec<f32>>> {
        Ok(md
            .resume_rollback_frame(&self.owner, budget)?
            .map(|frame| frame.audio))
    }
    pub(super) fn restore(
        &self,
        md: &mut crate::emu_backend::md::MdBackend,
        state: &MdBackendRollbackSnapshot,
        frame: u64,
    ) -> Result<()> {
        self.validate_snapshot(state, frame)?;
        md.restore_rollback(&self.owner, state)
    }
    pub(super) fn abort(
        &self,
        md: &mut crate::emu_backend::md::MdBackend,
        raw: &[u8],
    ) -> Result<()> {
        md.retire_rollback(&self.owner)?;
        md.abort_rollback_start(
            &self.owner,
            &MdSessionRestore {
                config: &self.config,
                hashes: &self.baseline,
                raw,
                persistent: &self.persistent,
                initial_field: self.initial_field,
            },
        )
    }
    pub(super) fn finish(
        &self,
        md: &mut crate::emu_backend::md::MdBackend,
        snapshot: (&MdBackendRollbackSnapshot, u64),
        raw: &[u8],
        persistent: [u8; 32],
        worker: Result<()>,
    ) -> Result<()> {
        let (state, frame) = snapshot;
        let persistence = (|| {
            self.validate_snapshot(state, frame)?;
            ensure!(frame == 0, "MD session baseline logical frame differs");
            ensure!(
                persistent == self.persistent,
                "MD session persistence witness differs"
            );
            Ok(())
        })();
        md.retire_rollback(&self.owner)?;
        md.release_rollback_after_session(
            &self.owner,
            state,
            &MdSessionRestore {
                config: &self.config,
                hashes: &self.baseline,
                raw,
                persistent: &self.persistent,
                initial_field: self.initial_field,
            },
            worker.and(persistence),
        )
    }
}
