use super::super::advance::TasFrameAdvanceResult;
use super::super::execution::{
    TasExecutionResult, validate_intermediate_cache_proofs, validate_predecessor_window,
};
use super::*;
use crate::emu_backend::loader::DirectMdTasExecutionLoader;
use crate::emu_core_trait::EmulatorCore;
use crate::emu_thread::{
    EmuThread, TasExecutionCacheProof, TasExecutionRejectedReason as Execute, TasExecutionRequest,
    TasFrameAdvanceRejectedReason as Advance, TasFrameAdvanceSnapshot, TasInputFrame,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, MAX_START_STATE_BYTES};

pub(super) struct MdAdvanceContext<'a> {
    pub(super) backend: &'a mut EmuBackend,
    pub(super) cache: &'a mut WorkerTasStateCache,
    pub(super) cache_proof: Option<TasExecutionCacheProof>,
}

impl MdLiveControl {
    pub(super) fn execute(
        &mut self,
        backend: &mut EmuBackend,
        cache: &mut WorkerTasStateCache,
        request: &TasExecutionRequest,
    ) -> Result<TasExecutionResult, Execute> {
        if self.failed {
            return Err(Execute::RuntimeFault);
        }
        validate_request(request)?;
        let EmuBackend::Md(md) = backend else {
            return Err(Execute::InvalidStartState);
        };
        let guard = md
            .capture_tas_live_guard(&self.lease)
            .map_err(|_| Execute::StateCaptureFailed)?;
        let was_seeded = self.seeded;
        let old_proof = self.proof.clone();
        let result = self.execute_owned(md, cache, request, guard.snapshot());
        match result {
            Ok(result) => Ok(result),
            Err(reason) => {
                cache.clear_md();
                if md.restore_tas_live_guard(&self.lease, &guard).is_err() {
                    self.failed = true;
                    return Err(Execute::RuntimeFault);
                }
                self.seeded = was_seeded;
                self.proof = old_proof;
                Err(reason)
            }
        }
    }

