use std::path::{Path, PathBuf};

use anyhow::{Result, bail, ensure};

use super::*;

pub(crate) fn select_private_tas_execution_loader(
    source_path: PathBuf,
    system: ActiveSystem,
    firmware_search_dirs: Vec<PathBuf>,
) -> Result<PrivateTasExecutionLoader> {
    select_private_tas_execution_loader_with_rom_path(
        source_path,
        None,
        system,
        firmware_search_dirs,
    )
}

pub(crate) fn select_private_tas_execution_loader_with_rom_path(
    source_path: PathBuf,
    rom_path: Option<PathBuf>,
    system: ActiveSystem,
    firmware_search_dirs: Vec<PathBuf>,
) -> Result<PrivateTasExecutionLoader> {
    match system {
        ActiveSystem::Nes if has_extension(&source_path, "fds") => {
            Ok(PrivateTasExecutionLoader::DirectFds(
                DirectFdsTasExecutionLoader::new(source_path, firmware_search_dirs),
            ))
        }
        ActiveSystem::Nes if has_extension(&source_path, "nes") => {
            Ok(PrivateTasExecutionLoader::DirectNes(
                DirectNesTasExecutionLoader::new(source_path, firmware_search_dirs),
            ))
        }
        ActiveSystem::Nes if has_extension(&source_path, "zip") => {
            let selected_fds = rom_path
                .as_deref()
                .is_some_and(|path| has_extension(path, "fds"));
            if selected_fds {
                let loader = DirectFdsTasExecutionLoader::new_zip(
                    source_path,
                    rom_path,
                    firmware_search_dirs,
                );
                loader.create_project()?;
                return Ok(PrivateTasExecutionLoader::DirectFds(loader));
            }
            let loader =
                DirectNesTasExecutionLoader::new_zip(source_path, rom_path, firmware_search_dirs);
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectNes(loader))
        }
        ActiveSystem::GameBoy if has_extension(&source_path, "gb") => {
            Ok(PrivateTasExecutionLoader::DirectGb(
                DirectGbTasExecutionLoader::new(source_path, firmware_search_dirs),
            ))
        }
        ActiveSystem::GameBoy if has_extension(&source_path, "gbc") => {
            Ok(PrivateTasExecutionLoader::DirectGbc(
                DirectGbcTasExecutionLoader::new(source_path, firmware_search_dirs),
            ))
        }
        ActiveSystem::GameBoy if has_extension(&source_path, "zip") => {
            let extension = rom_path
                .as_deref()
                .and_then(Path::extension)
                .and_then(|extension| extension.to_str());
            let gbc = if let Some(extension) = extension {
                extension.eq_ignore_ascii_case("gbc")
            } else {
                let gb_count = crate::rom_archive::inspect_bounded_zip_members(
                    &source_path,
                    "gb",
                    128 * 1024 * 1024,
                    8 * 1024 * 1024,
                )?
                .entries
                .len();
                let gbc_count = crate::rom_archive::inspect_bounded_zip_members(
                    &source_path,
                    "gbc",
                    128 * 1024 * 1024,
                    8 * 1024 * 1024,
                )?
                .entries
                .len();
                ensure!(
                    gb_count + gbc_count == 1,
                    "Game Boy ZIP must contain exactly one .gb or .gbc member unless one is selected explicitly"
                );
                gbc_count == 1
            };
            let loader = if gbc {
                let loader = DirectGbcTasExecutionLoader::new_zip(
                    source_path,
                    rom_path,
                    firmware_search_dirs,
                );
                loader.create_project()?;
                PrivateTasExecutionLoader::DirectGbc(loader)
            } else {
                let loader = DirectGbTasExecutionLoader::new_zip(
                    source_path,
                    rom_path,
                    firmware_search_dirs,
                );
                loader.create_project()?;
                PrivateTasExecutionLoader::DirectGb(loader)
            };
            Ok(loader)
        }
        ActiveSystem::Coleco if has_extension(&source_path, "col") => {
            Ok(PrivateTasExecutionLoader::DirectColeco(
                DirectColecoTasExecutionLoader::new(source_path, firmware_search_dirs),
            ))
        }
        ActiveSystem::Coleco if has_extension(&source_path, "zip") => {
            let loader = DirectColecoTasExecutionLoader::new_zip(
                source_path,
                rom_path,
                firmware_search_dirs,
            );
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectColeco(loader))
        }
        ActiveSystem::MasterSystem if has_extension(&source_path, "sms") => Ok(
            PrivateTasExecutionLoader::DirectSms(DirectSmsTasExecutionLoader::new(source_path)),
        ),
        ActiveSystem::MasterSystem if has_extension(&source_path, "zip") => {
            let loader = DirectSmsTasExecutionLoader::new_zip(source_path, rom_path);
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectSms(loader))
        }
        ActiveSystem::GameGear if has_extension(&source_path, "gg") => {
            let loader = DirectGameGearTasExecutionLoader::new(source_path);
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectGameGear(loader))
        }
        ActiveSystem::GameGear if has_extension(&source_path, "zip") => {
            let loader = DirectGameGearTasExecutionLoader::new_zip(source_path, rom_path, false);
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectGameGear(loader))
        }
        ActiveSystem::GameBoyAdvance if has_extension(&source_path, "gba") => Ok(
            PrivateTasExecutionLoader::DirectGba(DirectGbaTasExecutionLoader::new(source_path)),
        ),
        ActiveSystem::GameBoyAdvance if has_extension(&source_path, "zip") => {
            let loader = DirectGbaTasExecutionLoader::new_zip(source_path, rom_path);
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectGba(loader))
        }
        ActiveSystem::Sg1000
            if has_extension(&source_path, "sg") || has_extension(&source_path, "sc") =>
        {
            Ok(PrivateTasExecutionLoader::DirectSg1000(
                DirectSg1000TasExecutionLoader::new(source_path),
            ))
        }
        ActiveSystem::Sg1000 if has_extension(&source_path, "zip") => {
            let loader = DirectSg1000TasExecutionLoader::new_zip(source_path, rom_path);
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectSg1000(loader))
        }
        ActiveSystem::WonderSwan
            if has_extension(&source_path, "ws") || has_extension(&source_path, "wsc") =>
        {
            Ok(PrivateTasExecutionLoader::DirectWs(
                DirectWsTasExecutionLoader::new(source_path),
            ))
        }
        ActiveSystem::WonderSwan if has_extension(&source_path, "zip") => {
            let loader = DirectWsTasExecutionLoader::new_zip(source_path, rom_path);
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectWs(loader))
        }
        ActiveSystem::Pce if has_extension(&source_path, "pce") => {
            let loader = DirectPceTasExecutionLoader::new(source_path);
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectPce(loader))
        }
        ActiveSystem::Pce
            if has_extension(&source_path, "cue")
                || has_extension(&source_path, "chd")
                || has_extension(&source_path, "iso") =>
        {
            let loader = DirectPceCdTasExecutionLoader::new(source_path, firmware_search_dirs);
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectPceCd(loader))
        }
        ActiveSystem::Pce if has_extension(&source_path, "zip") => {
            let loader = DirectPceTasExecutionLoader::new_zip(source_path, rom_path);
            loader.load_fresh_backend()?;
            Ok(PrivateTasExecutionLoader::DirectPce(loader))
        }
        ActiveSystem::Nes => {
            bail!("NES TAS execution requires a direct cartridge/disk file or selected ZIP member")
        }
        ActiveSystem::GameBoy => {
            bail!("Game Boy TAS execution requires a direct cartridge or selected ZIP member")
        }
        ActiveSystem::Coleco => {
            bail!("ColecoVision TAS execution requires a direct .col file or selected ZIP member")
        }
        ActiveSystem::MasterSystem => {
            bail!("Master System TAS execution requires a direct .sms file or selected ZIP member")
        }
        ActiveSystem::GameGear => {
            bail!("Game Gear TAS execution requires a direct .gg file or selected ZIP member")
        }
        ActiveSystem::GameBoyAdvance => {
            bail!("GBA TAS execution requires a direct .gba file or selected ZIP member")
        }
        ActiveSystem::Sg1000 => {
            bail!("SG-1000 TAS execution requires a direct cartridge or selected ZIP member")
        }
        ActiveSystem::WonderSwan => {
            bail!("WonderSwan TAS execution requires a direct cartridge or selected ZIP member")
        }
        ActiveSystem::Pce => {
            bail!(
                "PC Engine TAS execution requires a supported direct HuCard, direct CUE/CHD/ISO, or selected ZIP member"
            )
        }
    }
}

