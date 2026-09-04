use zeff_emu_common::replay::ReplayJoypadFrame;

use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{
    EmuThread, TasExecutionCacheProof, TasExecutionProfile, TasExecutionRejectedReason as Rejected,
    TasExecutionRequest, TasInputFrame, TasPersistenceContract, WorkerRuntimeFault,
    tas_intermediate_cache_cursors,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, MAX_START_STATE_BYTES, TasDigest};

use super::state_cache::CachedTasState;

pub(super) mod fds;
pub(super) mod game_gear;
pub(super) mod gb;
pub(in crate::emu_thread::emu_loop) mod gba;
pub(super) mod pce;
pub(super) mod sg1000;
pub(super) mod sms;
pub(super) mod ws;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TasExecutionResult {
    pub(super) profile: TasExecutionProfile,
    pub(super) frame_count: u64,
    pub(super) state_sha256: TasDigest,
    pub(super) executed_project_frames: u64,
    pub(super) segment_id: u64,
    pub(super) segment_frame_count: u64,
    pub(super) last_advance_id: u64,
    pub(super) cache_proof: crate::emu_thread::TasExecutionCacheProof,
}

pub(super) fn execute_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: TasExecutionRequest,
    cached_state: Option<(TasExecutionCacheProof, CachedTasState)>,
    persistence: TasPersistenceContract,
) -> Result<TasExecutionResult, Rejected> {
    validate_request_for_cache(backend, &request, persistence)?;
    let result = if let Some((source_proof, cached_state)) = cached_state {
        if source_proof == request.cache_proof {
            match restore_cached_tas(backend, &request, cached_state, persistence) {
                Ok(result) => result,
                Err(_) => execute_fresh_tas(backend, runtime_fault, &request, persistence)?,
            }
        } else if restore_cached_profile_state(backend, request.profile, cached_state, persistence)
            .is_ok()
        {
            execute_cached_suffix(backend, runtime_fault, &request, source_proof, persistence)?
        } else {
            execute_fresh_tas(backend, runtime_fault, &request, persistence)?
        }
    } else {
        execute_fresh_tas(backend, runtime_fault, &request, persistence)?
    };
    backend.drain_audio_samples_into(&mut Vec::new());
    Ok(result)
}

fn execute_fresh_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: &TasExecutionRequest,
    persistence: TasPersistenceContract,
) -> Result<TasExecutionResult, Rejected> {
    match request.profile {
        TasExecutionProfile::DirectNesCartridge => {
            execute_direct_nes_tas(backend, runtime_fault, request)
        }
        TasExecutionProfile::DirectFdsDisk => {
            fds::execute_direct_fds_tas(backend, runtime_fault, request)
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            execute_direct_gb_tas(backend, runtime_fault, request, persistence)
        }
        TasExecutionProfile::DirectColecoCartridge => {
            execute_direct_coleco_tas(backend, runtime_fault, request)
        }
        TasExecutionProfile::DirectSmsCartridge => {
            sms::execute_direct_sms_tas(backend, runtime_fault, request)
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            game_gear::execute_direct_game_gear_tas(backend, runtime_fault, request)
        }
        TasExecutionProfile::DirectGbaCartridge => {
            gba::execute_direct_gba_tas(backend, runtime_fault, request, persistence)
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            sg1000::execute_direct_sg1000_tas(backend, runtime_fault, request)
        }
        TasExecutionProfile::DirectWsCartridge => {
            ws::execute_direct_ws_tas(backend, runtime_fault, request, persistence)
        }
        TasExecutionProfile::DirectPceHuCard
        | TasExecutionProfile::DirectPceSixButtonHuCard
        | TasExecutionProfile::DirectPceMultitapHuCard
        | TasExecutionProfile::DirectPceCd
        | TasExecutionProfile::DirectPceMultitapCd => {
            pce::execute_direct_pce_tas(backend, runtime_fault, request)
        }
    }
}

