use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use super::{
    BackendLoadConfig, PreparedNativeArchiveBackend, PreparedSevenZipBackend, check_package_cancel,
    finish_prepared_pce_cd_backend, prepare_pce_cd_7z_backend,
};

pub(crate) fn prepare_seven_zip_backend(
    source_path: &Path,
    selected_entry_index: Option<usize>,
    expected_rom_path: Option<&Path>,
    config: &BackendLoadConfig,
    cancel: &AtomicBool,
    progress: &super::super::super::pce_cd_archive::PceCdPackageProgress,
) -> anyhow::Result<PreparedSevenZipBackend> {
    check_package_cancel(cancel)?;
    match super::super::super::pce_cd_archive::inspect_7z_cue_members(
        source_path,
        config.pce_cd_archive_memory_limit_mib,
    )?
    .as_slice()
    {
        [] => match super::super::super::pce_cd_archive::inspect_7z_contents(
            source_path,
            config.pce_cd_archive_memory_limit_mib,
        )? {
            super::super::super::pce_cd_archive::SevenZipContents::Cd { cue_path } => {
                let _ = cue_path;
                Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into())
            }
            super::super::super::pce_cd_archive::SevenZipContents::Roms(entries) => {
                let selected = if let Some(index) = selected_entry_index {
                    entries.iter().find(|entry| entry.index == index)
                } else if let Some(expected) = expected_rom_path {
                    entries.iter().find(|entry| {
                        archive_member_virtual_path(source_path, &entry.name) == expected
                    })
                } else if entries.len() == 1 {
                    entries.first()
                } else {
                    return Ok(PreparedSevenZipBackend::Selection(entries));
                }
                .ok_or(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged)?;
                let (rom_path, bytes, system) =
                    super::super::super::pce_cd_archive::load_7z_rom_entry_with_control(
                        source_path,
                        selected.index,
                        cancel,
                        progress,
                        config.pce_cd_archive_memory_limit_mib,
                    )?;
                check_package_cancel(cancel)?;
                progress.set_phase(
                    super::super::super::pce_cd_archive::PceCdPackageLoadPhase::Building,
                );
                let loaded = super::super::load_backend_from_rom_source(
                    system,
                    source_path,
                    &rom_path,
                    Some(bytes),
                    config.clone(),
                )?;
                check_package_cancel(cancel)?;
                progress.set_phase(
                    super::super::super::pce_cd_archive::PceCdPackageLoadPhase::Complete,
                );
                Ok(PreparedSevenZipBackend::Ready {
                    rom_path,
                    system,
                    loaded,
                })
            }
        },
        [cue_name] => {
            let cue_path = archive_member_virtual_path(source_path, cue_name);
            if selected_entry_index.is_some()
                || expected_rom_path.is_some_and(|expected| expected != cue_path)
            {
                return Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
            }
            let (actual, loaded) =
                prepare_pce_cd_7z_backend(source_path, Some(&cue_path), config, cancel, progress)?;
            Ok(PreparedSevenZipBackend::Ready {
                rom_path: actual,
                system: super::super::super::ActiveSystem::Pce,
                loaded,
            })
        }
        cue_members => {
            let Some(cue_name) = select_archive_cue_member(
                source_path,
                cue_members,
                selected_entry_index,
                expected_rom_path,
            )?
            else {
                return Ok(PreparedSevenZipBackend::Selection(archive_cue_entries(
                    cue_members,
                )));
            };
            let cue_path = archive_member_virtual_path(source_path, cue_name);
            let (actual, loaded_disc, _) = super::super::super::pce_cd_archive::load_7z_selected_cue_with_control_and_archive_identity(
                source_path,
                cue_name,
                cancel,
                progress,
                config.pce_cd_archive_memory_limit_mib,
                config.apply_mods,
            )?;
            if actual != cue_path {
                return Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
            }
            let loaded = finish_prepared_pce_cd_backend(
                source_path,
                &cue_path,
                loaded_disc,
                config,
                cancel,
                progress,
            )?;
            Ok(PreparedSevenZipBackend::Ready {
                rom_path: cue_path,
                system: super::super::super::ActiveSystem::Pce,
                loaded,
            })
        }
    }
}

