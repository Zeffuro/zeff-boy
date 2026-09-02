use crate::emu_thread::{
    EmuThread, TasExecutionProfile, TasFrameAdvanceRejectedReason as Rejected,
    TasFrameAdvanceSnapshot, TasInputFrame, TasPersistenceContract, WorkerRuntimeFault,
};
use crate::tas_project::TasDigest;

use super::execution::{
    TasExecutionResult, capture_direct_coleco_candidate, replay_frame, step_coleco_inputs,
    validate_gb_input,
};

pub(super) struct TasFrameAdvanceResult {
    pub(super) frame_count: u64,
    pub(super) state_sha256: TasDigest,
    pub(super) rumble: bool,
    pub(super) audio_samples: Vec<f32>,
    pub(super) ui_data: Option<Box<crate::ui::UiFrameData>>,
}

pub(super) fn advance_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
    snapshot: Option<TasFrameAdvanceSnapshot>,
    persistence: TasPersistenceContract,
) -> Result<TasFrameAdvanceResult, Rejected> {
    if candidate.profile != TasExecutionProfile::DirectFdsDisk
        && (input.fds_disk_side.is_some()
            || input.fds_write_protected.is_some()
            || input.fds_media_event.is_some())
    {
        return Err(Rejected::InvalidInput);
    }
    match candidate.profile {
        TasExecutionProfile::DirectNesCartridge => {
            advance_direct_nes_tas_frame(backend, runtime_fault, candidate, input, snapshot)
        }
        TasExecutionProfile::DirectFdsDisk => {
            advance_direct_nes_tas_frame(backend, runtime_fault, candidate, input, snapshot)
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            advance_direct_gb_tas_frame(
                backend,
                runtime_fault,
                candidate,
                input,
                snapshot,
                persistence,
            )
        }
        TasExecutionProfile::DirectColecoCartridge => {
            advance_direct_coleco_tas_frame(backend, runtime_fault, candidate, input, snapshot)
        }
        TasExecutionProfile::DirectSmsCartridge => {
            advance_direct_sms_tas_frame(backend, runtime_fault, candidate, input, snapshot)
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            advance_direct_game_gear_tas_frame(backend, runtime_fault, candidate, input, snapshot)
        }
        TasExecutionProfile::DirectGbaCartridge => advance_direct_gba_tas_frame(
            backend,
            runtime_fault,
            candidate,
            input,
            snapshot,
            persistence,
        ),
        TasExecutionProfile::DirectSg1000Cartridge => {
            advance_direct_sg1000_tas_frame(backend, runtime_fault, candidate, input, snapshot)
        }
        TasExecutionProfile::DirectWsCartridge => advance_direct_ws_tas_frame(
            backend,
            runtime_fault,
            candidate,
            input,
            snapshot,
            persistence,
        ),
        TasExecutionProfile::DirectPceHuCard
        | TasExecutionProfile::DirectPceSixButtonHuCard
        | TasExecutionProfile::DirectPceCd => {
            advance_direct_pce_tas_frame(backend, runtime_fault, candidate, input, snapshot)
        }
    }
}

