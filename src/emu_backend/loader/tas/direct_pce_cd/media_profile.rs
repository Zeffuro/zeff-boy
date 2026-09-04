use zeff_pce_core::hardware::PceControllerMode;

use super::{
    direct_pce_cd_arcade_tas_sync_config_sha256,
    direct_pce_cd_archive_arcade_tas_sync_config_sha256,
    direct_pce_cd_archive_memory_base_tas_sync_config_sha256,
    direct_pce_cd_archive_ppf_source_identity, direct_pce_cd_archive_ppf_tas_sync_config_sha256,
    direct_pce_cd_archive_source_identity, direct_pce_cd_archive_tas_sync_config_sha256,
    direct_pce_cd_chd_arcade_tas_sync_config_sha256,
    direct_pce_cd_chd_memory_base_tas_sync_config_sha256, direct_pce_cd_chd_tas_sync_config_sha256,
    direct_pce_cd_iso_arcade_tas_sync_config_sha256,
    direct_pce_cd_iso_memory_base_tas_sync_config_sha256, direct_pce_cd_iso_tas_sync_config_sha256,
    direct_pce_cd_memory_base_tas_sync_config_sha256,
    direct_pce_cd_ppf_arcade_tas_sync_config_sha256,
    direct_pce_cd_ppf_memory_base_tas_sync_config_sha256, direct_pce_cd_ppf_tas_sync_config_sha256,
    direct_pce_cd_rar_arcade_tas_sync_config_sha256,
    direct_pce_cd_rar_memory_base_tas_sync_config_sha256, direct_pce_cd_rar_ppf_source_identity,
    direct_pce_cd_rar_ppf_tas_sync_config_sha256, direct_pce_cd_rar_source_identity,
    direct_pce_cd_rar_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_ppf_tas_sync_config_sha256,
    direct_pce_cd_selected_archive_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_ppf_tas_sync_config_sha256,
    direct_pce_cd_selected_rar_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_arcade_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_memory_base_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_ppf_tas_sync_config_sha256,
    direct_pce_cd_selected_zip_tas_sync_config_sha256, direct_pce_cd_tas_sync_config_sha256,
    direct_pce_cd_zip_arcade_tas_sync_config_sha256,
    direct_pce_cd_zip_memory_base_tas_sync_config_sha256, direct_pce_cd_zip_ppf_source_identity,
    direct_pce_cd_zip_ppf_tas_sync_config_sha256, direct_pce_cd_zip_source_identity,
    direct_pce_cd_zip_tas_sync_config_sha256, direct_pce_multitap_cd_arcade_tas_sync_config_sha256,
    direct_pce_multitap_cd_archive_tas_sync_config_sha256,
    direct_pce_multitap_cd_chd_tas_sync_config_sha256,
    direct_pce_multitap_cd_iso_tas_sync_config_sha256,
    direct_pce_multitap_cd_memory_base_tas_sync_config_sha256,
    direct_pce_multitap_cd_ppf_tas_sync_config_sha256,
    direct_pce_multitap_cd_rar_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256,
    direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256,
    direct_pce_multitap_cd_tas_sync_config_sha256,
    direct_pce_multitap_cd_zip_tas_sync_config_sha256,
};
use crate::tas_project::TasDigest;