    fn execute_owned(
        &mut self,
        md: &mut crate::emu_backend::md::MdBackend,
        cache: &mut WorkerTasStateCache,
        request: &TasExecutionRequest,
        guard: &crate::emu_backend::md::tas::MdTasLiveSnapshot,
    ) -> Result<TasExecutionResult, Execute> {
        cache.reserve_md(self.reservation.saturating_add(
            request.intermediate_cache_proofs.capacity()
                * std::mem::size_of::<TasExecutionCacheProof>(),
        ));
        let load = md
            .tas_live_load(&self.lease)
            .map_err(|_| Execute::InvalidStartState)?
            .clone();
        if md_product::md_pad_profile(request.profile) != Some(load.profile()) {
            return Err(Execute::NonStandardControllerTopology);
        }
        load.validate_source(md)
            .map_err(|_| Execute::InvalidStartState)?;
        let loader = if load.source().member().is_some() {
            DirectMdTasExecutionLoader::new_zip(
                md.source_path().to_owned(),
                md.rom_path().to_owned(),
                load.profile(),
            )
        } else {
            DirectMdTasExecutionLoader::new(md.source_path().to_owned(), load.profile())
        };
        let project = loader
            .create_project()
            .map_err(|_| Execute::InvalidStartState)?;
        if project.start_state() != request.start_state_bytes
            || project
                .sync_identity_sha256()
                .map_err(|_| Execute::InvalidCacheProof)?
                != request.cache_proof.sync_identity_sha256
        {
            return Err(Execute::InvalidCacheProof);
        }
        if !self.seeded {
            let fresh = loader
                .load_fresh_backend()
                .map_err(|_| Execute::InvalidStartState)?;
            let EmuBackend::Md(fresh) = fresh else {
                unreachable!()
            };
            if fresh.tas_load().map_err(|_| Execute::InvalidStartState)? != &load {
                return Err(Execute::InvalidStartState);
            }
            let seed = fresh
                .capture_authenticated_tas_seed(&request.start_state_bytes)
                .map_err(|_| Execute::InvalidStartState)?;
            md.install_authenticated_tas_seed(&self.lease, seed)
                .map_err(|_| Execute::StartStateRestoreFailed)?;
            self.seeded = true;
        }
        cache
            .bound_md(md, &self.lease, &[guard])
            .map_err(|_| Execute::StateCaptureFailed)?;
        let schema = "md-raw-v1-fresh-project-owned-runtime-v1";
        let source_proofs = request
            .predecessor_window
            .as_ref()
            .map_or(std::slice::from_ref(&request.cache_proof), |window| {
                window.source_proofs.as_slice()
            });
        let cached = cache.get_md(request.profile, schema, source_proofs);
        let (start, inputs) = if let Some((proof, state)) = cached {
            if state.proof().frame() != proof.target_cursor {
                return Err(Execute::InvalidCacheProof);
            }
            md.restore_tas_live(&self.lease, &state)
                .map_err(|_| Execute::InvalidCacheProof)?;
            if proof == request.cache_proof {
                (proof.target_cursor, &[][..])
            } else {
                let window = request
                    .predecessor_window
                    .as_ref()
                    .ok_or(Execute::InvalidCacheProof)?;
                let offset = proof
                    .target_cursor
                    .checked_sub(window.input_start_cursor)
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or(Execute::InvalidCacheProof)?;
                (
                    proof.target_cursor,
                    window
                        .input_frames
                        .get(offset..)
                        .ok_or(Execute::InvalidCacheProof)?,
                )
            }
        } else {
            md.rebuild_tas_live_candidate(&self.lease)
                .map_err(|_| Execute::StartStateRestoreFailed)?;
            (0, request.input_prefix.as_slice())
        };
        let count = u64::try_from(inputs.len()).map_err(|_| Execute::FrameLimitExceeded)?;
        let reached = start
            .checked_add(count)
            .ok_or(Execute::FrameCountOverflow)?;
        if count > MAX_EDITOR_SEEK_EXECUTION_FRAMES || reached > request.cache_proof.target_cursor {
            return Err(Execute::InvalidCacheProof);
        }
        for input in inputs {
            let before = md
                .tas_live_proof(&self.lease)
                .map_err(|_| Execute::StateCaptureFailed)?;
            let frame = md
                .advance_tas_live(&self.lease, ports(*input))
                .map_err(|_| Execute::FrameProgressFailed)?;
            if before.frame().checked_add(1) != Some(frame.field)
                || frame.master_tick <= before.master_tick()
            {
                return Err(Execute::FrameProgressFailed);
            }
        }
        load.validate_source(md)
            .map_err(|_| Execute::InvalidStartState)?;
        let proof = md
            .tas_live_proof(&self.lease)
            .map_err(|_| Execute::StateCaptureFailed)?;
        if proof.frame() != reached || proof.queued_audio_frames() != 0 {
            return Err(Execute::StateFrameMismatch);
        }
        let cache_proof = if reached == request.cache_proof.target_cursor {
            Some(request.cache_proof)
        } else {
            request
                .intermediate_cache_proofs
                .iter()
                .find(|p| p.target_cursor == reached)
                .copied()
        };
        if let Some(cache_proof) = cache_proof {
            let state = md
                .capture_tas_live(&self.lease)
                .map_err(|_| Execute::StateCaptureFailed)?;
            cache
                .insert_md(request.profile, schema, cache_proof, state)
                .map_err(|_| Execute::StateCaptureFailed)?;
        }
        cache
            .bound_md(md, &self.lease, &[guard])
            .map_err(|_| Execute::StateCaptureFailed)?;
        self.proof = Some(proof.clone());
        Ok(TasExecutionResult {
            profile: request.profile,
            frame_count: proof.frame(),
            state_sha256: TasDigest(proof.raw_digest()),
            executed_project_frames: reached,
            segment_id: 1,
            segment_frame_count: count,
            last_advance_id: 0,
            cache_proof: request.cache_proof,
        })
    }