pub(crate) fn prepare_native_archive_backend(
    source_path: &Path,
    selected_entry_index: Option<usize>,
    expected_rom_path: Option<&Path>,
    config: &BackendLoadConfig,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<super::super::super::pce_cd_archive::PceCdPackageProgress>,
) -> anyhow::Result<PreparedNativeArchiveBackend> {
    if super::path_extension_is(source_path, "7z") {
        return Ok(
            match prepare_seven_zip_backend(
                source_path,
                selected_entry_index,
                expected_rom_path,
                config,
                cancel,
                progress,
            )? {
                PreparedSevenZipBackend::Ready {
                    rom_path,
                    system,
                    loaded,
                } => PreparedNativeArchiveBackend::Ready {
                    rom_path,
                    system,
                    loaded,
                },
                PreparedSevenZipBackend::Selection(entries) => {
                    PreparedNativeArchiveBackend::Selection(entries)
                }
            },
        );
    }
    if super::path_extension_is(source_path, "zip") {
        return prepare_zip_backend(
            source_path,
            selected_entry_index,
            expected_rom_path,
            config,
            cancel,
            progress,
        );
    }
    if !super::path_extension_is(source_path, "rar") {
        return Err(super::super::super::pce_cd::PceCdLoadError::PackagedCdSetUnsupported.into());
    }
    check_package_cancel(cancel)?;
    match super::super::super::pce_cd_rar::inspect_rar_cue_members(source_path)?.as_slice() {
        [] => Err(super::super::super::pce_cd::PceCdLoadError::NoArchiveCue.into()),
        [cue_name] => {
            let cue_path = archive_member_virtual_path(source_path, cue_name);
            if selected_entry_index.is_some()
                || expected_rom_path.is_some_and(|expected| expected != cue_path)
            {
                return Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
            }
            let (actual, loaded_disc) =
                super::super::super::pce_cd_rar::load_rar_cue_with_control_and_mods(
                    source_path,
                    Arc::clone(cancel),
                    Arc::clone(progress),
                    config.apply_mods,
                )?;
            if actual != cue_path {
                return Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
            }
            let loaded = finish_prepared_pce_cd_backend(
                source_path,
                &cue_path,
                loaded_disc,
                config,
                cancel,
                progress,
            )?;
            Ok(PreparedNativeArchiveBackend::Ready {
                rom_path: cue_path,
                system: super::super::super::ActiveSystem::Pce,
                loaded,
            })
        }
        cue_members => {
            let Some(cue_name) = select_archive_cue_member(
                source_path,
                cue_members,
                selected_entry_index,
                expected_rom_path,
            )?
            else {
                return Ok(PreparedNativeArchiveBackend::Selection(
                    archive_cue_entries(cue_members),
                ));
            };
            let cue_path = archive_member_virtual_path(source_path, cue_name);
            let (actual, loaded_disc, _) = super::super::super::pce_cd_rar::load_rar_selected_cue_with_control_and_archive_identity(
                source_path,
                cue_name,
                Arc::clone(cancel),
                Arc::clone(progress),
                config.apply_mods,
            )?;
            if actual != cue_path {
                return Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
            }
            let loaded = finish_prepared_pce_cd_backend(
                source_path,
                &cue_path,
                loaded_disc,
                config,
                cancel,
                progress,
            )?;
            Ok(PreparedNativeArchiveBackend::Ready {
                rom_path: cue_path,
                system: super::super::super::ActiveSystem::Pce,
                loaded,
            })
        }
    }
}

