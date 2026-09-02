use crate::emu_backend::EmuBackend;
use crate::emu_thread::{
    EmuThread, TasExecutionProfile, TasExecutionRejectedReason as Rejected, TasExecutionRequest,
    TasInputFrame, TasPersistenceContract, WorkerRuntimeFault,
};
use crate::tas_project::{MAX_EDITOR_SEEK_EXECUTION_FRAMES, MAX_START_STATE_BYTES, TasDigest};

use super::{TasExecutionResult, replay_frame};

pub(in crate::emu_thread::emu_loop::tas_control) fn execute_direct_ws_tas(
    backend: &mut EmuBackend,
    runtime_fault: &mut WorkerRuntimeFault,
    request: &TasExecutionRequest,
    persistence: TasPersistenceContract,
) -> Result<TasExecutionResult, Rejected> {
    validate_ws_inputs(&request.input_prefix).map_err(|_| Rejected::InvalidInput)?;
    let transaction_frames =
        u64::try_from(request.input_prefix.len()).map_err(|_| Rejected::FrameLimitExceeded)?;
    if transaction_frames > MAX_EDITOR_SEEK_EXECUTION_FRAMES {
        return Err(Rejected::FrameLimitExceeded);
    }
    if request.start_state_bytes.len() > MAX_START_STATE_BYTES {
        return Err(Rejected::StartStateTooLarge);
    }
    restore_direct_ws_state(backend, &request.start_state_bytes, persistence)?;
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
    capture_direct_ws_candidate(backend, expected_frame, persistence).map(
        |(frame_count, state_sha256)| TasExecutionResult {
            profile: TasExecutionProfile::DirectWsCartridge,
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

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_ws_input(
    input: TasInputFrame,
) -> Result<(), ()> {
    if input.p1_buttons & !0xFB != 0
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

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_ws_inputs(
    inputs: &[TasInputFrame],
) -> Result<(), ()> {
    inputs.iter().copied().try_for_each(validate_ws_input)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_direct_ws_start_state(
    backend: &EmuBackend,
    state: &[u8],
    persistence: TasPersistenceContract,
) -> Result<(), Rejected> {
    validate_direct_ws_profile_runtime(backend, persistence)
        .map_err(|_| Rejected::InvalidStartState)?;
    let ws = backend.ws().ok_or(Rejected::InvalidStartState)?;
    let inspection =
        zeff_ws_core::save_state::inspect_current_native_wonder_swan_tas_state(&ws.emu, state)
            .map_err(|_| Rejected::InvalidStartState)?;
    let provenance = ws
        .tas_load_provenance()
        .ok_or(Rejected::InvalidStartState)?;
    if inspection.rom_sha256 != ws.emu.rom_hash()
        || inspection.rom_len != provenance.load.raw_source_media_len
        || inspection.rom_footer.rom_size.declared_bytes != Some(inspection.rom_len)
        || !inspection.rom_footer.checksum_valid
        || provenance.load.source_system != Some(inspection.minimum_system)
        || inspection.save_kind != ws.emu.footer().save_kind
        || inspection.save_ram_kind != backend.save_ram_kind()
        || inspection.cartridge_save_len != backend.save_ram_kind().size()
        || inspection.rtc_present != persistence.is_rtc_battery()
        || !inspection.uart.is_disconnected()
    {
        return Err(Rejected::InvalidStartState);
    }
    match persistence {
        TasPersistenceContract::Absent | TasPersistenceContract::WsBattery { .. } => {}
        TasPersistenceContract::WsRtcBattery { byte_len, .. }
            if inspection.save_kind.size() as u64 + 24 == byte_len => {}
        _ => return Err(Rejected::InvalidStartState),
    }
    Ok(())
}

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_direct_ws_profile_runtime(
    backend: &EmuBackend,
    persistence: TasPersistenceContract,
) -> anyhow::Result<()> {
    match persistence {
        TasPersistenceContract::Absent => {
            crate::emu_backend::loader::validate_direct_ws_tas_execution_runtime(backend, false)?;
        }
        TasPersistenceContract::WsBattery {
            save_kind,
            byte_len,
            ..
        } => {
            let inspection =
                crate::emu_backend::loader::validate_direct_ws_tas_private_execution_runtime(
                    backend, false,
                )?;
            let bytes = backend
                .ws_tas_battery_bytes()
                .ok_or_else(|| anyhow::anyhow!("WonderSwan battery state is unavailable"))?;
            anyhow::ensure!(
                !inspection.rtc_present
                    && inspection.save_kind == save_kind
                    && bytes.len() as u64 == byte_len
            );
        }
        TasPersistenceContract::WsRtcBattery {
            save_kind,
            byte_len,
            ..
        } => {
            let inspection =
                crate::emu_backend::loader::validate_direct_ws_tas_private_execution_runtime(
                    backend, false,
                )?;
            let bytes = backend
                .ws_tas_rtc_battery_bytes()
                .ok_or_else(|| anyhow::anyhow!("WonderSwan RTC persistence is unavailable"))?;
            anyhow::ensure!(
                inspection.rtc_present
                    && inspection.save_kind == save_kind
                    && bytes.len() as u64 == byte_len
            );
        }
        _ => anyhow::bail!("persistence contract does not match the WonderSwan profile"),
    }
    Ok(())
}

pub(in crate::emu_thread::emu_loop::tas_control) fn restore_direct_ws_state(
    backend: &mut EmuBackend,
    state: &[u8],
    persistence: TasPersistenceContract,
) -> Result<(), Rejected> {
    validate_direct_ws_start_state(backend, state, persistence)?;
    let projection = if persistence != TasPersistenceContract::Absent {
        crate::emu_backend::loader::validate_direct_ws_tas_private_state(backend, state)
    } else {
        crate::emu_backend::loader::validate_direct_ws_tas_state(backend, state)
    }
    .map_err(|_| Rejected::StartStateRestoreFailed)?;
    if backend.frame_count() != projection.frame_count
        || backend.framebuffer() != projection.framebuffer.as_ref()
    {
        return Err(Rejected::StateFrameMismatch);
    }
    validate_direct_ws_profile_runtime(backend, persistence)
        .map(|_| ())
        .map_err(|_| Rejected::InvalidStartState)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn capture_direct_ws_candidate(
    backend: &EmuBackend,
    expected_frame: u64,
    persistence: TasPersistenceContract,
) -> Result<(u64, TasDigest), Rejected> {
    validate_direct_ws_profile_runtime(backend, persistence)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    validate_direct_ws_start_state(backend, &state, persistence)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok((frame_count, TasDigest::from_bytes(&state)))
}