    pub(super) fn advance(
        &mut self,
        context: MdAdvanceContext<'_>,
        candidate: TasExecutionResult,
        input: TasInputFrame,
        snapshot: Option<TasFrameAdvanceSnapshot>,
    ) -> Result<TasFrameAdvanceResult, Advance> {
        let MdAdvanceContext {
            backend,
            cache,
            cache_proof,
        } = context;
        if self.failed {
            return Err(Advance::RuntimeFault);
        }
        md_product::validate_md_tas_worker_input(candidate.profile, input)
            .map_err(|_| Advance::InvalidInput)?;
        let EmuBackend::Md(md) = backend else {
            return Err(Advance::StateVerificationUnavailable);
        };
        let guard = md
            .capture_tas_live_guard(&self.lease)
            .map_err(|_| Advance::StateCaptureFailed)?;
        let old_proof = self.proof.clone();
        let result = (|| {
            finalization::validate_candidate(md, self, candidate)
                .map_err(|_| Advance::CandidateProofMismatch)?;
            cache
                .bound_md(md, &self.lease, &[guard.snapshot()])
                .map_err(|_| Advance::StateCaptureFailed)?;
            if let Some(proof) = cache_proof {
                let state = md
                    .capture_tas_live(&self.lease)
                    .map_err(|_| Advance::StateCaptureFailed)?;
                cache
                    .insert_md(
                        candidate.profile,
                        "md-raw-v1-fresh-project-owned-runtime-v1",
                        proof,
                        state,
                    )
                    .map_err(|_| Advance::StateCaptureFailed)?;
            }
            let frame = md
                .advance_tas_live(&self.lease, ports(input))
                .map_err(|_| Advance::FrameProgressFailed)?;
            let proof = md
                .tas_live_proof(&self.lease)
                .map_err(|_| Advance::StateCaptureFailed)?;
            if candidate.frame_count.checked_add(1) != Some(frame.field)
                || old_proof
                    .as_ref()
                    .is_none_or(|p| frame.master_tick <= p.master_tick())
                || proof.queued_audio_frames() != 0
            {
                return Err(Advance::StateFrameMismatch);
            }
            md.tas_live_load(&self.lease)
                .and_then(|load| load.validate_source(md))
                .map_err(|_| Advance::StateVerificationUnavailable)?;
            cache
                .bound_md(md, &self.lease, &[guard.snapshot()])
                .map_err(|_| Advance::StateCaptureFailed)?;
            self.proof = Some(proof.clone());
            Ok((proof, frame.audio))
        })();
        let (proof, audio_samples) = match result {
            Ok(value) => value,
            Err(reason) => {
                cache.clear_md();
                if md.restore_tas_live_guard(&self.lease, &guard).is_err() {
                    self.failed = true;
                    return Err(Advance::RuntimeFault);
                }
                self.proof = old_proof;
                return Err(reason);
            }
        };
        Ok(TasFrameAdvanceResult {
            frame_count: proof.frame(),
            state_sha256: TasDigest(proof.raw_digest()),
            rumble: false,
            audio_samples,
            ui_data: snapshot.map(|snapshot| {
                Box::new(EmuThread::collect_ui_snapshot(
                    backend,
                    &snapshot.request,
                    snapshot.buffers,
                ))
            }),
        })
    }
}

fn validate_request(request: &TasExecutionRequest) -> Result<(), Execute> {
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Execute::StartStateTooLarge);
    }
    let expected = request
        .cache_proof
        .target_cursor
        .min(MAX_EDITOR_SEEK_EXECUTION_FRAMES);
    if request.input_prefix.len() as u64 != expected {
        return Err(Execute::InvalidCacheProof);
    }
    validate_predecessor_window(request)?;
    validate_intermediate_cache_proofs(request)?;
    for input in request.input_prefix.iter().chain(
        request
            .predecessor_window
            .iter()
            .flat_map(|w| &w.input_frames),
    ) {
        md_product::validate_md_tas_worker_input(request.profile, *input)
            .map_err(|_| Execute::InvalidInput)?;
    }
    Ok(())
}

fn ports(input: TasInputFrame) -> [u16; 2] {
    [
        u16::from(input.p1_buttons) | u16::from(input.p1_dpad) << 8,
        u16::from(input.p2_buttons) | u16::from(input.p2_dpad) << 8,
    ]
}