pub(crate) fn select_private_tas_execution_loader_for_project(
    source_path: PathBuf,
    system: ActiveSystem,
    firmware_search_dirs: Vec<PathBuf>,
    project: &TasProject,
) -> Result<PrivateTasExecutionLoader> {
    if system == ActiveSystem::Pce
        && (has_extension(&source_path, "cue")
            || has_extension(&source_path, "chd")
            || has_extension(&source_path, "iso"))
        && validate_direct_pce_cd_tas_project_identity(project).is_ok()
    {
        let loader = DirectPceCdTasExecutionLoader::new(source_path, firmware_search_dirs.clone());
        loader.load_fresh_backend()?;
        return Ok(PrivateTasExecutionLoader::DirectPceCd(loader));
    }
    if system == ActiveSystem::Pce {
        let profile = direct_pce_tas_project_profile(project)?;
        let loader = match (
            has_extension(&source_path, "pce"),
            has_extension(&source_path, "zip"),
            profile.controller_mode,
        ) {
            (true, false, zeff_pce_core::hardware::PceControllerMode::TwoButton) => {
                DirectPceTasExecutionLoader::new(source_path)
            }
            (true, false, zeff_pce_core::hardware::PceControllerMode::SixButton) => {
                DirectPceTasExecutionLoader::new_six_button(source_path)
            }
            (false, true, _) => {
                DirectPceTasExecutionLoader::new_zip_for_project(source_path, project)?
            }
            _ => {
                bail!("PC Engine TAS execution requires a direct .pce file or selected ZIP member")
            }
        };
        loader.load_fresh_backend()?;
        return Ok(PrivateTasExecutionLoader::DirectPce(loader));
    }
    if system == ActiveSystem::Nes
        && (has_extension(&source_path, "fds") || has_extension(&source_path, "zip"))
        && validate_fds_tas_project_identity(project).is_ok()
    {
        let loader = DirectFdsTasExecutionLoader::new_for_project(
            source_path,
            firmware_search_dirs,
            project,
        )?;
        return Ok(PrivateTasExecutionLoader::DirectFds(loader));
    }
    if system == ActiveSystem::Nes && has_extension(&source_path, "zip") {
        let loader = DirectNesTasExecutionLoader::new_zip_for_project(
            source_path,
            firmware_search_dirs,
            project,
        )?;
        loader.load_fresh_backend()?;
        return Ok(PrivateTasExecutionLoader::DirectNes(loader));
    }
    if system == ActiveSystem::GameBoyAdvance && has_extension(&source_path, "zip") {
        let loader = DirectGbaTasExecutionLoader::new_zip_for_project(source_path, project)?;
        loader.load_fresh_backend()?;
        return Ok(PrivateTasExecutionLoader::DirectGba(loader));
    }
    if system == ActiveSystem::GameBoy && has_extension(&source_path, "zip") {
        if direct_gbc::validate_direct_gbc_tas_project_identity(project).is_ok() {
            let loader = DirectGbcTasExecutionLoader::new_zip_for_project(
                source_path,
                firmware_search_dirs,
                project,
            )?;
            return Ok(PrivateTasExecutionLoader::DirectGbc(loader));
        }
        let loader = DirectGbTasExecutionLoader::new_zip_for_project(
            source_path,
            firmware_search_dirs,
            project,
        )?;
        return Ok(PrivateTasExecutionLoader::DirectGb(loader));
    }
    if system == ActiveSystem::Coleco && has_extension(&source_path, "zip") {
        let loader = DirectColecoTasExecutionLoader::new_zip_for_project(
            source_path,
            firmware_search_dirs,
            project,
        )?;
        loader.load_fresh_backend()?;
        return Ok(PrivateTasExecutionLoader::DirectColeco(loader));
    }
    if system == ActiveSystem::WonderSwan && has_extension(&source_path, "zip") {
        let loader = DirectWsTasExecutionLoader::new_zip_for_project(source_path, project)?;
        loader.load_fresh_backend()?;
        return Ok(PrivateTasExecutionLoader::DirectWs(loader));
    }
    if system == ActiveSystem::MasterSystem && has_extension(&source_path, "zip") {
        let loader = DirectSmsTasExecutionLoader::new_zip_for_project(source_path, project)?;
        loader.load_fresh_backend()?;
        return Ok(PrivateTasExecutionLoader::DirectSms(loader));
    }
    if system == ActiveSystem::GameGear && has_extension(&source_path, "zip") {
        let loader = DirectGameGearTasExecutionLoader::new_zip_for_project(source_path, project)?;
        loader.load_fresh_backend()?;
        return Ok(PrivateTasExecutionLoader::DirectGameGear(loader));
    }
    if system == ActiveSystem::Sg1000 && has_extension(&source_path, "zip") {
        let loader = DirectSg1000TasExecutionLoader::new_zip_for_project(source_path, project)?;
        loader.load_fresh_backend()?;
        return Ok(PrivateTasExecutionLoader::DirectSg1000(loader));
    }
    if system != ActiveSystem::GameGear || !has_extension(&source_path, "gg") {
        return select_private_tas_execution_loader(source_path, system, firmware_search_dirs);
    }
    validate_direct_game_gear_tas_project_identity(project)?;
    let loader = match direct_game_gear::direct_game_gear_tas_board_choice(project.identity())? {
        direct_game_gear::DirectGameGearTasBoardChoice::CataloguedAbsent => {
            DirectGameGearTasExecutionLoader::new(source_path)
        }
        direct_game_gear::DirectGameGearTasBoardChoice::CataloguedBattery8KiB => {
            DirectGameGearTasExecutionLoader::new(source_path)
        }
        direct_game_gear::DirectGameGearTasBoardChoice::ConfirmedNoCartridgeSaveMemory => {
            DirectGameGearTasExecutionLoader::new_with_confirmed_no_cartridge_save_memory(
                source_path,
            )
        }
    };
    loader.load_fresh_backend()?;
    Ok(PrivateTasExecutionLoader::DirectGameGear(loader))
}

