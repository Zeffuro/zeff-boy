use crate::emu_thread::{
    TasControlCommitRejectedReason, TasControlRollbackRejectedReason, TasExecutionProfile,
};
use crate::tas_project::TasDigest;

use super::execution::TasExecutionResult;
use super::{TasControlCheckpoint, TasRestoredCheckpoint, execution};

pub(super) fn restore_backend_checkpoint(
    backend: &mut crate::emu_backend::EmuBackend,
    checkpoint: &TasControlCheckpoint,
    persistence: crate::emu_thread::TasPersistenceContract,
) -> Result<TasRestoredCheckpoint, TasControlRollbackRejectedReason> {
    match checkpoint.profile {
        TasExecutionProfile::DirectNesCartridge => backend
            .load_state_from_bytes(checkpoint.state_bytes.clone())
            .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?,
        TasExecutionProfile::DirectFdsDisk => {
            execution::fds::restore_direct_fds_state(backend, &checkpoint.state_bytes)
                .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?;
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            execution::gb::restore_direct_gb_state(
                backend,
                &checkpoint.state_bytes,
                checkpoint.profile,
                persistence,
            )
            .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?;
        }
        TasExecutionProfile::DirectColecoCartridge => {
            execution::restore_direct_coleco_state(backend, &checkpoint.state_bytes)
                .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?;
        }
        TasExecutionProfile::DirectSmsCartridge => {
            execution::sms::restore_direct_sms_state(backend, &checkpoint.state_bytes)
                .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?;
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            execution::game_gear::restore_direct_game_gear_state(backend, &checkpoint.state_bytes)
                .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?;
        }
        TasExecutionProfile::DirectGbaCartridge => {
            execution::gba::restore_direct_gba_state(backend, &checkpoint.state_bytes, persistence)
                .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?;
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            execution::sg1000::restore_direct_sg1000_state(backend, &checkpoint.state_bytes)
                .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?;
        }
        TasExecutionProfile::DirectWsCartridge => {
            execution::ws::restore_direct_ws_state(backend, &checkpoint.state_bytes, persistence)
                .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?;
        }
        TasExecutionProfile::DirectPceHuCard | TasExecutionProfile::DirectPceSixButtonHuCard => {
            execution::pce::restore_direct_pce_state(
                backend,
                checkpoint.profile,
                &checkpoint.state_bytes,
            )
            .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?;
        }
        TasExecutionProfile::DirectPceCd => {
            execution::pce::restore_direct_pce_cd_state(backend, &checkpoint.state_bytes)
                .map_err(|_| TasControlRollbackRejectedReason::RestoreFailed)?;
        }
    }
    let state_bytes = backend
        .encode_state_bytes()
        .map_err(|_| TasControlRollbackRejectedReason::StateVerificationUnavailable)?;
    Ok(TasRestoredCheckpoint {
        state_sha256: TasDigest::from_bytes(&state_bytes),
        frame_count: backend.frame_count(),
    })
}

pub(super) fn verify_tas_execution_candidate(
    backend: &crate::emu_backend::EmuBackend,
    candidate: TasExecutionResult,
    persistence: crate::emu_thread::TasPersistenceContract,
) -> Result<(), TasControlCommitRejectedReason> {
    validate_runtime(backend, candidate.profile, persistence)?;
    let state_bytes = backend
        .encode_state_bytes()
        .map_err(|_| TasControlCommitRejectedReason::StateVerificationUnavailable)?;
    if TasDigest::from_bytes(&state_bytes) != candidate.state_sha256 {
        return Err(TasControlCommitRejectedReason::CandidateStateDigestMismatch);
    }
    if backend.frame_count() != candidate.frame_count {
        return Err(TasControlCommitRejectedReason::CandidateFrameCountMismatch);
    }
    validate_state(backend, candidate.profile, persistence, &state_bytes)
}