fn execute_cached_suffix(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: &TasExecutionRequest,
    source_proof: TasExecutionCacheProof,
    persistence: TasPersistenceContract,
) -> Result<TasExecutionResult, Rejected> {
    let window = request
        .predecessor_window
        .as_ref()
        .ok_or(Rejected::InvalidCacheProof)?;
    let offset = source_proof
        .target_cursor
        .checked_sub(window.input_start_cursor)
        .and_then(|offset| usize::try_from(offset).ok())
        .ok_or(Rejected::InvalidCacheProof)?;
    let inputs = window
        .input_frames
        .get(offset..)
        .ok_or(Rejected::InvalidCacheProof)?;
    match request.profile {
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            validate_gb_inputs(inputs).map_err(|_| Rejected::InvalidInput)?;
        }
        TasExecutionProfile::DirectColecoCartridge => {
            validate_coleco_inputs(inputs).map_err(|_| Rejected::InvalidInput)?;
        }
        TasExecutionProfile::DirectSmsCartridge => {
            sms::validate_sms_inputs(inputs).map_err(|_| Rejected::InvalidInput)?;
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            game_gear::validate_game_gear_inputs(inputs).map_err(|_| Rejected::InvalidInput)?;
        }
        TasExecutionProfile::DirectGbaCartridge => {
            gba::validate_gba_inputs(inputs).map_err(|_| Rejected::InvalidInput)?;
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            sg1000::validate_sg1000_inputs(inputs).map_err(|_| Rejected::InvalidInput)?;
        }
        TasExecutionProfile::DirectWsCartridge => {
            ws::validate_ws_inputs(inputs).map_err(|_| Rejected::InvalidInput)?;
        }
        TasExecutionProfile::DirectPceHuCard
        | TasExecutionProfile::DirectPceSixButtonHuCard
        | TasExecutionProfile::DirectPceMultitapHuCard
        | TasExecutionProfile::DirectPceCd
        | TasExecutionProfile::DirectPceMultitapCd => {
            pce::validate_pce_inputs(request.profile, inputs)
                .map_err(|_| Rejected::InvalidInput)?;
        }
        TasExecutionProfile::DirectNesCartridge => {}
        TasExecutionProfile::DirectFdsDisk => {
            fds::validate_fds_inputs(inputs).map_err(|_| Rejected::InvalidInput)?;
        }
    }
    let transaction_frames =
        u64::try_from(inputs.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    let executed_project_frames = source_proof
        .target_cursor
        .checked_add(transaction_frames)
        .ok_or(Rejected::FrameCountOverflow)?;
    if transaction_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES
        || executed_project_frames > request.cache_proof.target_cursor
    {
        return Err(Rejected::InvalidCacheProof);
    }
    let start_frame = backend.frame_count();
    let expected_frame = start_frame
        .checked_add(transaction_frames)
        .ok_or(Rejected::FrameCountOverflow)?;
    let advanced = match request.profile {
        TasExecutionProfile::DirectColecoCartridge => {
            step_coleco_inputs(backend, inputs, runtime_fault)?
        }
        TasExecutionProfile::DirectFdsDisk => fds::step_fds_inputs(backend, inputs, runtime_fault)?,
        _ => {
            let frames: Vec<_> = inputs.iter().copied().map(replay_frame).collect();
            EmuThread::step_n_frames_with_runtime_fault(
                backend,
                frames.len(),
                &[],
                false,
                &mut Vec::new(),
                Some(&frames),
                runtime_fault,
            )
        }
    };
    if !runtime_fault.can_step() {
        return Err(Rejected::RuntimeFault);
    }
    if advanced != inputs.len() {
        return Err(Rejected::FrameProgressFailed);
    }
    if matches!(
        request.profile,
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb
    ) {
        gb::validate_direct_gb_profile_runtime(backend, request.profile, persistence)
            .map_err(|_| Rejected::StateCaptureFailed)?;
    } else if request.profile == TasExecutionProfile::DirectGbaCartridge {
        gba::validate_direct_gba_profile_runtime(backend, persistence)
            .map_err(|_| Rejected::StateCaptureFailed)?;
    }
    let state_bytes = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok(TasExecutionResult {
        profile: request.profile,
        frame_count,
        state_sha256: super::tas_state_digest(request.profile, &state_bytes),
        executed_project_frames,
        segment_id: 1,
        segment_frame_count: transaction_frames,
        last_advance_id: 0,
        cache_proof: request.cache_proof,
    })
}

