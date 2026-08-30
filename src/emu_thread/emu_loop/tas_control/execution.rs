use zeff_emu_common::replay::ReplayJoypadFrame;

use crate::emu_backend::{ActiveSystem, EmuBackend};
use crate::emu_thread::{
    EmuThread, TasExecutionProfile, TasExecutionRejectedReason as Rejected, TasExecutionRequest,
    TasInputFrame, WorkerRuntimeFault,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, MAX_START_STATE_BYTES, TasDigest};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct TasExecutionResult {
    pub(super) profile: TasExecutionProfile,
    pub(super) frame_count: u64,
    pub(super) state_sha256: TasDigest,
    pub(super) executed_project_frames: u64,
    pub(super) segment_id: u64,
    pub(super) segment_frame_count: u64,
    pub(super) last_advance_id: u64,
}

pub(super) fn execute_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: TasExecutionRequest,
) -> Result<TasExecutionResult, Rejected> {
    match request.profile {
        TasExecutionProfile::DirectNesCartridge => {
            execute_direct_nes_tas(backend, runtime_fault, request)
        }
        TasExecutionProfile::DirectGbRomOnlyDmg => {
            execute_direct_gb_tas(backend, runtime_fault, request)
        }
    }
}

fn execute_direct_nes_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: TasExecutionRequest,
) -> Result<TasExecutionResult, Rejected> {
    if request.input_prefix.is_empty() {
        return Err(Rejected::EmptyInputPrefix);
    }
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
        .load_state_from_bytes(request.start_state_bytes)
        .map_err(|_| Rejected::StartStateRestoreFailed)?;
    if backend.system() != ActiveSystem::Nes
        || backend.nes_has_standard_controller_topology() != Some(true)
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
    let frames: Vec<_> = request.input_prefix.into_iter().map(replay_frame).collect();
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
        profile: TasExecutionProfile::DirectNesCartridge,
        frame_count,
        state_sha256: TasDigest::from_bytes(&state_bytes),
        executed_project_frames: transaction_frames,
        segment_id: 1,
        segment_frame_count: transaction_frames,
        last_advance_id: 0,
    })
}

fn execute_direct_gb_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: TasExecutionRequest,
) -> Result<TasExecutionResult, Rejected> {
    validate_gb_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
    if request.input_prefix.is_empty() {
        return Err(Rejected::EmptyInputPrefix);
    }
    let transaction_frames =
        u64::try_from(request.input_prefix.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    if transaction_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
        return Err(Rejected::FrameLimitExceeded);
    }
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    restore_direct_gb_state(backend, &request.start_state_bytes)?;
    let start_frame = backend.frame_count();
    let expected_frame = start_frame
        .checked_add(transaction_frames)
        .ok_or(Rejected::FrameCountOverflow)?;
    let frames: Vec<_> = request.input_prefix.into_iter().map(replay_frame).collect();
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
    capture_direct_gb_candidate(backend, expected_frame).map(|(frame_count, state_sha256)| {
        TasExecutionResult {
            profile: TasExecutionProfile::DirectGbRomOnlyDmg,
            frame_count,
            state_sha256,
            executed_project_frames: transaction_frames,
            segment_id: 1,
            segment_frame_count: transaction_frames,
            last_advance_id: 0,
        }
    })
}

pub(super) fn replay_frame(input: TasInputFrame) -> ReplayJoypadFrame {
    ReplayJoypadFrame {
        buttons: input.p1_buttons,
        dpad: input.p1_dpad,
        buttons_p2: input.p2_buttons,
        dpad_p2: input.p2_dpad,
        ..ReplayJoypadFrame::default()
    }
}

pub(super) fn validate_gb_input(input: TasInputFrame) -> Result<(), ()> {
    if input.p1_buttons & !0x0F != 0
        || input.p1_dpad & !0x0F != 0
        || input.p2_buttons != 0
        || input.p2_dpad != 0
    {
        return Err(());
    }
    Ok(())
}

fn validate_gb_inputs(inputs: &[TasInputFrame]) -> Result<(), ()> {
    inputs.iter().copied().try_for_each(validate_gb_input)
}

pub(super) fn restore_direct_gb_state(
    backend: &mut EmuBackend,
    state: &[u8],
) -> Result<(), Rejected> {
    crate::emu_backend::loader::validate_direct_gb_tas_runtime(backend, false)
        .map_err(|_| Rejected::InvalidStartState)?;
    let inspection = match backend {
        EmuBackend::Gb(gb) => {
            zeff_gb_core::save_state::inspect_current_native_tas_state(&gb.emu, state)
                .map_err(|_| Rejected::StartStateRestoreFailed)?
        }
        _ => return Err(Rejected::InvalidStartState),
    };
    if inspection.hardware_mode_preference
        != zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceDmg
        || inspection.hardware_mode
            != zeff_gb_core::hardware::types::hardware_mode::HardwareMode::DMG
        || inspection.boot_rom_enabled
        || inspection.serial_device != zeff_gb_core::hardware::GameBoySerialDevice::Disconnected
    {
        return Err(Rejected::InvalidStartState);
    }
    let projection = match backend {
        EmuBackend::Gb(gb) => {
            zeff_gb_core::save_state::validate_and_load_current_native_tas_state(&mut gb.emu, state)
                .map_err(|_| Rejected::StartStateRestoreFailed)?
        }
        _ => return Err(Rejected::InvalidStartState),
    };
    if backend.frame_count() != inspection.projection.frame_count
        || backend.framebuffer() != inspection.projection.lcd_framebuffer.as_ref()
        || projection != inspection.projection
    {
        return Err(Rejected::StateFrameMismatch);
    }
    crate::emu_backend::loader::validate_direct_gb_tas_runtime(backend, false)
        .map_err(|_| Rejected::InvalidStartState)
}

pub(super) fn capture_direct_gb_candidate(
    backend: &EmuBackend,
    expected_frame: u64,
) -> Result<(u64, TasDigest), Rejected> {
    crate::emu_backend::loader::validate_direct_gb_tas_runtime(backend, false)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    crate::emu_backend::loader::validate_direct_gb_tas_state(&state)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok((frame_count, TasDigest::from_bytes(&state)))
}