fn advance_direct_nes_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
    snapshot: Option<TasFrameAdvanceSnapshot>,
) -> Result<TasFrameAdvanceResult, Rejected> {
    if candidate.profile == TasExecutionProfile::DirectFdsDisk {
        super::execution::fds::validate_fds_input(input).map_err(|_| Rejected::InvalidInput)?;
    } else if input.fds_disk_side.is_some()
        || input.fds_write_protected.is_some()
        || input.fds_media_event.is_some()
    {
        return Err(Rejected::InvalidInput);
    }
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
    if candidate.profile == TasExecutionProfile::DirectFdsDisk {
        super::execution::fds::apply_fds_drive_input(backend, input)
            .map_err(|_| Rejected::InvalidInput)?;
    }
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
    let mut audio_samples = Vec::new();
    backend.drain_audio_samples_into(&mut audio_samples);
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
        rumble: backend.rumble_active(),
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

fn advance_direct_gb_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
    snapshot: Option<TasFrameAdvanceSnapshot>,
    persistence: TasPersistenceContract,
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
    let mut audio_samples = Vec::new();
    backend.drain_audio_samples_into(&mut audio_samples);
    super::execution::gb::capture_direct_gb_candidate(
        backend,
        expected_frame,
        candidate.profile,
        persistence,
    )
    .map_err(|_| Rejected::StateCaptureFailed)
    .map(|(frame_count, state_sha256)| TasFrameAdvanceResult {
        frame_count,
        state_sha256,
        rumble: backend.rumble_active(),
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

fn advance_direct_coleco_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
    snapshot: Option<TasFrameAdvanceSnapshot>,
) -> Result<TasFrameAdvanceResult, Rejected> {
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
    if step_coleco_inputs(backend, &[input], runtime_fault).map_err(map_coleco_advance_error)? != 1
    {
        return Err(Rejected::FrameProgressFailed);
    }
    let mut audio_samples = Vec::new();
    backend.drain_audio_samples_into(&mut audio_samples);
    capture_direct_coleco_candidate(backend, expected_frame)
        .map_err(|_| Rejected::StateCaptureFailed)
        .map(|(frame_count, state_sha256)| TasFrameAdvanceResult {
            frame_count,
            state_sha256,
            rumble: backend.rumble_active(),
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

fn advance_direct_sms_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
    snapshot: Option<TasFrameAdvanceSnapshot>,
) -> Result<TasFrameAdvanceResult, Rejected> {
    super::execution::sms::validate_sms_input(input).map_err(|_| Rejected::InvalidInput)?;
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
    let mut audio_samples = Vec::new();
    backend.drain_audio_samples_into(&mut audio_samples);
    super::execution::sms::capture_direct_sms_candidate(backend, expected_frame)
        .map_err(|_| Rejected::StateCaptureFailed)
        .map(|(frame_count, state_sha256)| TasFrameAdvanceResult {
            frame_count,
            state_sha256,
            rumble: backend.rumble_active(),
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

fn advance_direct_game_gear_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
    snapshot: Option<TasFrameAdvanceSnapshot>,
) -> Result<TasFrameAdvanceResult, Rejected> {
    super::execution::game_gear::validate_game_gear_input(input)
        .map_err(|_| Rejected::InvalidInput)?;
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
    let mut audio_samples = Vec::new();
    backend.drain_audio_samples_into(&mut audio_samples);
    super::execution::game_gear::capture_direct_game_gear_candidate(backend, expected_frame)
        .map_err(|_| Rejected::StateCaptureFailed)
        .map(|(frame_count, state_sha256)| TasFrameAdvanceResult {
            frame_count,
            state_sha256,
            rumble: backend.rumble_active(),
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

fn advance_direct_gba_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
    snapshot: Option<TasFrameAdvanceSnapshot>,
    persistence: TasPersistenceContract,
) -> Result<TasFrameAdvanceResult, Rejected> {
    super::execution::gba::validate_gba_input(input).map_err(|_| Rejected::InvalidInput)?;
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
    let mut audio_samples = Vec::new();
    backend.drain_audio_samples_into(&mut audio_samples);
    super::execution::gba::capture_direct_gba_candidate(backend, expected_frame, persistence)
        .map_err(|_| Rejected::StateCaptureFailed)
        .map(|(frame_count, state_sha256)| TasFrameAdvanceResult {
            frame_count,
            state_sha256,
            rumble: backend.rumble_active(),
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

fn advance_direct_sg1000_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
    snapshot: Option<TasFrameAdvanceSnapshot>,
) -> Result<TasFrameAdvanceResult, Rejected> {
    super::execution::sg1000::validate_sg1000_input(input).map_err(|_| Rejected::InvalidInput)?;
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
    let mut audio_samples = Vec::new();
    backend.drain_audio_samples_into(&mut audio_samples);
    super::execution::sg1000::capture_direct_sg1000_candidate(backend, expected_frame)
        .map_err(|_| Rejected::StateCaptureFailed)
        .map(|(frame_count, state_sha256)| TasFrameAdvanceResult {
            frame_count,
            state_sha256,
            rumble: backend.rumble_active(),
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

fn advance_direct_ws_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
    snapshot: Option<TasFrameAdvanceSnapshot>,
    persistence: TasPersistenceContract,
) -> Result<TasFrameAdvanceResult, Rejected> {
    super::execution::ws::validate_ws_input(input).map_err(|_| Rejected::InvalidInput)?;
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
    let mut audio_samples = Vec::new();
    backend.drain_audio_samples_into(&mut audio_samples);
    super::execution::ws::capture_direct_ws_candidate(backend, expected_frame, persistence)
        .map_err(|_| Rejected::StateCaptureFailed)
        .map(|(frame_count, state_sha256)| TasFrameAdvanceResult {
            frame_count,
            state_sha256,
            rumble: backend.rumble_active(),
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

fn advance_direct_pce_tas_frame(
    backend: &mut crate::emu_backend::EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    candidate: TasExecutionResult,
    input: TasInputFrame,
    snapshot: Option<TasFrameAdvanceSnapshot>,
) -> Result<TasFrameAdvanceResult, Rejected> {
    super::execution::pce::validate_pce_input(candidate.profile, input)
        .map_err(|_| Rejected::InvalidInput)?;
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
    let mut audio_samples = Vec::new();
    backend.drain_audio_samples_into(&mut audio_samples);
    super::execution::pce::capture_direct_pce_candidate(backend, candidate.profile, expected_frame)
        .map_err(|_| Rejected::StateCaptureFailed)
        .map(|(frame_count, state_sha256)| TasFrameAdvanceResult {
            frame_count,
            state_sha256,
            rumble: backend.rumble_active(),
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

fn map_coleco_advance_error(reason: crate::emu_thread::TasExecutionRejectedReason) -> Rejected {
    match reason {
        crate::emu_thread::TasExecutionRejectedReason::InvalidInput => Rejected::InvalidInput,
        crate::emu_thread::TasExecutionRejectedReason::RuntimeFault => Rejected::RuntimeFault,
        _ => Rejected::FrameProgressFailed,
    }
}