fn execute_direct_nes_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: &TasExecutionRequest,
) -> Result<TasExecutionResult, Rejected> {
    let transaction_frames =
        u64::try_from(request.input_prefix.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    if transaction_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
        return Err(Rejected::FrameLimitExceeded);
    }
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    crate::emu_backend::loader::validate_current_nes_start_state(&request.start_state_bytes)
        .map_err(|_| Rejected::InvalidStartState)?;
    backend
        .load_state_from_bytes(request.start_state_bytes.clone())
        .map_err(|_| Rejected::StartStateRestoreFailed)?;
    if backend.system() != ActiveSystem::Nes
        || backend.nes_has_standard_or_zapper_controller_topology() != Some(true)
        || !backend
            .nes()
            .is_some_and(|nes| nes.has_standard_console_hardware())
    {
        return Err(Rejected::NonStandardControllerTopology);
    }
    let start_frame = backend.frame_count();
    let expected_frame = start_frame
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
    let state_bytes = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok(TasExecutionResult {
        profile: request.profile,
        frame_count,
        state_sha256: TasDigest::from_bytes(&state_bytes),
        executed_project_frames: transaction_frames,
        segment_id: 1,
        segment_frame_count: transaction_frames,
        last_advance_id: 0,
        cache_proof: request.cache_proof,
    })
}

