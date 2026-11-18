use super::super::execution::TasExecutionResult;
use super::*;
use crate::emu_thread::{
    TasControlCommitRejectedReason as Commit, TasControlRollbackRejectedReason as Rollback,
};

impl EmuLoop {
    pub(in crate::emu_thread::emu_loop::tas_control) fn rollback_md_tas(
        &mut self,
        lease_id: u64,
    ) -> EmuResponse {
        let Some(owned) = self.md_tas.take() else {
            unreachable!()
        };
        let restoration = owned.baseline.verify(self);
        let mut cache = std::mem::replace(
            &mut self.tas_control.state_cache,
            WorkerTasStateCache::new(),
        );
        let backend = &mut self.backend;
        let shared = &self.shared_framebuffer;
        let response = self.tas_control.rollback(lease_id, |checkpoint| {
            let EmuBackend::Md(md) = backend else {
                unreachable!()
            };
            md.prepare_tas_live_original_return(&owned.lease)
                .map_err(|_| Rollback::RestoreFailed)?;
            let original = md
                .tas_live_original(&owned.lease)
                .map_err(|_| Rollback::StateVerificationUnavailable)?;
            let state = original
                .encode_state()
                .map_err(|_| Rollback::StateVerificationUnavailable)?;
            let restored = TasRestoredCheckpoint {
                state_sha256: TasDigest::from_bytes(&state),
                frame_count: original.pacing_field_count(),
            };
            verify_checkpoint(checkpoint, restored)?;
            md.complete_tas_live_original(&owned.lease, restoration)
                .map_err(|_| Rollback::RestoreFailed)?;
            cache.clear_md();
            crate::emu_thread::types::publish_backend_framebuffer(shared, backend);
            Ok(restored)
        });
        self.tas_control.state_cache = cache;
        if !matches!(response, EmuResponse::TasControlRolledBack { .. }) {
            self.md_tas = Some(owned);
        }
        response
    }

    pub(in crate::emu_thread::emu_loop::tas_control) fn commit_md_tas(
        &mut self,
        lease_id: u64,
    ) -> EmuResponse {
        let Some(mut owned) = self.md_tas.take() else {
            unreachable!()
        };
        let host = owned.baseline.verify(self);
        let mut cache = std::mem::replace(
            &mut self.tas_control.state_cache,
            WorkerTasStateCache::new(),
        );
        let fresh_history = zeff_emu_common::rewind::RewindBuffer::new_with_frame_duration(
            self.rewind_seconds,
            crate::emu_thread::REWIND_CAPTURE_INTERVAL_FRAMES,
            owned.baseline.duration,
        );
        let backend = &mut self.backend;
        let shared = &self.shared_framebuffer;
        let rewind = &mut self.rewind_buffer;
        let response = self.tas_control.commit(lease_id, |candidate| {
            let EmuBackend::Md(md) = backend else {
                unreachable!()
            };
            if owned.failed {
                return Err(Commit::StateVerificationUnavailable);
            }
            validate_candidate(md, &owned, candidate)
                .map_err(|_| Commit::StateVerificationUnavailable)?;
            let expected = owned
                .proof
                .as_ref()
                .ok_or(Commit::StateVerificationUnavailable)?;
            md.prepare_tas_live_promotion(&owned.lease, expected)
                .map_err(|_| Commit::StateVerificationUnavailable)?;
            let old_history = std::mem::replace(rewind, fresh_history);
            if md
                .finish_tas_live_promotion(&owned.lease, expected, host)
                .is_err()
            {
                *rewind = old_history;
                owned.failed = true;
                return Err(Commit::StateVerificationUnavailable);
            }
            cache.clear_md();
            crate::emu_thread::types::publish_backend_framebuffer(shared, backend);
            Ok(())
        });
        self.tas_control.state_cache = cache;
        if !matches!(response, EmuResponse::TasControlCommitted { .. }) {
            self.md_tas = Some(owned);
        }
        response
    }
}

fn verify_checkpoint(
    checkpoint: &TasControlCheckpoint,
    restored: TasRestoredCheckpoint,
) -> Result<(), Rollback> {
    if restored.state_sha256 != checkpoint.state_sha256 {
        return Err(Rollback::StateDigestMismatch);
    }
    if restored.frame_count != checkpoint.frame_count {
        return Err(Rollback::FrameCountMismatch);
    }
    Ok(())
}

pub(super) fn validate_candidate(
    md: &crate::emu_backend::md::MdBackend,
    owned: &MdLiveControl,
    candidate: TasExecutionResult,
) -> Result<()> {
    let load = md.tas_live_load(&owned.lease)?;
    ensure!(
        md_product::md_pad_profile(candidate.profile) == Some(load.profile()),
        "MD live profile differs"
    );
    load.validate_source(md)?;
    let proof = md.tas_live_proof(&owned.lease)?;
    ensure!(
        owned.proof.as_ref() == Some(&proof)
            && TasDigest(proof.raw_digest()) == candidate.state_sha256
            && proof.frame() == candidate.frame_count,
        "MD live candidate proof differs"
    );
    Ok(())
}
