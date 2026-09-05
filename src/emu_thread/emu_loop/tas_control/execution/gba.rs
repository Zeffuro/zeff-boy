use crate::emu_backend::EmuBackend;
use crate::emu_thread::{
    EmuThread, TasExecutionProfile, TasExecutionRejectedReason as Rejected, TasExecutionRequest,
    TasInputFrame, TasPersistenceContract, WorkerRuntimeFault,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, MAX_START_STATE_BYTES, TasDigest};

use super::{TasExecutionResult, replay_frame};

pub(in crate::emu_thread::emu_loop::tas_control) fn execute_direct_gba_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: &TasExecutionRequest,
    persistence: TasPersistenceContract,
    discarded_audio_samples: &mut Vec<f32>,
) -> Result<TasExecutionResult, Rejected> {
    validate_gba_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
    let transaction_frames =
        u64::try_from(request.input_prefix.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    if transaction_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
        return Err(Rejected::FrameLimitExceeded);
    }
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    restore_direct_gba_state(backend, &request.start_state_bytes, persistence)?;
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
    super::discard_audio(backend, discarded_audio_samples);
    capture_direct_gba_candidate(backend, expected_frame, persistence).map(
        |(frame_count, state_sha256)| TasExecutionResult {
            profile: TasExecutionProfile::DirectGbaCartridge,
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

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_gba_input(
    input: TasInputFrame,
) -> Result<(), ()> {
    if input.p1_buttons & !0x3F != 0
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

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_gba_inputs(
    inputs: &[TasInputFrame],
) -> Result<(), ()> {
    inputs.iter().copied().try_for_each(validate_gba_input)
}

pub(in crate::emu_thread::emu_loop) fn validate_direct_gba_start_state(
    backend: &EmuBackend,
    state: &[u8],
    persistence: TasPersistenceContract,
) -> Result<(), Rejected> {
    validate_direct_gba_profile_runtime(backend, persistence)
        .map_err(|_| Rejected::InvalidStartState)?;
    let current =
        crate::emu_backend::gba::validate_direct_gba_tas_private_execution_runtime(backend, false)
            .map_err(|_| Rejected::InvalidStartState)?;
    let gba = backend.gba().ok_or(Rejected::InvalidStartState)?;
    let inspection =
        if gba.emu.sensor_kind() == zeff_gba_core::hardware::cartridge::SensorKind::Tilt {
            zeff_gba_core::save_state::inspect_current_native_gba_tilt_tas_state(&gba.emu, state)
        } else {
            zeff_gba_core::save_state::inspect_current_native_gba_tas_state(&gba.emu, state)
        }
        .map_err(|_| Rejected::InvalidStartState)?;
    if inspection.rom_sha256 != gba.emu.rom_hash()
        || inspection.save_ram_kind != current.save_ram_kind
        || inspection.battery_data.as_ref().map(Vec::len)
            != current.battery_data.as_ref().map(Vec::len)
        || inspection.rtc_present
            != matches!(persistence, TasPersistenceContract::GbaRtcBattery { .. })
        || inspection.rtc_date_time.is_some() != inspection.rtc_present
        || inspection.sensor_kind != current.sensor_kind
        || inspection.tilt_state.is_some() != current.tilt_state.is_some()
        || inspection.external_bios
        || inspection.startup != zeff_gba_core::save_state::GbaTasStartup::InternalPostBoot
        || inspection.sample_rate != crate::emu_backend::gba::DIRECT_GBA_SAMPLE_RATE
    {
        return Err(Rejected::InvalidStartState);
    }
    match persistence {
        TasPersistenceContract::Absent | TasPersistenceContract::GbaBattery { .. } => {}
        TasPersistenceContract::GbaRtcBattery { byte_len, .. }
            if inspection.rtc_persistence_state.as_ref().map(Vec::len) == Some(32)
                && inspection
                    .complete_rtc_persistence
                    .as_ref()
                    .is_some_and(|bytes| bytes.len() as u64 == byte_len) => {}
        _ => return Err(Rejected::InvalidStartState),
    }
    Ok(())
}

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_direct_gba_profile_runtime(
    backend: &EmuBackend,
    persistence: TasPersistenceContract,
) -> anyhow::Result<()> {
    match persistence {
        TasPersistenceContract::Absent => {
            crate::emu_backend::gba::validate_direct_gba_tas_execution_runtime(backend, false)?;
        }
        TasPersistenceContract::GbaBattery { kind, byte_len, .. } => {
            crate::emu_backend::gba::validate_direct_gba_tas_private_execution_runtime(
                backend, false,
            )?;
            let (actual_kind, bytes) = backend
                .gba_tas_battery_component()?
                .ok_or_else(|| anyhow::anyhow!("GBA battery state is unavailable"))?;
            anyhow::ensure!(actual_kind == kind && bytes.len() as u64 == byte_len);
        }
        TasPersistenceContract::GbaRtcBattery { kind, byte_len, .. } => {
            crate::emu_backend::gba::validate_direct_gba_tas_private_execution_runtime(
                backend, false,
            )?;
            let gba = backend
                .gba()
                .ok_or_else(|| anyhow::anyhow!("GBA backend is unavailable"))?;
            let bytes = gba
                .emu
                .dump_complete_rtc_persistence()
                .ok_or_else(|| anyhow::anyhow!("GBA RTC persistence is unavailable"))?;
            anyhow::ensure!(gba.emu.backup_kind() == kind && bytes.len() as u64 == byte_len);
        }
        _ => anyhow::bail!("persistence contract does not match the GBA profile"),
    }
    Ok(())
}

pub(in crate::emu_thread::emu_loop::tas_control) fn restore_direct_gba_state(
    backend: &mut EmuBackend,
    state: &[u8],
    persistence: TasPersistenceContract,
) -> Result<(), Rejected> {
    validate_direct_gba_start_state(backend, state, persistence)?;
    let projection =
        crate::emu_backend::gba::restore_direct_gba_tas_execution_state(backend, state)
            .map_err(|_| Rejected::StartStateRestoreFailed)?;
    if backend.frame_count() != projection.frame_count
        || backend.framebuffer() != projection.framebuffer.as_ref()
    {
        return Err(Rejected::StateFrameMismatch);
    }
    validate_direct_gba_profile_runtime(backend, persistence)
        .map_err(|_| Rejected::InvalidStartState)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn capture_direct_gba_candidate(
    backend: &EmuBackend,
    expected_frame: u64,
    persistence: TasPersistenceContract,
) -> Result<(u64, TasDigest), Rejected> {
    validate_direct_gba_profile_runtime(backend, persistence)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    capture_direct_gba_advanced_candidate(backend, expected_frame)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn capture_direct_gba_advanced_candidate(
    backend: &EmuBackend,
    expected_frame: u64,
) -> Result<(u64, TasDigest), Rejected> {
    let state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok((frame_count, TasDigest::from_bytes(&state)))
}