fn execute_direct_gb_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: &TasExecutionRequest,
    persistence: TasPersistenceContract,
) -> Result<TasExecutionResult, Rejected> {
    validate_gb_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
    let transaction_frames =
        u64::try_from(request.input_prefix.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    if transaction_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
        return Err(Rejected::FrameLimitExceeded);
    }
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    gb::restore_direct_gb_state(
        backend,
        &request.start_state_bytes,
        request.profile,
        persistence,
    )?;
    let start_frame = backend.frame_count();
    let expected_frame = start_frame
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
    gb::capture_direct_gb_candidate(backend, expected_frame, request.profile, persistence).map(
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

fn execute_direct_coleco_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: &TasExecutionRequest,
) -> Result<TasExecutionResult, Rejected> {
    validate_coleco_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
    let transaction_frames =
        u64::try_from(request.input_prefix.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    if transaction_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
        return Err(Rejected::FrameLimitExceeded);
    }
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    restore_direct_coleco_state(backend, &request.start_state_bytes)?;
    let expected_frame = backend
        .frame_count()
        .checked_add(transaction_frames)
        .ok_or(Rejected::FrameCountOverflow)?;
    let advanced = step_coleco_inputs(backend, &request.input_prefix, runtime_fault)?;
    if advanced != request.input_prefix.len() {
        return Err(Rejected::FrameProgressFailed);
    }
    capture_direct_coleco_candidate(backend, expected_frame).map(|(frame_count, state_sha256)| {
        TasExecutionResult {
            profile: TasExecutionProfile::DirectColecoCartridge,
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

fn validate_request_for_cache(
    backend: &EmuBackend,
    request: &TasExecutionRequest,
    persistence: TasPersistenceContract,
) -> Result<(), Rejected> {
    if request.profile != TasExecutionProfile::DirectFdsDisk
        && request.input_prefix.iter().any(|input| {
            input.fds_disk_side.is_some()
                || input.fds_write_protected.is_some()
                || input.fds_media_event.is_some()
        })
    {
        return Err(Rejected::InvalidInput);
    }
    if !matches!(
        request.profile,
        TasExecutionProfile::DirectPceMultitapHuCard | TasExecutionProfile::DirectPceMultitapCd
    ) && request.input_prefix.iter().any(|input| {
        input.p3_buttons != 0
            || input.p3_dpad != 0
            || input.p4_buttons != 0
            || input.p4_dpad != 0
            || input.p5_buttons != 0
            || input.p5_dpad != 0
    }) {
        return Err(Rejected::InvalidInput);
    }
    let input_frames =
        u64::try_from(request.input_prefix.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    if input_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
        return Err(Rejected::FrameLimitExceeded);
    }
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    match request.profile {
        TasExecutionProfile::DirectNesCartridge => {
            crate::emu_backend::loader::validate_current_nes_start_state(
                &request.start_state_bytes,
            )
            .map_err(|_| Rejected::InvalidStartState)?;
        }
        TasExecutionProfile::DirectFdsDisk => {
            fds::validate_fds_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
            fds::validate_direct_fds_start_state(backend, &request.start_state_bytes)?;
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            validate_gb_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
            gb::validate_direct_gb_start_state(
                backend,
                &request.start_state_bytes,
                request.profile,
                persistence,
            )?;
        }
        TasExecutionProfile::DirectColecoCartridge => {
            validate_coleco_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
            validate_direct_coleco_start_state(backend, &request.start_state_bytes)?;
        }
        TasExecutionProfile::DirectSmsCartridge => {
            sms::validate_sms_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
            sms::validate_direct_sms_start_state(backend, &request.start_state_bytes)?;
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            game_gear::validate_game_gear_inputs(&request.input_prefix)
                .map_err(|_| Rejected::InvalidInput)?;
            game_gear::validate_direct_game_gear_start_state(backend, &request.start_state_bytes)?;
        }
        TasExecutionProfile::DirectGbaCartridge => {
            gba::validate_gba_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
            gba::validate_direct_gba_start_state(backend, &request.start_state_bytes, persistence)?;
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            sg1000::validate_sg1000_inputs(&request.input_prefix)
                .map_err(|_| Rejected::InvalidInput)?;
            sg1000::validate_direct_sg1000_start_state(backend, &request.start_state_bytes)?;
        }
        TasExecutionProfile::DirectWsCartridge => {
            ws::validate_ws_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
            ws::validate_direct_ws_start_state(backend, &request.start_state_bytes, persistence)?;
        }
        TasExecutionProfile::DirectPceHuCard
        | TasExecutionProfile::DirectPceSixButtonHuCard
        | TasExecutionProfile::DirectPceMultitapHuCard
        | TasExecutionProfile::DirectPceCd
        | TasExecutionProfile::DirectPceMultitapCd => {
            pce::validate_pce_inputs(request.profile, &request.input_prefix)
                .map_err(|_| Rejected::InvalidInput)?;
            pce::validate_direct_pce_start_state(
                backend,
                request.profile,
                &request.start_state_bytes,
            )?;
        }
    }
    validate_predecessor_window(request)?;
    validate_intermediate_cache_proofs(request)?;
    if input_frames
        != request
            .cache_proof
            .target_cursor
            .min(MAX_EDITOR_SEEK_EXECUTION_FRAMES)
    {
        return Err(Rejected::InvalidCacheProof);
    }
    Ok(())
}

fn validate_predecessor_window(request: &TasExecutionRequest) -> Result<(), Rejected> {
    let Some(window) = &request.predecessor_window else {
        return Ok(());
    };
    if window.source_proofs.len() < 2 || window.source_proofs.len() > 16 {
        return Err(Rejected::InvalidCacheProof);
    }
    let target = request.cache_proof.target_cursor;
    let input_len =
        u64::try_from(window.input_frames.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    let input_end_cursor = window
        .input_start_cursor
        .checked_add(input_len)
        .ok_or(Rejected::InvalidCacheProof)?;
    if window.input_start_cursor == 0
        || window.input_start_cursor >= target
        || input_len != (target - window.input_start_cursor).min(MAX_EDITOR_SEEK_EXECUTION_FRAMES)
    {
        return Err(Rejected::InvalidCacheProof);
    }
    if window.source_proofs.first() != Some(&request.cache_proof)
        || window.source_proofs.last().map(|proof| proof.target_cursor)
            != Some(window.input_start_cursor)
    {
        return Err(Rejected::InvalidCacheProof);
    }
    let mut previous_cursor = None;
    for proof in &window.source_proofs {
        if proof.sync_identity_sha256 != request.cache_proof.sync_identity_sha256
            || proof.target_cursor < window.input_start_cursor
            || (proof.target_cursor > input_end_cursor && proof.target_cursor != target)
            || previous_cursor.is_some_and(|previous| proof.target_cursor >= previous)
        {
            return Err(Rejected::InvalidCacheProof);
        }
        previous_cursor = Some(proof.target_cursor);
    }
    Ok(())
}

fn validate_intermediate_cache_proofs(request: &TasExecutionRequest) -> Result<(), Rejected> {
    if request
        .intermediate_cache_proofs
        .iter()
        .map(|proof| proof.target_cursor)
        .ne(tas_intermediate_cache_cursors(
            request.cache_proof.target_cursor,
        ))
    {
        return Err(Rejected::InvalidCacheProof);
    }
    let mut previous = 0;
    for proof in &request.intermediate_cache_proofs {
        if proof.sync_identity_sha256 != request.cache_proof.sync_identity_sha256
            || proof.target_cursor <= previous
            || proof.target_cursor >= request.cache_proof.target_cursor
        {
            return Err(Rejected::InvalidCacheProof);
        }
        previous = proof.target_cursor;
    }
    Ok(())
}

fn validate_direct_coleco_start_state(backend: &EmuBackend, state: &[u8]) -> Result<(), Rejected> {
    crate::emu_backend::loader::validate_direct_coleco_tas_execution_runtime(backend, false)
        .map_err(|_| Rejected::InvalidStartState)?;
    let inspection = zeff_coleco_core::save_state::inspect_current_native_tas_state_identity(state)
        .map_err(|_| Rejected::InvalidStartState)?;
    let coleco = backend.coleco().ok_or(Rejected::InvalidStartState)?;
    if inspection.cartridge_sha256 != coleco.emu.cartridge_hash()
        || inspection.bios_sha256 != coleco.emu.bios_hash()
    {
        return Err(Rejected::InvalidStartState);
    }
    Ok(())
}

fn restore_cached_tas(
    backend: &mut EmuBackend,
    request: &TasExecutionRequest,
    cached_state: CachedTasState,
    persistence: TasPersistenceContract,
) -> Result<TasExecutionResult, Rejected> {
    if cached_state.bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    match request.profile {
        TasExecutionProfile::DirectNesCartridge => {
            crate::emu_backend::loader::validate_current_nes_start_state(&cached_state.bytes)
                .map_err(|_| Rejected::InvalidStartState)?;
            backend
                .load_state_from_bytes(cached_state.bytes)
                .map_err(|_| Rejected::StartStateRestoreFailed)?;
        }
        TasExecutionProfile::DirectFdsDisk => {
            fds::restore_direct_fds_execution_state(backend, &cached_state.bytes)?;
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            gb::restore_direct_gb_state(
                backend,
                &cached_state.bytes,
                request.profile,
                persistence,
            )?;
        }
        TasExecutionProfile::DirectColecoCartridge => {
            restore_direct_coleco_state(backend, &cached_state.bytes)?;
        }
        TasExecutionProfile::DirectSmsCartridge => {
            sms::restore_direct_sms_state(backend, &cached_state.bytes)?;
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            game_gear::restore_direct_game_gear_state(backend, &cached_state.bytes)?;
        }
        TasExecutionProfile::DirectGbaCartridge => {
            gba::restore_direct_gba_state(backend, &cached_state.bytes, persistence)?;
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            sg1000::restore_direct_sg1000_state(backend, &cached_state.bytes)?;
        }
        TasExecutionProfile::DirectWsCartridge => {
            ws::restore_direct_ws_state(backend, &cached_state.bytes, persistence)?;
        }
        TasExecutionProfile::DirectPceHuCard
        | TasExecutionProfile::DirectPceSixButtonHuCard
        | TasExecutionProfile::DirectPceMultitapHuCard
        | TasExecutionProfile::DirectPceCd
        | TasExecutionProfile::DirectPceMultitapCd => {
            pce::restore_direct_pce_state(backend, request.profile, &cached_state.bytes)?;
        }
    }
    let restored = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != cached_state.frame_count
        || super::tas_state_digest(request.profile, &restored) != cached_state.sha256
    {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok(TasExecutionResult {
        profile: request.profile,
        frame_count,
        state_sha256: cached_state.sha256,
        executed_project_frames: request.cache_proof.target_cursor,
        segment_id: 1,
        segment_frame_count: 0,
        last_advance_id: 0,
        cache_proof: request.cache_proof,
    })
}

fn restore_cached_profile_state(
    backend: &mut EmuBackend,
    profile: TasExecutionProfile,
    cached_state: CachedTasState,
    persistence: TasPersistenceContract,
) -> Result<(), Rejected> {
    if cached_state.bytes.len() > MAX_START_STATE_BYTES
        || super::tas_state_digest(profile, &cached_state.bytes) != cached_state.sha256
    {
        return Err(Rejected::InvalidStartState);
    }
    match profile {
        TasExecutionProfile::DirectNesCartridge => {
            crate::emu_backend::loader::validate_current_nes_start_state(&cached_state.bytes)
                .map_err(|_| Rejected::InvalidStartState)?;
            backend
                .load_state_from_bytes(cached_state.bytes)
                .map_err(|_| Rejected::StartStateRestoreFailed)?;
        }
        TasExecutionProfile::DirectFdsDisk => {
            fds::restore_direct_fds_execution_state(backend, &cached_state.bytes)?;
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            gb::restore_direct_gb_state(backend, &cached_state.bytes, profile, persistence)?;
        }
        TasExecutionProfile::DirectColecoCartridge => {
            restore_direct_coleco_state(backend, &cached_state.bytes)?;
        }
        TasExecutionProfile::DirectSmsCartridge => {
            sms::restore_direct_sms_state(backend, &cached_state.bytes)?;
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            game_gear::restore_direct_game_gear_state(backend, &cached_state.bytes)?;
        }
        TasExecutionProfile::DirectGbaCartridge => {
            gba::restore_direct_gba_state(backend, &cached_state.bytes, persistence)?;
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            sg1000::restore_direct_sg1000_state(backend, &cached_state.bytes)?;
        }
        TasExecutionProfile::DirectWsCartridge => {
            ws::restore_direct_ws_state(backend, &cached_state.bytes, persistence)?;
        }
        TasExecutionProfile::DirectPceHuCard
        | TasExecutionProfile::DirectPceSixButtonHuCard
        | TasExecutionProfile::DirectPceMultitapHuCard
        | TasExecutionProfile::DirectPceCd
        | TasExecutionProfile::DirectPceMultitapCd => {
            pce::restore_direct_pce_state(backend, profile, &cached_state.bytes)?;
        }
    }
    let restored = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    if backend.frame_count() != cached_state.frame_count
        || super::tas_state_digest(profile, &restored) != cached_state.sha256
    {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok(())
}

pub(super) fn validate_gb_input(input: TasInputFrame) -> Result<(), ()> {
    if input.p1_buttons & !0x0F != 0
        || input.p1_dpad & !0x0F != 0
        || input.p2_buttons != 0
        || input.p2_dpad != 0
        || input.coleco != [crate::tas_project::TasColecoControllerInput::default(); 2]
        || input.zapper.enabled
        || input.zapper.trigger
        || input.zapper.hit
        || input.zapper.screen_pos.is_some()
    {
        return Err(());
    }
    Ok(())
}

pub(super) fn replay_frame(input: TasInputFrame) -> ReplayJoypadFrame {
    ReplayJoypadFrame {
        buttons: input.p1_buttons,
        dpad: input.p1_dpad,
        buttons_p2: input.p2_buttons,
        dpad_p2: input.p2_dpad,
        buttons_p3: input.p3_buttons,
        dpad_p3: input.p3_dpad,
        buttons_p4: input.p4_buttons,
        dpad_p4: input.p4_dpad,
        buttons_p5: input.p5_buttons,
        dpad_p5: input.p5_dpad,
        zapper: input.zapper,
        host_tilt: (
            f32::from_bits(input.tilt_x_bits),
            f32::from_bits(input.tilt_y_bits),
        ),
        ..ReplayJoypadFrame::default()
    }
}

fn validate_gb_inputs(inputs: &[TasInputFrame]) -> Result<(), ()> {
    inputs.iter().copied().try_for_each(validate_gb_input)
}

pub(super) fn apply_coleco_input(backend: &mut EmuBackend, input: TasInputFrame) -> Result<(), ()> {
    validate_coleco_input(input)?;
    backend.apply_coleco_tas_input(input.coleco).map_err(|_| ())
}

fn validate_coleco_input(input: TasInputFrame) -> Result<(), ()> {
    if input.p1_buttons != 0
        || input.p1_dpad != 0
        || input.p2_buttons != 0
        || input.p2_dpad != 0
        || input.zapper != Default::default()
    {
        return Err(());
    }
    Ok(())
}

fn validate_coleco_inputs(inputs: &[TasInputFrame]) -> Result<(), ()> {
    inputs.iter().copied().try_for_each(validate_coleco_input)
}

pub(super) fn step_coleco_inputs(
    backend: &mut EmuBackend,
    inputs: &[TasInputFrame],
    runtime_fault: &mut WorkerRuntimeFault,
) -> Result<usize, Rejected> {
    let mut advanced = 0;
    for input in inputs {
        apply_coleco_input(backend, *input).map_err(|_| Rejected::InvalidInput)?;
        let frame_count = backend.frame_count();
        backend.step_frame();
        if runtime_fault.latch(backend.take_runtime_fault()) {
            break;
        }
        if backend.frame_count() == frame_count {
            break;
        }
        advanced += 1;
        if backend.is_suspended() {
            break;
        }
    }
    if !runtime_fault.can_step() {
        return Err(Rejected::RuntimeFault);
    }
    Ok(advanced)
}

pub(super) fn restore_direct_coleco_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<(), Rejected> {
    validate_direct_coleco_start_state(backend, state)?;
    let projection = crate::emu_backend::loader::validate_direct_coleco_tas_state(backend, state)
        .map_err(|_| Rejected::StartStateRestoreFailed)?;
    if backend.frame_count() != projection.frame_count
        || backend.framebuffer() != projection.framebuffer.as_ref()
    {
        return Err(Rejected::StateFrameMismatch);
    }
    crate::emu_backend::loader::validate_direct_coleco_tas_execution_runtime(backend, false)
        .map_err(|_| Rejected::InvalidStartState)
}

pub(super) fn capture_direct_coleco_candidate(
    backend: &EmuBackend,
    expected_frame: u64,
) -> Result<(u64, TasDigest), Rejected> {
    crate::emu_backend::loader::validate_direct_coleco_tas_execution_runtime(backend, false)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    validate_direct_coleco_start_state(backend, &state)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok((frame_count, TasDigest::from_bytes(&state)))
}

#[cfg(test)]
mod tests;