pub(crate) fn select_private_tas_execution_attachment(
    source_path: Option<PathBuf>,
    rom_path: Option<PathBuf>,
    system: Option<ActiveSystem>,
    firmware_search_dirs: Vec<PathBuf>,
    project: Option<&TasProject>,
) -> TasEditorExecutionAttachment {
    let (Some(source_path), Some(system)) = (source_path, system) else {
        return TasEditorExecutionAttachment::Unavailable(
            TasEditorExecutionUnavailableReason::NoRunningEmulator,
        );
    };
    let selection = match project {
        Some(project) => select_private_tas_execution_loader_for_project(
            source_path,
            system,
            firmware_search_dirs,
            project,
        ),
        None => select_private_tas_execution_loader_with_rom_path(
            source_path,
            rom_path,
            system,
            firmware_search_dirs,
        ),
    };
    match selection {
        Ok(loader) => TasEditorExecutionAttachment::Available(Box::new(loader)),
        Err(error)
            if matches!(
                system,
                ActiveSystem::Nes
                    | ActiveSystem::GameBoy
                    | ActiveSystem::Coleco
                    | ActiveSystem::MasterSystem
                    | ActiveSystem::GameGear
                    | ActiveSystem::GameBoyAdvance
                    | ActiveSystem::Sg1000
                    | ActiveSystem::WonderSwan
                    | ActiveSystem::Pce
            ) =>
        {
            TasEditorExecutionAttachment::Unavailable(
                TasEditorExecutionUnavailableReason::UnsupportedMedia(error.to_string()),
            )
        }
        Err(_) => TasEditorExecutionAttachment::Unavailable(
            TasEditorExecutionUnavailableReason::UnsupportedSystem(system.to_string()),
        ),
    }
}

pub(crate) fn has_extension(path: &Path, extension: &str) -> bool {
    path.extension()
        .and_then(|candidate| candidate.to_str())
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(extension))
}
