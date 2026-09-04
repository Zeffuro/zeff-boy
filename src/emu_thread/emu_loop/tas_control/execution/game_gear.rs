use crate::emu_backend::EmuBackend;
use crate::emu_thread::{
    EmuThread, TasExecutionProfile, TasExecutionRejectedReason as Rejected, TasExecutionRequest,
    TasInputFrame, WorkerRuntimeFault,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, MAX_START_STATE_BYTES, TasDigest};

use super::{TasExecutionResult, replay_frame};

pub(in crate::emu_thread::emu_loop::tas_control) fn execute_direct_game_gear_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: &TasExecutionRequest,
) -> Result<TasExecutionResult, Rejected> {
    validate_game_gear_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
    let transaction_frames =
        u64::try_from(request.input_prefix.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    if transaction_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
        return Err(Rejected::FrameLimitExceeded);
    }
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    restore_direct_game_gear_state(backend, &request.start_state_bytes)?;
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
    capture_direct_game_gear_candidate(backend, expected_frame).map(
        |(frame_count, state_sha256)| TasExecutionResult {
            profile: TasExecutionProfile::DirectGameGearCartridge,
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

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_game_gear_input(
    input: TasInputFrame,
) -> Result<(), ()> {
    if input.p1_buttons & !0x0B != 0
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

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_game_gear_inputs(
    inputs: &[TasInputFrame],
) -> Result<(), ()> {
    inputs
        .iter()
        .copied()
        .try_for_each(validate_game_gear_input)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_direct_game_gear_start_state(
    backend: &EmuBackend,
    state: &[u8],
) -> Result<(), Rejected> {
    crate::emu_backend::loader::validate_direct_game_gear_tas_private_execution_runtime(
        backend, false,
    )
    .map_err(|_| Rejected::InvalidStartState)?;
    let sega8 = backend.sega8().ok_or(Rejected::InvalidStartState)?;
    let inspection =
        zeff_sega8_core::save_state::inspect_current_native_game_gear_tas_state(&sega8.emu, state)
            .map_err(|_| Rejected::InvalidStartState)?;
    if inspection.rom_sha256 != sega8.emu.rom_hash()
        || inspection.mapper_kind != zeff_sega8_core::hardware::cartridge::Sega8MapperKind::Sega
        || inspection.save_ram_kind != backend.save_ram_kind()
        || inspection.video_standard != zeff_sega8_core::hardware::timing::Sega8VideoStandard::Ntsc
        || inspection.console_region != zeff_sega8_core::hardware::region::Sega8Region::Export
        || inspection.boot_rom_enabled
        || inspection.controller_raw[1] != 0xFF
        || inspection.controller_raw[0] | 0x3F != 0xFF
        || inspection.serial.peer_present
    {
        return Err(Rejected::InvalidStartState);
    }
    Ok(())
}

pub(in crate::emu_thread::emu_loop::tas_control) fn restore_direct_game_gear_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<(), Rejected> {
    validate_direct_game_gear_start_state(backend, state)?;
    let projection =
        crate::emu_backend::loader::restore_direct_game_gear_tas_private_execution_state(
            backend, state,
        )
        .map_err(|_| Rejected::StartStateRestoreFailed)?;
    if backend.frame_count() != projection.frame_count
        || backend.framebuffer() != projection.framebuffer.as_ref()
    {
        return Err(Rejected::StateFrameMismatch);
    }
    crate::emu_backend::loader::validate_direct_game_gear_tas_private_execution_runtime(
        backend, false,
    )
    .map_err(|_| Rejected::InvalidStartState)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn capture_direct_game_gear_candidate(
    backend: &EmuBackend,
    expected_frame: u64,
) -> Result<(u64, TasDigest), Rejected> {
    crate::emu_backend::loader::validate_direct_game_gear_tas_private_execution_runtime(
        backend, false,
    )
    .map_err(|_| Rejected::StateCaptureFailed)?;
    let state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    validate_direct_game_gear_start_state(backend, &state)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok((frame_count, TasDigest::from_bytes(&state)))
}
