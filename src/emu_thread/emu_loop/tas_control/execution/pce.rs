use crate::emu_backend::EmuBackend;
use crate::emu_thread::{
    EmuThread, TasExecutionProfile, TasExecutionRejectedReason as Rejected, TasExecutionRequest,
    TasInputFrame, WorkerRuntimeFault,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, MAX_START_STATE_BYTES, TasDigest};

use super::{TasExecutionResult, replay_frame};

pub(in crate::emu_thread::emu_loop::tas_control) fn execute_direct_pce_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: &TasExecutionRequest,
) -> Result<TasExecutionResult, Rejected> {
    validate_pce_inputs(request.profile, &request.input_prefix)
        .map_err(|_| Rejected::InvalidInput)?;
    let transaction_frames =
        u64::try_from(request.input_prefix.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    if transaction_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
        return Err(Rejected::FrameLimitExceeded);
    }
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    restore_direct_pce_state(backend, request.profile, &request.start_state_bytes)?;
    let expected_frame = backend
        .frame_count()
        .checked_add(transaction_frames)
        .ok_or(Rejected::FrameCountOverflow)?;
    let frames: Vec<_> = request
        .input_prefix
        .iter()
        .copied()
        .map(replay_frame)
        .collect();
    let advanced = EmuThread::step_n_frames_with_runtime_fault(
        backend,
        frames.len(),
        &[],
        false,
        &mut Vec::new(),
        Some(&frames),
        runtime_fault,
    );
    if !runtime_fault.can_step() {
        return Err(Rejected::RuntimeFault);
    }
    if advanced != frames.len() {
        return Err(Rejected::FrameProgressFailed);
    }
    backend.drain_audio_samples_into(&mut Vec::new());
    capture_direct_pce_candidate(backend, request.profile, expected_frame).map(
        |(frame_count, state_sha256)| TasExecutionResult {
            profile: request.profile,
            frame_count,
            state_sha256,
            executed_project_frames: transaction_frames,
            segment_id: 1,
            segment_frame_count: transaction_frames,
            last_advance_id: 0,
            cache_proof: request.cache_proof,
        },
    )
}

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_pce_input(
    profile: TasExecutionProfile,
    input: TasInputFrame,
) -> Result<(), ()> {
    let button_mask = match profile {
        TasExecutionProfile::DirectPceHuCard | TasExecutionProfile::DirectPceCd => 0x0F,
        TasExecutionProfile::DirectPceSixButtonHuCard => 0xFF,
        _ => return Err(()),
    };
    if input.p1_buttons & !button_mask != 0
        || input.p1_dpad & !0x0F != 0
        || input.p2_buttons != 0
        || input.p2_dpad != 0
        || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
        || input.zapper != Default::default()
    {
        return Err(());
    }
    Ok(())
}

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_pce_inputs(
    profile: TasExecutionProfile,
    inputs: &[TasInputFrame],
) -> Result<(), ()> {
    inputs
        .iter()
        .copied()
        .try_for_each(|input| validate_pce_input(profile, input))
}

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_direct_pce_start_state(
    backend: &EmuBackend,
    profile: TasExecutionProfile,
    state: &[u8],
) -> Result<(), Rejected> {
    validate_pce_runtime(backend, profile).map_err(|_| Rejected::InvalidStartState)?;
    let pce = backend.pce().ok_or(Rejected::InvalidStartState)?;
    let frame_count = if profile == TasExecutionProfile::DirectPceCd {
        let runtime = crate::emu_backend::loader::validate_direct_pce_cd_tas_execution_runtime(
            backend, false,
        )
        .map_err(|_| Rejected::InvalidStartState)?;
        let inspection = pce
            .inspect_current_native_cd_tas_state_for_profile(
                state,
                runtime.arcade_card_enabled,
                runtime.memory_base_enabled,
            )
            .map_err(|_| Rejected::InvalidStartState)?;
        if Some(inspection.disc_sha256) != backend.replay_metadata().rom_sha256 {
            return Err(Rejected::InvalidStartState);
        }
        inspection.projection.frame_count
    } else {
        let inspection = pce
            .inspect_current_native_tas_state(state)
            .map_err(|_| Rejected::InvalidStartState)?;
        if Some(inspection.normalized_rom_sha256) != backend.replay_metadata().rom_sha256 {
            return Err(Rejected::InvalidStartState);
        }
        inspection.projection.frame_count
    };
    if frame_count != parse_backend_frame_count(state).ok_or(Rejected::InvalidStartState)? {
        return Err(Rejected::InvalidStartState);
    }
    Ok(())
}

pub(in crate::emu_thread::emu_loop::tas_control) fn restore_direct_pce_state(
    backend: &mut EmuBackend,
    profile: TasExecutionProfile,
    state: &[u8],
) -> Result<(), Rejected> {
    validate_direct_pce_start_state(backend, profile, state)?;
    let projection = if profile == TasExecutionProfile::DirectPceCd {
        crate::emu_backend::loader::validate_direct_pce_cd_tas_state(backend, state)
    } else {
        crate::emu_backend::loader::validate_direct_pce_tas_state(backend, state)
    }
    .map_err(|_| Rejected::StartStateRestoreFailed)?;
    let pce = backend.pce().ok_or(Rejected::StateFrameMismatch)?;
    if backend.frame_count() != projection.frame_count
        || pce.tas_core_framebuffer() != projection.framebuffer.as_ref()
        || !pce.tas_presented_frame_is_current()
    {
        return Err(Rejected::StateFrameMismatch);
    }
    validate_pce_runtime(backend, profile).map_err(|_| Rejected::InvalidStartState)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn restore_direct_pce_cd_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<(), Rejected> {
    restore_direct_pce_state(backend, TasExecutionProfile::DirectPceCd, state)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn capture_direct_pce_candidate(
    backend: &EmuBackend,
    profile: TasExecutionProfile,
    expected_frame: u64,
) -> Result<(u64, TasDigest), Rejected> {
    validate_pce_runtime(backend, profile).map_err(|_| Rejected::StateCaptureFailed)?;
    let state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    validate_direct_pce_start_state(backend, profile, &state)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok((frame_count, TasDigest::from_bytes(&state)))
}

fn validate_pce_runtime(backend: &EmuBackend, profile: TasExecutionProfile) -> anyhow::Result<()> {
    match profile {
        TasExecutionProfile::DirectPceHuCard => {
            crate::emu_backend::loader::validate_direct_pce_tas_execution_runtime(backend, false)?;
        }
        TasExecutionProfile::DirectPceSixButtonHuCard => {
            crate::emu_backend::loader::validate_direct_pce_six_button_tas_execution_runtime(
                backend, false,
            )?;
        }
        TasExecutionProfile::DirectPceCd => {
            crate::emu_backend::loader::validate_direct_pce_cd_tas_execution_runtime(
                backend, false,
            )?;
        }
        _ => anyhow::bail!("invalid PC Engine TAS profile"),
    }
    Ok(())
}

fn parse_backend_frame_count(state: &[u8]) -> Option<u64> {
    let bytes = state.get(12..20)?;
    Some(u64::from_le_bytes(bytes.try_into().ok()?))
}
