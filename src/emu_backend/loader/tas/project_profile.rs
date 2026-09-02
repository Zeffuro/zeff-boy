use anyhow::{Result, bail, ensure};

use super::*;
use crate::emu_thread::TasExecutionProfile;

pub(crate) fn classify_direct_tas_execution_profile(
    project: &TasProject,
) -> Result<TasExecutionProfile> {
    project.validate()?;
    match project.identity().system.as_str() {
        system if system == ActiveSystem::Nes.code() => {
            if validate_fds_tas_project_identity(project).is_ok() {
                return Ok(TasExecutionProfile::DirectFdsDisk);
            }
            validate_direct_nes_tas_project_identity(project)?;
            Ok(TasExecutionProfile::DirectNesCartridge)
        }
        system if system == ActiveSystem::GameBoy.code() => {
            if validate_direct_gb_tas_project_identity(project).is_ok() {
                Ok(TasExecutionProfile::DirectGbCartridgeDmg)
            } else {
                validate_direct_gbc_tas_project_identity(project)?;
                Ok(TasExecutionProfile::DirectGbCartridgeCgb)
            }
        }
        system if system == ActiveSystem::Coleco.code() => {
            validate_direct_coleco_tas_project_identity(project)?;
            Ok(TasExecutionProfile::DirectColecoCartridge)
        }
        system if system == ActiveSystem::MasterSystem.code() => {
            validate_direct_sms_tas_project_identity(project)?;
            Ok(TasExecutionProfile::DirectSmsCartridge)
        }
        system if system == ActiveSystem::GameGear.code() => {
            validate_direct_game_gear_tas_project_identity(project)?;
            Ok(TasExecutionProfile::DirectGameGearCartridge)
        }
        system if system == ActiveSystem::GameBoyAdvance.code() => {
            crate::emu_backend::gba::validate_direct_gba_tas_project_identity(project)?;
            Ok(TasExecutionProfile::DirectGbaCartridge)
        }
        system if system == ActiveSystem::Sg1000.code() => {
            validate_direct_sg1000_tas_project_identity(project)?;
            Ok(TasExecutionProfile::DirectSg1000Cartridge)
        }
        system if system == ActiveSystem::WonderSwan.code() => {
            validate_direct_ws_tas_project_identity(project)?;
            Ok(TasExecutionProfile::DirectWsCartridge)
        }
        system if system == ActiveSystem::Pce.code() => {
            if validate_direct_pce_tas_project_identity(project).is_ok() {
                Ok(
                    if direct_pce_tas_project_profile(project)?.controller_mode
                        == zeff_pce_core::hardware::PceControllerMode::SixButton
                    {
                        TasExecutionProfile::DirectPceSixButtonHuCard
                    } else {
                        TasExecutionProfile::DirectPceHuCard
                    },
                )
            } else {
                validate_direct_pce_cd_tas_project_identity(project)?;
                Ok(TasExecutionProfile::DirectPceCd)
            }
        }
        _ => bail!("the TAS project does not identify a live execution profile"),
    }
}

pub(crate) struct TasProjectRuntimeWitness<'a> {
    pub(crate) profile: TasExecutionProfile,
    pub(crate) source_media_sha256: TasDigest,
    pub(crate) effective_media_sha256: TasDigest,
    pub(crate) current_state_bytes: &'a [u8],
    pub(crate) current_state_sha256: TasDigest,
    pub(crate) determinism_abi: &'a str,
    pub(crate) state_format_compatibility_id: &'a str,
    pub(crate) sync_config_sha256: TasDigest,
}

pub(crate) fn validate_tas_project_witness(
    project: &TasProject,
    branch_id: &str,
    witness: TasProjectRuntimeWitness<'_>,
) -> Result<()> {
    ensure!(
        classify_direct_tas_execution_profile(project)? == witness.profile,
        "worker execution profile does not match the TAS project"
    );
    match witness.profile {
        TasExecutionProfile::DirectNesCartridge => validate_direct_nes_tas_project_witness(
            project,
            branch_id,
            DirectNesTasRuntimeWitness {
                source_media_sha256: witness.source_media_sha256,
                effective_media_sha256: witness.effective_media_sha256,
                current_state_bytes: witness.current_state_bytes,
                current_state_sha256: witness.current_state_sha256,
                determinism_abi: witness.determinism_abi,
                state_format_compatibility_id: witness.state_format_compatibility_id,
                sync_config_sha256: witness.sync_config_sha256,
            },
        ),
        TasExecutionProfile::DirectFdsDisk => {
            validate_fds_tas_project_witness(project, branch_id, witness)
        }
        TasExecutionProfile::DirectGbCartridgeDmg | TasExecutionProfile::DirectGbCartridgeCgb => {
            let gb_witness = DirectGbTasRuntimeWitness {
                source_media_sha256: witness.source_media_sha256,
                effective_media_sha256: witness.effective_media_sha256,
                current_state_bytes: witness.current_state_bytes,
                current_state_sha256: witness.current_state_sha256,
                determinism_abi: witness.determinism_abi,
                state_format_compatibility_id: witness.state_format_compatibility_id,
                sync_config_sha256: witness.sync_config_sha256,
            };
            if witness.profile == TasExecutionProfile::DirectGbCartridgeCgb {
                validate_direct_gbc_tas_project_witness(project, branch_id, gb_witness)
            } else {
                validate_direct_gb_tas_project_witness(project, branch_id, gb_witness)
            }
        }
        TasExecutionProfile::DirectColecoCartridge => {
            validate_direct_coleco_tas_project_witness(project, branch_id, witness)
        }
        TasExecutionProfile::DirectSmsCartridge => {
            validate_direct_sms_tas_project_witness(project, branch_id, witness)
        }
        TasExecutionProfile::DirectGameGearCartridge => {
            validate_direct_game_gear_tas_project_witness(project, branch_id, witness)
        }
        TasExecutionProfile::DirectGbaCartridge => {
            crate::emu_backend::gba::validate_direct_gba_tas_project_witness(
                project, branch_id, witness,
            )
        }
        TasExecutionProfile::DirectSg1000Cartridge => {
            validate_direct_sg1000_tas_project_witness(project, branch_id, witness)
        }
        TasExecutionProfile::DirectWsCartridge => {
            validate_direct_ws_tas_project_witness(project, branch_id, witness)
        }
        TasExecutionProfile::DirectPceHuCard | TasExecutionProfile::DirectPceSixButtonHuCard => {
            validate_direct_pce_tas_project_witness(project, branch_id, witness)
        }
        TasExecutionProfile::DirectPceCd => {
            validate_direct_pce_cd_tas_project_witness(project, branch_id, witness)
        }
    }
}