fn prepare_zip_backend(
    source_path: &Path,
    selected_entry_index: Option<usize>,
    expected_rom_path: Option<&Path>,
    config: &BackendLoadConfig,
    cancel: &Arc<AtomicBool>,
    progress: &Arc<super::super::super::pce_cd_archive::PceCdPackageProgress>,
) -> anyhow::Result<PreparedNativeArchiveBackend> {
    check_package_cancel(cancel)?;
    let cue_members = super::super::super::pce_cd_zip::inspect_zip_cue_members(source_path)?;
    match cue_members.as_slice() {
        [] => Err(super::super::super::pce_cd::PceCdLoadError::NoArchiveCue.into()),
        [cue_name] => {
            let cue_path = archive_member_virtual_path(source_path, cue_name);
            if selected_entry_index.is_some()
                || expected_rom_path.is_some_and(|expected| expected != cue_path)
            {
                return Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
            }
            let (actual, loaded_disc) =
                super::super::super::pce_cd_zip::load_zip_cue_with_control_and_mods(
                    source_path,
                    Arc::clone(cancel),
                    Arc::clone(progress),
                    config.apply_mods,
                )?;
            if actual != cue_path {
                return Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
            }
            let loaded = finish_prepared_pce_cd_backend(
                source_path,
                &cue_path,
                loaded_disc,
                config,
                cancel,
                progress,
            )?;
            Ok(PreparedNativeArchiveBackend::Ready {
                rom_path: cue_path,
                system: super::super::super::ActiveSystem::Pce,
                loaded,
            })
        }
        cue_members => {
            let Some(cue_name) = select_archive_cue_member(
                source_path,
                cue_members,
                selected_entry_index,
                expected_rom_path,
            )?
            else {
                return Ok(PreparedNativeArchiveBackend::Selection(
                    archive_cue_entries(cue_members),
                ));
            };
            let cue_path = archive_member_virtual_path(source_path, cue_name);
            let (actual, loaded_disc, _) = super::super::super::pce_cd_zip::load_zip_selected_cue_with_control_and_archive_identity(
                source_path,
                cue_name,
                Arc::clone(cancel),
                Arc::clone(progress),
                config.apply_mods,
            )?;
            if actual != cue_path {
                return Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged.into());
            }
            let loaded = finish_prepared_pce_cd_backend(
                source_path,
                &cue_path,
                loaded_disc,
                config,
                cancel,
                progress,
            )?;
            Ok(PreparedNativeArchiveBackend::Ready {
                rom_path: cue_path,
                system: super::super::super::ActiveSystem::Pce,
                loaded,
            })
        }
    }
}

fn archive_member_virtual_path(source_path: &Path, member_name: &str) -> PathBuf {
    member_name
        .split('/')
        .fold(source_path.to_path_buf(), |path, part| path.join(part))
}

fn archive_cue_entries(cue_members: &[String]) -> Vec<crate::rom_archive::ArchiveRomEntry> {
    cue_members
        .iter()
        .enumerate()
        .map(|(index, name)| crate::rom_archive::ArchiveRomEntry {
            index,
            name: name.clone(),
            system: super::super::super::ActiveSystem::Pce,
            uncompressed_size: 0,
        })
        .collect()
}

fn select_archive_cue_member<'a>(
    source_path: &Path,
    cue_members: &'a [String],
    selected_entry_index: Option<usize>,
    expected_rom_path: Option<&Path>,
) -> Result<Option<&'a str>, super::super::super::pce_cd::PceCdLoadError> {
    let selected_by_index = selected_entry_index
        .map(|index| {
            cue_members
                .get(index)
                .map(String::as_str)
                .ok_or(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged)
        })
        .transpose()?;
    let selected_by_path = expected_rom_path.map(|expected| {
        cue_members
            .iter()
            .map(String::as_str)
            .find(|member| archive_member_virtual_path(source_path, member) == expected)
    });
    match (selected_by_index, selected_by_path.flatten()) {
        (Some(selected), Some(expected)) if selected != expected => {
            Err(super::super::super::pce_cd::PceCdLoadError::ArchiveChanged)
        }
        (Some(selected), _) | (_, Some(selected)) => Ok(Some(selected)),
        (None, None) if cue_members.len() == 1 => Ok(cue_members.first().map(String::as_str)),
        (None, None) => Ok(None),
    }
}
