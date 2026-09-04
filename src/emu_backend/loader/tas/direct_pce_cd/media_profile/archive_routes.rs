use super::*;

pub(super) fn archive_route(
    format: PceCdArchiveFormat,
    selected: bool,
    ppf: bool,
) -> PceCdTasMediaRoute {
    let selection = selection(selected);
    if ppf {
        PceCdTasMediaRoute::ArchivePpf(format, selection)
    } else {
        PceCdTasMediaRoute::Archive(format, selection)
    }
}

pub(super) fn archive_ppf_sync_config(
    format: PceCdArchiveFormat,
    selection: PceCdArchiveSelection,
) -> TasDigest {
    match (format, selection) {
        (PceCdArchiveFormat::SevenZip, PceCdArchiveSelection::Unique) => {
            direct_pce_cd_archive_ppf_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::SevenZip, PceCdArchiveSelection::Selected) => {
            direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Rar, PceCdArchiveSelection::Unique) => {
            direct_pce_cd_rar_ppf_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Rar, PceCdArchiveSelection::Selected) => {
            direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Zip, PceCdArchiveSelection::Unique) => {
            direct_pce_cd_zip_ppf_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Zip, PceCdArchiveSelection::Selected) => {
            direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256()
        }
    }
}

pub(super) fn archive_sync_config(
    format: PceCdArchiveFormat,
    selection: PceCdArchiveSelection,
    expansion: PceCdExpansion,
) -> TasDigest {
    match (format, selection, expansion) {
        (PceCdArchiveFormat::SevenZip, PceCdArchiveSelection::Unique, PceCdExpansion::None) => {
            direct_pce_cd_archive_tas_sync_config_sha256()
        }
        (
            PceCdArchiveFormat::SevenZip,
            PceCdArchiveSelection::Unique,
            PceCdExpansion::ArcadeCard,
        ) => direct_pce_cd_archive_arcade_tas_sync_config_sha256(),
        (
            PceCdArchiveFormat::SevenZip,
            PceCdArchiveSelection::Unique,
            PceCdExpansion::MemoryBase128,
        ) => direct_pce_cd_archive_memory_base_tas_sync_config_sha256(),
        (PceCdArchiveFormat::SevenZip, PceCdArchiveSelection::Selected, PceCdExpansion::None) => {
            direct_pce_cd_selected_archive_tas_sync_config_sha256()
        }
        (
            PceCdArchiveFormat::SevenZip,
            PceCdArchiveSelection::Selected,
            PceCdExpansion::ArcadeCard,
        ) => direct_pce_cd_selected_archive_arcade_tas_sync_config_sha256(),
        (
            PceCdArchiveFormat::SevenZip,
            PceCdArchiveSelection::Selected,
            PceCdExpansion::MemoryBase128,
        ) => direct_pce_cd_selected_archive_memory_base_tas_sync_config_sha256(),
        (PceCdArchiveFormat::Rar, PceCdArchiveSelection::Unique, PceCdExpansion::None) => {
            direct_pce_cd_rar_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Rar, PceCdArchiveSelection::Unique, PceCdExpansion::ArcadeCard) => {
            direct_pce_cd_rar_arcade_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Rar, PceCdArchiveSelection::Unique, PceCdExpansion::MemoryBase128) => {
            direct_pce_cd_rar_memory_base_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Rar, PceCdArchiveSelection::Selected, PceCdExpansion::None) => {
            direct_pce_cd_selected_rar_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Rar, PceCdArchiveSelection::Selected, PceCdExpansion::ArcadeCard) => {
            direct_pce_cd_selected_rar_arcade_tas_sync_config_sha256()
        }
        (
            PceCdArchiveFormat::Rar,
            PceCdArchiveSelection::Selected,
            PceCdExpansion::MemoryBase128,
        ) => direct_pce_cd_selected_rar_memory_base_tas_sync_config_sha256(),
        (PceCdArchiveFormat::Zip, PceCdArchiveSelection::Unique, PceCdExpansion::None) => {
            direct_pce_cd_zip_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Zip, PceCdArchiveSelection::Unique, PceCdExpansion::ArcadeCard) => {
            direct_pce_cd_zip_arcade_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Zip, PceCdArchiveSelection::Unique, PceCdExpansion::MemoryBase128) => {
            direct_pce_cd_zip_memory_base_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Zip, PceCdArchiveSelection::Selected, PceCdExpansion::None) => {
            direct_pce_cd_selected_zip_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Zip, PceCdArchiveSelection::Selected, PceCdExpansion::ArcadeCard) => {
            direct_pce_cd_selected_zip_arcade_tas_sync_config_sha256()
        }
        (
            PceCdArchiveFormat::Zip,
            PceCdArchiveSelection::Selected,
            PceCdExpansion::MemoryBase128,
        ) => direct_pce_cd_selected_zip_memory_base_tas_sync_config_sha256(),
    }
}

pub(super) fn archive_multitap_sync_config(
    format: PceCdArchiveFormat,
    selection: PceCdArchiveSelection,
) -> TasDigest {
    match (format, selection) {
        (PceCdArchiveFormat::SevenZip, PceCdArchiveSelection::Unique) => {
            direct_pce_multitap_cd_archive_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::SevenZip, PceCdArchiveSelection::Selected) => {
            direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Rar, PceCdArchiveSelection::Unique) => {
            direct_pce_multitap_cd_rar_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Rar, PceCdArchiveSelection::Selected) => {
            direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Zip, PceCdArchiveSelection::Unique) => {
            direct_pce_multitap_cd_zip_tas_sync_config_sha256()
        }
        (PceCdArchiveFormat::Zip, PceCdArchiveSelection::Selected) => {
            direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256()
        }
    }
}