fn validate_runtime(
    backend: &crate::emu_backend::EmuBackend,
    profile: TasExecutionProfile,
    persistence: crate::emu_thread::TasPersistenceContract,
) -> Result<(), TasControlCommitRejectedReason> {
    let invalid = |_| TasControlCommitRejectedReason::StateVerificationUnavailable;
    match profile {
        TasExecutionProfile::DirectGbCartridgeDmg => match persistence {
            crate::emu_thread::TasPersistenceContract::Absent => {
                crate::emu_backend::loader::validate_direct_gb_tas_runtime(backend, false)
                    .map_err(invalid)?;
            }
            crate::emu_thread::TasPersistenceContract::GbBattery { .. } => {
                crate::emu_backend::loader::validate_direct_gb_tas_runtime_with_project_sram(
                    backend, false,
                )
                .map_err(invalid)?;
            }
            crate::emu_thread::TasPersistenceContract::GbRtcBattery { .. } => {
                crate::emu_backend::loader::validate_direct_gb_tas_runtime_with_project_rtc(
                    backend, false,
                )
                .map_err(invalid)?;
            }
            crate::emu_thread::TasPersistenceContract::NesBattery { .. }
            | crate::emu_thread::TasPersistenceContract::GbaBattery { .. }
            | crate::emu_thread::TasPersistenceContract::GbaRtcBattery { .. }
            | crate::emu_thread::TasPersistenceContract::GameGearBattery8KiB { .. }
            | crate::emu_thread::TasPersistenceContract::WsBattery { .. }
            | crate::emu_thread::TasPersistenceContract::WsRtcBattery { .. } => {
                return Err(TasControlCommitRejectedReason::StateVerificationUnavailable);
            }
        },
        TasExecutionProfile::DirectGbCartridgeCgb => match persistence {
            crate::emu_thread::TasPersistenceContract::Absent => {
                crate::emu_backend::loader::validate_direct_gbc_tas_runtime(backend, false)
                    .map_err(invalid)?;
            }
            crate::emu_thread::TasPersistenceContract::GbBattery { .. } => {
                crate::emu_backend::loader::validate_direct_gbc_tas_runtime_with_project_sram(
                    backend, false,
                )
                .map_err(invalid)?;
            }
            crate::emu_thread::TasPersistenceContract::GbRtcBattery { .. } => {
                crate::emu_backend::loader::validate_direct_gbc_tas_runtime_with_project_rtc(
                    backend, false,
                )
                .map_err(invalid)?;
            }
            crate::emu_thread::TasPersistenceContract::NesBattery { .. }
            | crate::emu_thread::TasPersistenceContract::GbaBattery { .. }
            | crate::emu_thread::TasPersistenceContract::GbaRtcBattery { .. }
            | crate::emu_thread::TasPersistenceContract::GameGearBattery8KiB { .. }
            | crate::emu_thread::TasPersistenceContract::WsBattery { .. }
            | crate::emu_thread::TasPersistenceContract::WsRtcBattery { .. } => {
                return Err(TasControlCommitRejectedReason::StateVerificationUnavailable);
            }
        },
        TasExecutionProfile::DirectColecoCartridge => {
            crate::emu_backend::loader::validate_direct_coleco_tas_execution_runtime(
                backend, false,
            )
            .map_err(invalid)?;
        }
        TasExecutionProfile::DirectSmsCartridge => {
            crate::emu_backend::loader::validate_direct_sms_tas_execution_runtime(backend, false)
                .map_err(invalid)?;
        }
        TasExecutionProfile::DirectGameGearCartridge => match persistence {
            crate::emu_thread::TasPersistenceContract::Absent => {
                crate::emu_backend::loader::validate_direct_game_gear_tas_execution_runtime(
                    backend, false,
                )
                .map_err(invalid)?;
            }
            crate::emu_thread::TasPersistenceContract::GameGearBattery8KiB { byte_len, .. } => {
                crate::emu_backend::loader::validate_direct_game_gear_tas_private_execution_runtime(
                    backend, false,
                )
                .map_err(invalid)?;
                let bytes = backend
                    .game_gear_tas_battery_bytes()
                    .ok_or(TasControlCommitRejectedReason::StateVerificationUnavailable)?;
                if byte_len != 8 * 1024 || bytes.len() as u64 != byte_len {
                    return Err(TasControlCommitRejectedReason::StateVerificationUnavailable);
                }
            }
            _ => return Err(TasControlCommitRejectedReason::StateVerificationUnavailable),
        },
        TasExecutionProfile::DirectGbaCartridge => {
            execution::gba::validate_direct_gba_profile_runtime(backend, persistence)
                .map_err(invalid)?;
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            crate::emu_backend::loader::validate_direct_sg1000_tas_execution_runtime(
                backend, false,
            )
            .map_err(invalid)?;
        }
        TasExecutionProfile::DirectWsCartridge => {
            execution::ws::validate_direct_ws_profile_runtime(backend, persistence)
                .map_err(invalid)?;
        }
        TasExecutionProfile::DirectPceHuCard | TasExecutionProfile::DirectPceSixButtonHuCard => {
            if profile == TasExecutionProfile::DirectPceHuCard {
                crate::emu_backend::loader::validate_direct_pce_tas_execution_runtime(
                    backend, false,
                )
            } else {
                crate::emu_backend::loader::validate_direct_pce_six_button_tas_execution_runtime(
                    backend, false,
                )
            }
            .map_err(invalid)?;
        }
        TasExecutionProfile::DirectPceCd => {
            if !matches!(
                persistence,
                crate::emu_thread::TasPersistenceContract::Absent
            ) {
                return Err(TasControlCommitRejectedReason::StateVerificationUnavailable);
            }
            crate::emu_backend::loader::validate_direct_pce_cd_tas_execution_runtime(
                backend, false,
            )
            .map_err(invalid)?;
        }
        TasExecutionProfile::DirectNesCartridge => {}
        TasExecutionProfile::DirectFdsDisk => {
            if !matches!(
                persistence,
                crate::emu_thread::TasPersistenceContract::Absent
            ) {
                return Err(TasControlCommitRejectedReason::StateVerificationUnavailable);
            }
            crate::emu_backend::loader::validate_fds_tas_execution_runtime(backend, false)
                .map_err(invalid)?;
        }
    }
    Ok(())
}

