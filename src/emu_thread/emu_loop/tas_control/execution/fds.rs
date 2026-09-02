use super::{TasExecutionResult, replay_frame};
use crate::emu_backend::EmuBackend;
use crate::emu_thread::{
    EmuThread, TasExecutionProfile, TasExecutionRejectedReason as Rejected, TasExecutionRequest,
    TasFdsMediaEvent, TasInputFrame, WorkerRuntimeFault,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, MAX_START_STATE_BYTES, TasDigest};
use zeff_emu_common::media::{MediaEvent, MediaSlotId};

pub(super) fn execute_direct_fds_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: &TasExecutionRequest,
) -> Result<TasExecutionResult, Rejected> {
    validate_fds_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
    let transaction_frames =
        u64::try_from(request.input_prefix.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    if transaction_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
        return Err(Rejected::FrameLimitExceeded);
    }
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    restore_direct_fds_state(backend, &request.start_state_bytes)?;
    let expected_frame = backend
        .frame_count()
        .checked_add(transaction_frames)
        .ok_or(Rejected::FrameCountOverflow)?;
    let advanced = step_fds_inputs(backend, &request.input_prefix, runtime_fault)?;
    if advanced != request.input_prefix.len() {
        return Err(Rejected::FrameProgressFailed);
    }
    capture_direct_fds_candidate(backend, expected_frame).map(|(frame_count, state_sha256)| {
        TasExecutionResult {
            profile: TasExecutionProfile::DirectFdsDisk,
            frame_count,
            state_sha256,
            executed_project_frames: transaction_frames,
            segment_id: 1,
            segment_frame_count: transaction_frames,
            last_advance_id: 0,
            cache_proof: request.cache_proof,
        }
    })
}

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_fds_input(
    input: TasInputFrame,
) -> Result<(), ()> {
    if input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
        || input.zapper != Default::default()
        || [
            input.fds_disk_side.is_some(),
            input.fds_write_protected.is_some(),
            input.fds_media_event.is_some(),
        ]
        .into_iter()
        .filter(|present| *present)
        .count()
            > 1
    {
        return Err(());
    }
    Ok(())
}

pub(super) fn validate_fds_inputs(inputs: &[TasInputFrame]) -> Result<(), ()> {
    inputs.iter().copied().try_for_each(validate_fds_input)
}

pub(super) fn step_fds_inputs(
    backend: &mut EmuBackend,
    inputs: &[TasInputFrame],
    runtime_fault: &mut WorkerRuntimeFault,
) -> Result<usize, Rejected> {
    let mut advanced = 0;
    for input in inputs.iter().copied() {
        apply_fds_drive_input(backend, input).map_err(|_| Rejected::InvalidInput)?;
        let frames = [replay_frame(input)];
        let stepped = EmuThread::step_n_frames_with_runtime_fault(
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
        if stepped != 1 {
            return Err(Rejected::FrameProgressFailed);
        }
        advanced += 1;
    }
    Ok(advanced)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn apply_fds_drive_input(
    backend: &mut EmuBackend,
    input: TasInputFrame,
) -> Result<(), ()> {
    validate_fds_input(input)?;
    if let Some(side) = input.fds_disk_side {
        backend.set_fds_disk_side(side).map_err(|_| ())?;
    }
    if let Some(write_protected) = input.fds_write_protected {
        backend
            .apply_media_event(&MediaEvent::SetWriteProtected {
                slot: MediaSlotId::from(
                    zeff_nes_core::hardware::cartridge::mappers::FDS_DRIVE_SLOT_ID,
                ),
                write_protected,
            })
            .map_err(|_| ())?;
    }
    if let Some(event) = input.fds_media_event {
        let slot =
            MediaSlotId::from(zeff_nes_core::hardware::cartridge::mappers::FDS_DRIVE_SLOT_ID);
        let event = match event {
            TasFdsMediaEvent::Eject => MediaEvent::Eject { slot },
            TasFdsMediaEvent::Insert {
                side,
                write_protected,
            } => {
                let media_id = backend
                    .media_slot_snapshot()
                    .and_then(|snapshot| snapshot.source_media_id)
                    .ok_or(())?;
                MediaEvent::Insert {
                    slot,
                    media_id,
                    side: Some(side),
                    write_protected,
                }
            }
        };
        backend.apply_media_event(&event).map_err(|_| ())?;
    }
    Ok(())
}

pub(super) fn validate_direct_fds_start_state(
    backend: &EmuBackend,
    state: &[u8],
) -> Result<(), Rejected> {
    crate::emu_backend::loader::validate_current_nes_start_state(state)
        .map_err(|_| Rejected::InvalidStartState)?;
    crate::emu_backend::loader::validate_fds_tas_private_runtime(backend, false)
        .map_err(|_| Rejected::InvalidStartState)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn restore_direct_fds_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<(), Rejected> {
    validate_direct_fds_start_state(backend, state)?;
    backend
        .load_state_from_bytes(state.to_vec())
        .map_err(|_| Rejected::StartStateRestoreFailed)?;
    crate::emu_backend::loader::validate_fds_tas_private_runtime(backend, false)
        .map_err(|_| Rejected::InvalidStartState)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn restore_direct_fds_execution_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<(), Rejected> {
    crate::emu_backend::loader::validate_current_nes_start_state(state)
        .map_err(|_| Rejected::InvalidStartState)?;
    crate::emu_backend::loader::validate_fds_tas_execution_runtime(backend, false)
        .map_err(|_| Rejected::InvalidStartState)?;
    backend
        .load_state_from_bytes(state.to_vec())
        .map_err(|_| Rejected::StartStateRestoreFailed)?;
    crate::emu_backend::loader::validate_fds_tas_execution_runtime(backend, false)
        .map_err(|_| Rejected::InvalidStartState)
}

pub(super) fn capture_direct_fds_candidate(
    backend: &EmuBackend,
    expected_frame: u64,
) -> Result<(u64, TasDigest), Rejected> {
    crate::emu_backend::loader::validate_fds_tas_execution_runtime(backend, false)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok((frame_count, TasDigest::from_bytes(&state)))
}
