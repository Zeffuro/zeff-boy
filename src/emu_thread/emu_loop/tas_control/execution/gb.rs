use crate::emu_backend::EmuBackend;
use crate::emu_thread::{
    TasExecutionProfile, TasExecutionRejectedReason as Rejected, TasPersistenceContract,
};
use crate::tas_project::TasDigest;

pub(super) fn validate_direct_gb_profile_runtime(
    backend: &EmuBackend,
    profile: TasExecutionProfile,
    persistence: TasPersistenceContract,
) -> anyhow::Result<()> {
    match (profile, persistence) {
        (TasExecutionProfile::DirectGbCartridgeDmg, TasPersistenceContract::Absent) => {
            crate::emu_backend::loader::validate_direct_gb_tas_runtime(backend, false)
        }
        (TasExecutionProfile::DirectGbCartridgeDmg, TasPersistenceContract::GbBattery { .. }) => {
            crate::emu_backend::loader::validate_direct_gb_tas_runtime_with_project_sram(
                backend, false,
            )
        }
        (
            TasExecutionProfile::DirectGbCartridgeDmg,
            TasPersistenceContract::GbRtcBattery { .. },
        ) => crate::emu_backend::loader::validate_direct_gb_tas_runtime_with_project_rtc(
            backend, false,
        ),
        (TasExecutionProfile::DirectGbCartridgeCgb, TasPersistenceContract::Absent) => {
            crate::emu_backend::loader::validate_direct_gbc_tas_runtime(backend, false)
        }
        (TasExecutionProfile::DirectGbCartridgeCgb, TasPersistenceContract::GbBattery { .. }) => {
            crate::emu_backend::loader::validate_direct_gbc_tas_runtime_with_project_sram(
                backend, false,
            )
        }
        (
            TasExecutionProfile::DirectGbCartridgeCgb,
            TasPersistenceContract::GbRtcBattery { .. },
        ) => crate::emu_backend::loader::validate_direct_gbc_tas_runtime_with_project_rtc(
            backend, false,
        ),
        _ => anyhow::bail!("persistence contract does not match the Game Boy profile"),
    }
}

pub(super) fn validate_direct_gb_profile_state(
    backend: &EmuBackend,
    state: &[u8],
    profile: TasExecutionProfile,
    persistence: TasPersistenceContract,
    require_normal_speed: bool,
) -> anyhow::Result<()> {
    validate_direct_gb_profile_runtime(backend, profile, persistence)?;
    match (profile, persistence) {
        (TasExecutionProfile::DirectGbCartridgeDmg, _) => {
            crate::emu_backend::loader::validate_direct_gb_tas_state(state)
        }
        (TasExecutionProfile::DirectGbCartridgeCgb, TasPersistenceContract::Absent) => {
            crate::emu_backend::loader::validate_direct_gbc_state_for_backend(
                backend,
                state,
                require_normal_speed,
            )
        }
        (TasExecutionProfile::DirectGbCartridgeCgb, TasPersistenceContract::GbBattery { .. }) => {
            crate::emu_backend::loader::validate_direct_gbc_state_for_backend_with_project_sram(
                backend,
                state,
                require_normal_speed,
            )
        }
        (
            TasExecutionProfile::DirectGbCartridgeCgb,
            TasPersistenceContract::GbRtcBattery { .. },
        ) => crate::emu_backend::loader::validate_direct_gbc_state_for_backend_with_project_rtc(
            backend,
            state,
            require_normal_speed,
        ),
        _ => anyhow::bail!("persistence contract does not match the Game Boy profile"),
    }
}

pub(in crate::emu_thread::emu_loop::tas_control) fn validate_direct_gb_start_state(
    backend: &EmuBackend,
    state: &[u8],
    profile: TasExecutionProfile,
    persistence: TasPersistenceContract,
) -> Result<(), Rejected> {
    validate_direct_gb_profile_state(backend, state, profile, persistence, true)
        .map_err(|_| Rejected::InvalidStartState)?;
    if profile == TasExecutionProfile::DirectGbCartridgeCgb {
        return Ok(());
    }
    let inspection = match backend {
        EmuBackend::Gb(gb) => {
            zeff_gb_core::save_state::inspect_current_native_tas_state(&gb.emu, state)
                .map_err(|_| Rejected::InvalidStartState)?
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
    Ok(())
}

pub(in crate::emu_thread::emu_loop::tas_control) fn restore_direct_gb_state(
    backend: &mut EmuBackend,
    state: &[u8],
    profile: TasExecutionProfile,
    persistence: TasPersistenceContract,
) -> Result<(), Rejected> {
    validate_direct_gb_profile_state(backend, state, profile, persistence, false)
        .map_err(|_| Rejected::InvalidStartState)?;
    let inspection = match backend {
        EmuBackend::Gb(gb) => {
            zeff_gb_core::save_state::inspect_current_native_tas_state(&gb.emu, state)
                .map_err(|_| Rejected::StartStateRestoreFailed)?
        }
        _ => return Err(Rejected::InvalidStartState),
    };
    let state_profile_matches =
        match profile {
            TasExecutionProfile::DirectGbCartridgeDmg => inspection.hardware_mode_preference
                == zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceDmg
                && inspection.hardware_mode
                    == zeff_gb_core::hardware::types::hardware_mode::HardwareMode::DMG,
            TasExecutionProfile::DirectGbCartridgeCgb => inspection.hardware_mode_preference
                == zeff_gb_core::hardware::types::hardware_mode::HardwareModePreference::ForceCgb
                && matches!(
                    inspection.hardware_mode,
                    zeff_gb_core::hardware::types::hardware_mode::HardwareMode::CGBNormal
                        | zeff_gb_core::hardware::types::hardware_mode::HardwareMode::CGBDouble
                ),
            _ => false,
        };
    if !state_profile_matches
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
    validate_direct_gb_profile_runtime(backend, profile, persistence)
        .map_err(|_| Rejected::InvalidStartState)
}

pub(in crate::emu_thread::emu_loop::tas_control) fn capture_direct_gb_candidate(
    backend: &EmuBackend,
    expected_frame: u64,
    profile: TasExecutionProfile,
    persistence: TasPersistenceContract,
) -> Result<(u64, TasDigest), Rejected> {
    validate_direct_gb_profile_runtime(backend, profile, persistence)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let state = backend
        .encode_state_bytes()
        .map_err(|_| Rejected::StateCaptureFailed)?;
    validate_direct_gb_profile_state(backend, &state, profile, persistence, false)
        .map_err(|_| Rejected::StateCaptureFailed)?;
    let frame_count = backend.frame_count();
    if frame_count != expected_frame {
        return Err(Rejected::StateFrameMismatch);
    }
    Ok((frame_count, super::super::tas_state_digest(profile, &state)))
}