fn validate_state(
    backend: &crate::emu_backend::EmuBackend,
    profile: TasExecutionProfile,
    persistence: crate::emu_thread::TasPersistenceContract,
    state_bytes: &[u8],
) -> Result<(), TasControlCommitRejectedReason> {
    let invalid = || TasControlCommitRejectedReason::StateVerificationUnavailable;
    match profile {
        TasExecutionProfile::DirectGbCartridgeDmg => {
            crate::emu_backend::loader::validate_direct_gb_tas_state(state_bytes)
                .map_err(|_| invalid())?;
        }
        TasExecutionProfile::DirectGbCartridgeCgb => match persistence {
            crate::emu_thread::TasPersistenceContract::Absent => {
                crate::emu_backend::loader::validate_direct_gbc_state_for_backend(
                    backend,
                    state_bytes,
                    false,
                )
                .map_err(|_| invalid())?;
            }
            crate::emu_thread::TasPersistenceContract::GbBattery { .. } => {
                crate::emu_backend::loader::validate_direct_gbc_state_for_backend_with_project_sram(
                        backend,
                        state_bytes,
                        false,
                    )
                    .map_err(|_| invalid())?;
            }
            crate::emu_thread::TasPersistenceContract::GbRtcBattery { .. } => {
                crate::emu_backend::loader::validate_direct_gbc_state_for_backend_with_project_rtc(
                    backend,
                    state_bytes,
                    false,
                )
                .map_err(|_| invalid())?;
            }
            crate::emu_thread::TasPersistenceContract::NesBattery { .. }
            | crate::emu_thread::TasPersistenceContract::GbaBattery { .. }
            | crate::emu_thread::TasPersistenceContract::GbaRtcBattery { .. }
            | crate::emu_thread::TasPersistenceContract::GameGearBattery8KiB { .. }
            | crate::emu_thread::TasPersistenceContract::WsBattery { .. }
            | crate::emu_thread::TasPersistenceContract::WsRtcBattery { .. } => {
                return Err(invalid());
            }
        },
        TasExecutionProfile::DirectColecoCartridge => {
            let identity = zeff_coleco_core::save_state::inspect_current_native_tas_state_identity(
                state_bytes,
            )
            .map_err(|_| invalid())?;
            let coleco = backend.coleco().ok_or_else(invalid)?;
            if identity.cartridge_sha256 != coleco.emu.cartridge_hash()
                || identity.bios_sha256 != coleco.emu.bios_hash()
            {
                return Err(invalid());
            }
        }
        TasExecutionProfile::DirectSmsCartridge => {
            let sega8 = backend.sega8().ok_or_else(invalid)?;
            zeff_sega8_core::save_state::inspect_current_native_sms_tas_state(
                &sega8.emu,
                state_bytes,
            )
            .map_err(|_| invalid())?;
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            let sega8 = backend.sega8().ok_or_else(invalid)?;
            let inspection =
                zeff_sega8_core::save_state::inspect_current_native_game_gear_tas_state(
                    &sega8.emu,
                    state_bytes,
                )
                .map_err(|_| invalid())?;
            match persistence {
                crate::emu_thread::TasPersistenceContract::Absent
                    if inspection.save_ram_kind == zeff_emu_common::save_ram::SaveRamKind::None => {
                }
                crate::emu_thread::TasPersistenceContract::GameGearBattery8KiB {
                    byte_len, ..
                } if byte_len == 8 * 1024
                    && inspection.save_ram_kind
                        == zeff_emu_common::save_ram::SaveRamKind::known_battery_backed(
                            8 * 1024,
                        ) => {}
                _ => return Err(invalid()),
            }
        }
        TasExecutionProfile::DirectGbaCartridge => {
            execution::gba::validate_direct_gba_start_state(backend, state_bytes, persistence)
                .map_err(|_| invalid())?;
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            let sega8 = backend.sega8().ok_or_else(invalid)?;
            zeff_sega8_core::save_state::inspect_current_native_sg1000_tas_state(
                &sega8.emu,
                state_bytes,
            )
            .map_err(|_| invalid())?;
        }
        TasExecutionProfile::DirectWsCartridge => {
            execution::ws::validate_direct_ws_start_state(backend, state_bytes, persistence)
                .map_err(|_| invalid())?;
        }
        TasExecutionProfile::DirectPceHuCard | TasExecutionProfile::DirectPceSixButtonHuCard => {
            let pce = backend.pce().ok_or_else(invalid)?;
            pce.inspect_current_native_tas_state(state_bytes)
                .map_err(|_| invalid())?;
        }
        TasExecutionProfile::DirectPceCd => {
            let runtime = crate::emu_backend::loader::validate_direct_pce_cd_tas_execution_runtime(
                backend, false,
            )
            .map_err(|_| invalid())?;
            let pce = backend.pce().ok_or_else(invalid)?;
            pce.inspect_current_native_cd_tas_state_for_profile(
                state_bytes,
                runtime.arcade_card_enabled,
                runtime.memory_base_enabled,
            )
            .map_err(|_| invalid())?;
        }
        TasExecutionProfile::DirectNesCartridge => {}
        TasExecutionProfile::DirectFdsDisk => {
            crate::emu_backend::loader::validate_current_nes_start_state(state_bytes)
                .map_err(|_| invalid())?;
        }
    }
    Ok(())
}