#[path = "media_profile/archive_routes.rs"]
mod archive_routes;
use archive_routes::{
    archive_multitap_sync_config, archive_ppf_sync_config, archive_route, archive_sync_config,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PceCdArchiveFormat {
    SevenZip,
    Rar,
    Zip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PceCdArchiveSelection {
    Unique,
    Selected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PceCdTasMediaRoute {
    Cue,
    Chd,
    Iso,
    Ppf,
    Archive(PceCdArchiveFormat, PceCdArchiveSelection),
    ArchivePpf(PceCdArchiveFormat, PceCdArchiveSelection),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PceCdExpansion {
    None,
    ArcadeCard,
    MemoryBase128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PceCdTasProfile {
    media: PceCdTasMediaRoute,
    expansion: PceCdExpansion,
    controller: PceControllerMode,
}

impl PceCdTasProfile {
    pub(crate) fn from_runtime_flags(
        media: (bool, bool, bool, bool, bool, bool),
        archive_ppf: bool,
        archive_selection: (bool, bool, bool),
        cards: (bool, bool),
        controller: PceControllerMode,
    ) -> Option<Self> {
        let (chd, iso, ppf, archive, rar, zip) = media;
        let (archive_selected, rar_selected, zip_selected) = archive_selection;
        if [chd, iso, ppf, archive, rar, zip]
            .into_iter()
            .filter(|selected| *selected)
            .count()
            > 1
            || [archive_selected, rar_selected, zip_selected]
                .into_iter()
                .filter(|selected| *selected)
                .count()
                > 1
            || archive_selected && !archive
            || rar_selected && !rar
            || zip_selected && !zip
            || archive_ppf && !(archive || rar || zip)
            || archive_ppf && (cards.0 || cards.1 || controller != PceControllerMode::TwoButton)
            || cards.0 && cards.1
            || !matches!(
                controller,
                PceControllerMode::TwoButton | PceControllerMode::Multitap
            )
        {
            return None;
        }
        let media = if chd {
            PceCdTasMediaRoute::Chd
        } else if iso {
            PceCdTasMediaRoute::Iso
        } else if ppf {
            PceCdTasMediaRoute::Ppf
        } else if archive {
            archive_route(PceCdArchiveFormat::SevenZip, archive_selected, archive_ppf)
        } else if rar {
            archive_route(PceCdArchiveFormat::Rar, rar_selected, archive_ppf)
        } else if zip {
            archive_route(PceCdArchiveFormat::Zip, zip_selected, archive_ppf)
        } else {
            PceCdTasMediaRoute::Cue
        };
        let expansion = if cards.0 {
            PceCdExpansion::ArcadeCard
        } else if cards.1 {
            PceCdExpansion::MemoryBase128
        } else {
            PceCdExpansion::None
        };
        if controller == PceControllerMode::Multitap
            && !matches!(
                (media, expansion),
                (PceCdTasMediaRoute::Cue, PceCdExpansion::ArcadeCard)
                    | (PceCdTasMediaRoute::Cue, PceCdExpansion::MemoryBase128)
                    | (_, PceCdExpansion::None)
            )
        {
            return None;
        }
        Some(Self {
            media,
            expansion,
            controller,
        })
    }

    pub(crate) fn from_sync(sync: TasDigest) -> Option<Self> {
        let direct = [
            PceCdTasMediaRoute::Cue,
            PceCdTasMediaRoute::Chd,
            PceCdTasMediaRoute::Iso,
            PceCdTasMediaRoute::Ppf,
        ];
        let archives = [
            PceCdArchiveFormat::SevenZip,
            PceCdArchiveFormat::Rar,
            PceCdArchiveFormat::Zip,
        ];
        let selections = [
            PceCdArchiveSelection::Unique,
            PceCdArchiveSelection::Selected,
        ];
        let expansions = [
            PceCdExpansion::None,
            PceCdExpansion::ArcadeCard,
            PceCdExpansion::MemoryBase128,
        ];
        for media in direct
            .into_iter()
            .chain(archives.into_iter().flat_map(|format| {
                selections
                    .into_iter()
                    .map(move |selection| PceCdTasMediaRoute::Archive(format, selection))
            }))
        {
            for expansion in expansions {
                let profile = Self {
                    media,
                    expansion,
                    controller: PceControllerMode::TwoButton,
                };
                if profile.sync_config() == sync {
                    return Some(profile);
                }
                if expansion == PceCdExpansion::None
                    || (media == PceCdTasMediaRoute::Cue
                        && matches!(
                            expansion,
                            PceCdExpansion::ArcadeCard | PceCdExpansion::MemoryBase128
                        ))
                {
                    let profile = Self {
                        media,
                        expansion,
                        controller: PceControllerMode::Multitap,
                    };
                    if profile.sync_config() == sync {
                        return Some(profile);
                    }
                }
            }
        }
        for format in archives {
            for selection in selections {
                let profile = Self {
                    media: PceCdTasMediaRoute::ArchivePpf(format, selection),
                    expansion: PceCdExpansion::None,
                    controller: PceControllerMode::TwoButton,
                };
                if profile.sync_config() == sync {
                    return Some(profile);
                }
            }
        }
        None
    }

    pub(crate) fn sync_config(self) -> TasDigest {
        if self.controller == PceControllerMode::Multitap {
            return match (self.media, self.expansion) {
                (PceCdTasMediaRoute::Cue, PceCdExpansion::None) => {
                    direct_pce_multitap_cd_tas_sync_config_sha256()
                }
                (PceCdTasMediaRoute::Cue, PceCdExpansion::ArcadeCard) => {
                    direct_pce_multitap_cd_arcade_tas_sync_config_sha256()
                }
                (PceCdTasMediaRoute::Cue, PceCdExpansion::MemoryBase128) => {
                    direct_pce_multitap_cd_memory_base_tas_sync_config_sha256()
                }
                (PceCdTasMediaRoute::Chd, PceCdExpansion::None) => {
                    direct_pce_multitap_cd_chd_tas_sync_config_sha256()
                }
                (PceCdTasMediaRoute::Iso, PceCdExpansion::None) => {
                    direct_pce_multitap_cd_iso_tas_sync_config_sha256()
                }
                (PceCdTasMediaRoute::Ppf, PceCdExpansion::None) => {
                    direct_pce_multitap_cd_ppf_tas_sync_config_sha256()
                }
                (PceCdTasMediaRoute::Archive(format, selection), PceCdExpansion::None) => {
                    archive_multitap_sync_config(format, selection)
                }
                (PceCdTasMediaRoute::ArchivePpf(_, _), _) => {
                    unreachable!("archive PPF does not support PC Engine CD Multitap")
                }
                _ => unreachable!("invalid PC Engine CD Multitap profile"),
            };
        }
        match self.media {
            PceCdTasMediaRoute::Cue => match self.expansion {
                PceCdExpansion::None => direct_pce_cd_tas_sync_config_sha256(),
                PceCdExpansion::ArcadeCard => direct_pce_cd_arcade_tas_sync_config_sha256(),
                PceCdExpansion::MemoryBase128 => direct_pce_cd_memory_base_tas_sync_config_sha256(),
            },
            PceCdTasMediaRoute::Chd => match self.expansion {
                PceCdExpansion::None => direct_pce_cd_chd_tas_sync_config_sha256(),
                PceCdExpansion::ArcadeCard => direct_pce_cd_chd_arcade_tas_sync_config_sha256(),
                PceCdExpansion::MemoryBase128 => {
                    direct_pce_cd_chd_memory_base_tas_sync_config_sha256()
                }
            },
            PceCdTasMediaRoute::Iso => match self.expansion {
                PceCdExpansion::None => direct_pce_cd_iso_tas_sync_config_sha256(),
                PceCdExpansion::ArcadeCard => direct_pce_cd_iso_arcade_tas_sync_config_sha256(),
                PceCdExpansion::MemoryBase128 => {
                    direct_pce_cd_iso_memory_base_tas_sync_config_sha256()
                }
            },
            PceCdTasMediaRoute::Ppf => match self.expansion {
                PceCdExpansion::None => direct_pce_cd_ppf_tas_sync_config_sha256(),
                PceCdExpansion::ArcadeCard => direct_pce_cd_ppf_arcade_tas_sync_config_sha256(),
                PceCdExpansion::MemoryBase128 => {
                    direct_pce_cd_ppf_memory_base_tas_sync_config_sha256()
                }
            },
            PceCdTasMediaRoute::Archive(format, selection) => {
                archive_sync_config(format, selection, self.expansion)
            }
            PceCdTasMediaRoute::ArchivePpf(format, selection) => {
                assert_eq!(self.expansion, PceCdExpansion::None);
                archive_ppf_sync_config(format, selection)
            }
        }
    }

    pub(crate) fn media(self) -> PceCdTasMediaRoute {
        self.media
    }

    pub(crate) fn expansion(self) -> PceCdExpansion {
        self.expansion
    }

    pub(crate) fn controller(self) -> PceControllerMode {
        self.controller
    }

    pub(crate) fn archive(self) -> Option<(PceCdArchiveFormat, PceCdArchiveSelection)> {
        match self.media {
            PceCdTasMediaRoute::Archive(format, selection)
            | PceCdTasMediaRoute::ArchivePpf(format, selection) => Some((format, selection)),
            _ => None,
        }
    }

    pub(crate) fn archive_ppf(self) -> bool {
        matches!(self.media, PceCdTasMediaRoute::ArchivePpf(_, _))
    }

    pub(crate) fn archive_source_identity(
        self,
        raw_source_sha256: [u8; 32],
        raw_source_len: usize,
        cue_member_path_sha256: [u8; 32],
    ) -> Option<TasDigest> {
        let (format, _) = self.archive()?;
        if self.archive_ppf() {
            return None;
        }
        Some(match format {
            PceCdArchiveFormat::SevenZip => direct_pce_cd_archive_source_identity(
                raw_source_sha256,
                raw_source_len,
                cue_member_path_sha256,
            ),
            PceCdArchiveFormat::Rar => direct_pce_cd_rar_source_identity(
                raw_source_sha256,
                raw_source_len,
                cue_member_path_sha256,
            ),
            PceCdArchiveFormat::Zip => direct_pce_cd_zip_source_identity(
                raw_source_sha256,
                raw_source_len,
                cue_member_path_sha256,
            ),
        })
    }

    pub(crate) fn archive_ppf_source_identity(
        self,
        raw_source_sha256: [u8; 32],
        raw_source_len: usize,
        cue_member_path_sha256: [u8; 32],
        patches: &[(&str, usize, [u8; 32])],
    ) -> Option<TasDigest> {
        let (format, _) = self.archive()?;
        if !self.archive_ppf() {
            return None;
        }
        Some(match format {
            PceCdArchiveFormat::SevenZip => direct_pce_cd_archive_ppf_source_identity(
                raw_source_sha256,
                raw_source_len,
                cue_member_path_sha256,
                patches,
            ),
            PceCdArchiveFormat::Rar => direct_pce_cd_rar_ppf_source_identity(
                raw_source_sha256,
                raw_source_len,
                cue_member_path_sha256,
                patches,
            ),
            PceCdArchiveFormat::Zip => direct_pce_cd_zip_ppf_source_identity(
                raw_source_sha256,
                raw_source_len,
                cue_member_path_sha256,
                patches,
            ),
        })
    }
}

fn selection(selected: bool) -> PceCdArchiveSelection {
    if selected {
        PceCdArchiveSelection::Selected
    } else {
        PceCdArchiveSelection::Unique
    }
}

#[cfg(test)]
pub(super) fn sync_config_for_runtime(
    media: (bool, bool, bool, bool, bool, bool),
    archive_selection: (bool, bool, bool),
    cards: (bool, bool),
) -> TasDigest {
    PceCdTasProfile::from_runtime_flags(
        media,
        false,
        archive_selection,
        cards,
        PceControllerMode::TwoButton,
    )
    .expect("two-button PC Engine CD profile")
    .sync_config()
}

pub(super) fn arcade_sync_config(sync: TasDigest) -> bool {
    PceCdTasProfile::from_sync(sync)
        .is_some_and(|profile| profile.expansion() == PceCdExpansion::ArcadeCard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_and_multitap_syncs_round_trip_through_profiles() {
        let archive_vectors = [
            (
                PceCdArchiveFormat::SevenZip,
                PceCdArchiveSelection::Unique,
                PceCdExpansion::None,
                direct_pce_cd_archive_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::SevenZip,
                PceCdArchiveSelection::Unique,
                PceCdExpansion::ArcadeCard,
                direct_pce_cd_archive_arcade_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::SevenZip,
                PceCdArchiveSelection::Unique,
                PceCdExpansion::MemoryBase128,
                direct_pce_cd_archive_memory_base_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::SevenZip,
                PceCdArchiveSelection::Selected,
                PceCdExpansion::None,
                direct_pce_cd_selected_archive_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::SevenZip,
                PceCdArchiveSelection::Selected,
                PceCdExpansion::ArcadeCard,
                direct_pce_cd_selected_archive_arcade_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::SevenZip,
                PceCdArchiveSelection::Selected,
                PceCdExpansion::MemoryBase128,
                direct_pce_cd_selected_archive_memory_base_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Rar,
                PceCdArchiveSelection::Unique,
                PceCdExpansion::None,
                direct_pce_cd_rar_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Rar,
                PceCdArchiveSelection::Unique,
                PceCdExpansion::ArcadeCard,
                direct_pce_cd_rar_arcade_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Rar,
                PceCdArchiveSelection::Unique,
                PceCdExpansion::MemoryBase128,
                direct_pce_cd_rar_memory_base_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Rar,
                PceCdArchiveSelection::Selected,
                PceCdExpansion::None,
                direct_pce_cd_selected_rar_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Rar,
                PceCdArchiveSelection::Selected,
                PceCdExpansion::ArcadeCard,
                direct_pce_cd_selected_rar_arcade_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Rar,
                PceCdArchiveSelection::Selected,
                PceCdExpansion::MemoryBase128,
                direct_pce_cd_selected_rar_memory_base_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Zip,
                PceCdArchiveSelection::Unique,
                PceCdExpansion::None,
                direct_pce_cd_zip_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Zip,
                PceCdArchiveSelection::Unique,
                PceCdExpansion::ArcadeCard,
                direct_pce_cd_zip_arcade_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Zip,
                PceCdArchiveSelection::Unique,
                PceCdExpansion::MemoryBase128,
                direct_pce_cd_zip_memory_base_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Zip,
                PceCdArchiveSelection::Selected,
                PceCdExpansion::None,
                direct_pce_cd_selected_zip_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Zip,
                PceCdArchiveSelection::Selected,
                PceCdExpansion::ArcadeCard,
                direct_pce_cd_selected_zip_arcade_tas_sync_config_sha256(),
            ),
            (
                PceCdArchiveFormat::Zip,
                PceCdArchiveSelection::Selected,
                PceCdExpansion::MemoryBase128,
                direct_pce_cd_selected_zip_memory_base_tas_sync_config_sha256(),
            ),
        ];
        for (format, selection, expansion, sync) in archive_vectors {
            let profile = PceCdTasProfile::from_sync(sync).expect("known archive sync");
            assert_eq!(profile.archive(), Some((format, selection)));
            assert_eq!(profile.expansion(), expansion);
            assert_eq!(profile.controller(), PceControllerMode::TwoButton);
            assert_eq!(profile.sync_config(), sync);
        }

        for (media, sync) in [
            (
                PceCdTasMediaRoute::Cue,
                direct_pce_multitap_cd_tas_sync_config_sha256(),
            ),
            (
                PceCdTasMediaRoute::Chd,
                direct_pce_multitap_cd_chd_tas_sync_config_sha256(),
            ),
            (
                PceCdTasMediaRoute::Iso,
                direct_pce_multitap_cd_iso_tas_sync_config_sha256(),
            ),
            (
                PceCdTasMediaRoute::Ppf,
                direct_pce_multitap_cd_ppf_tas_sync_config_sha256(),
            ),
            (
                PceCdTasMediaRoute::Archive(
                    PceCdArchiveFormat::SevenZip,
                    PceCdArchiveSelection::Unique,
                ),
                direct_pce_multitap_cd_archive_tas_sync_config_sha256(),
            ),
            (
                PceCdTasMediaRoute::Archive(
                    PceCdArchiveFormat::SevenZip,
                    PceCdArchiveSelection::Selected,
                ),
                direct_pce_multitap_cd_selected_archive_tas_sync_config_sha256(),
            ),
            (
                PceCdTasMediaRoute::Archive(PceCdArchiveFormat::Rar, PceCdArchiveSelection::Unique),
                direct_pce_multitap_cd_rar_tas_sync_config_sha256(),
            ),
            (
                PceCdTasMediaRoute::Archive(
                    PceCdArchiveFormat::Rar,
                    PceCdArchiveSelection::Selected,
                ),
                direct_pce_multitap_cd_selected_rar_tas_sync_config_sha256(),
            ),
            (
                PceCdTasMediaRoute::Archive(PceCdArchiveFormat::Zip, PceCdArchiveSelection::Unique),
                direct_pce_multitap_cd_zip_tas_sync_config_sha256(),
            ),
            (
                PceCdTasMediaRoute::Archive(
                    PceCdArchiveFormat::Zip,
                    PceCdArchiveSelection::Selected,
                ),
                direct_pce_multitap_cd_selected_zip_tas_sync_config_sha256(),
            ),
        ] {
            let profile = PceCdTasProfile::from_sync(sync).expect("known Multitap sync");
            assert_eq!(profile.media(), media);
            assert_eq!(profile.expansion(), PceCdExpansion::None);
            assert_eq!(profile.controller(), PceControllerMode::Multitap);
            assert_eq!(profile.sync_config(), sync);
        }

        let arcade_multitap =
            PceCdTasProfile::from_sync(direct_pce_multitap_cd_arcade_tas_sync_config_sha256())
                .expect("known Arcade Card Multitap sync");
        assert_eq!(arcade_multitap.media(), PceCdTasMediaRoute::Cue);
        assert_eq!(arcade_multitap.expansion(), PceCdExpansion::ArcadeCard);
        assert_eq!(arcade_multitap.controller(), PceControllerMode::Multitap);
        assert_eq!(
            arcade_multitap.sync_config(),
            direct_pce_multitap_cd_arcade_tas_sync_config_sha256()
        );

        let memory_base_multitap =
            PceCdTasProfile::from_sync(direct_pce_multitap_cd_memory_base_tas_sync_config_sha256())
                .expect("known Memory Base Multitap sync");
        assert_eq!(memory_base_multitap.media(), PceCdTasMediaRoute::Cue);
        assert_eq!(
            memory_base_multitap.expansion(),
            PceCdExpansion::MemoryBase128
        );
        assert_eq!(
            memory_base_multitap.controller(),
            PceControllerMode::Multitap
        );
        assert_eq!(
            memory_base_multitap.sync_config(),
            direct_pce_multitap_cd_memory_base_tas_sync_config_sha256()
        );
    }

    #[test]
    fn invalid_profile_flag_combinations_are_rejected() {
        let none_selected = (false, false, false);
        let no_cards = (false, false);
        let invalid = [
            (
                (true, true, false, false, false, false),
                none_selected,
                no_cards,
                PceControllerMode::TwoButton,
            ),
            (
                (false, false, false, true, true, false),
                none_selected,
                no_cards,
                PceControllerMode::TwoButton,
            ),
            (
                (false, false, false, false, false, false),
                (true, false, false),
                no_cards,
                PceControllerMode::TwoButton,
            ),
            (
                (false, false, false, true, false, false),
                (false, true, false),
                no_cards,
                PceControllerMode::TwoButton,
            ),
            (
                (false, false, false, true, false, false),
                (true, true, false),
                no_cards,
                PceControllerMode::TwoButton,
            ),
            (
                (false, false, false, false, false, false),
                none_selected,
                (true, true),
                PceControllerMode::TwoButton,
            ),
            (
                (true, false, false, false, false, false),
                none_selected,
                (true, false),
                PceControllerMode::Multitap,
            ),
            (
                (false, false, false, true, false, false),
                none_selected,
                (true, false),
                PceControllerMode::Multitap,
            ),
            (
                (false, false, false, false, false, true),
                (false, false, true),
                (false, true),
                PceControllerMode::Multitap,
            ),
            (
                (true, false, false, false, false, false),
                none_selected,
                (false, true),
                PceControllerMode::Multitap,
            ),
            (
                (false, false, false, false, false, false),
                none_selected,
                no_cards,
                PceControllerMode::Automatic,
            ),
            (
                (false, false, false, false, false, false),
                none_selected,
                no_cards,
                PceControllerMode::SixButton,
            ),
            (
                (false, false, false, false, false, false),
                none_selected,
                no_cards,
                PceControllerMode::Mouse,
            ),
        ];
        for (media, selection, cards, controller) in invalid {
            assert!(
                PceCdTasProfile::from_runtime_flags(media, false, selection, cards, controller)
                    .is_none()
            );
        }
    }
}
