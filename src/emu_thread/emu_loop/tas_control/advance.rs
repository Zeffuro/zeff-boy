use crate::emu_thread::{
    EmuThread, TasExecutionProfile, TasFrameAdvanceRejectedReason as Rejected, TasInputFrame,
    WorkerRuntimeFault,
};
use crate::tas_project::TasDigest;

use super::execution::{
    TasExecutionResult, capture_direct_gb_candidate, replay_frame, validate_gb_input,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TasFrameAdvanceResult {
    pub(super) frame_count: u64,
    pub(super) state_sha256: TasDigest,
}

pub(super) fn advance_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
) -> Result<TasFrameAdvanceResult, Rejected> {
    match candidate.profile {
        TasExecutionProfile::DirectNesCartridge => {
            advance_direct_nes_tas_frame(backend, runtime_fault, candidate, input)
        }
        TasExecutionProfile::DirectGbRomOnlyDmg => {
            advance_direct_gb_tas_frame(backend, runtime_fault, candidate, input)
        }
    }
}

fn advance_direct_nes_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
) -> Result<TasFrameAdvanceResult, Rejected> {
    let current_state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateVerificationUnavailable)?;
    if TasDigest::from_bytes(&current_state) != candidate.state_sha256 {
        return Err(Rejected::CandidateStateDigestMismatch);
    }
    if backend.frame_count() != candidate.frame_count {
        return Err(Rejected::CandidateFrameCountMismatch);
    }
    let expected_frame = candidate
        .frame_count
        .checked_add(1)
        .ok_or(Rejected::FrameCountOverflow)?;
    let frames = [replay_frame(input)];
    let advanced = EmuThread::step_n_frames_with_runtime_fault(
        backend,
        1,
        &[],
        false,
        &mut Vec::new(),
        Some(&frames),
        runtime_fault,
    );
    if !runtime_fault.can_step() {
        return Err(Rejected::RuntimeFault);
    }
    if advanced != 1 {
        return Err(Rejected::FrameProgressFailed);
    }
    let state_bytes = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok(TasFrameAdvanceResult {
        frame_count,
        state_sha256: TasDigest::from_bytes(&state_bytes),
    })
}

fn advance_direct_gb_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
) -> Result<TasFrameAdvanceResult, Rejected> {
    validate_gb_input(input).map_err(|_| Rejected::InvalidInput)?;
    let state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateVerificationUnavailable)?;
    if TasDigest::from_bytes(&state) != candidate.state_sha256 {
        return Err(Rejected::CandidateStateDigestMismatch);
    }
    if backend.frame_count() != candidate.frame_count {
        return Err(Rejected::CandidateFrameCountMismatch);
    }
    let expected_frame = candidate
        .frame_count
        .checked_add(1)
        .ok_or(Rejected::FrameCountOverflow)?;
    let frames = [replay_frame(input)];
    let advanced = EmuThread::step_n_frames_with_runtime_fault(
        backend,
        1,
        &[],
        false,
        &mut Vec::new(),
        Some(&frames),
        runtime_fault,
    );
    if !runtime_fault.can_step() {
        return Err(Rejected::RuntimeFault);
    }
    if advanced != 1 {
        return Err(Rejected::FrameProgressFailed);
    }
    capture_direct_gb_candidate(backend, expected_frame)
        .map_err(|_| Rejected::StateCaptureFailed)
        .map(|(frame_count, state_sha256)| TasFrameAdvanceResult {
            frame_count,
            state_sha256,
        })
}
