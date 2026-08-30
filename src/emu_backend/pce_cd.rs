#![cfg(not(target_arch = "wasm32"))]

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crc32fast::Hasher as Crc32;
use sha2::{Digest, Sha256};
use zeff_pce_core::hardware::{CdDisc, CdTrack, CdTrackMode};

pub(crate) const PCE_CD_CUE_BYTES_LIMIT: usize = 1024 * 1024;
// Multi-track files can transiently duplicate spans; 80-minute raw discs exceed 800 MiB.
pub(crate) const PCE_CD_DATA_BYTES_LIMIT: usize = 900 * 1024 * 1024;
const _: () = assert!(PCE_CD_DATA_BYTES_LIMIT >= 80 * 60 * 75 * 2_352);
pub(crate) const PCE_CD_FILE_REFERENCE_LIMIT: usize = 99;
pub(crate) const PCE_CD_PATH_BYTES_LIMIT: usize = 1024;
pub(crate) const PCE_CD_PATH_COMPONENT_BYTES_LIMIT: usize = 255;
pub(crate) const PCE_CD_PATH_DEPTH_LIMIT: usize = 16;

pub(super) const CONTENT_ID_DOMAIN: &[u8] = b"zeff-boy:pce-cd-data:v2";
pub(super) const ADPCM_FIXTURE_DISC_SHA256: [u8; 32] = [
    0xC8, 0xC7, 0x42, 0x6B, 0x3F, 0x91, 0xD7, 0xBF, 0xB5, 0xF5, 0x02, 0x9F, 0xFE, 0x18, 0xD8, 0xE2,
    0x60, 0x41, 0x95, 0xDA, 0xF8, 0xF5, 0x4C, 0x8B, 0x24, 0x94, 0xC2, 0x98, 0x1E, 0x8F, 0x68, 0xA2,
];

pub(crate) struct LoadedPceCd {
    pub(crate) disc: CdDisc,
    pub(crate) content_sha256: [u8; 32],
    pub(crate) content_crc32: u32,
    pub(crate) mod_crc32: u32,
    pub(crate) source_disc_sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PceCdLoadError {
    PackagedCdSetUnsupported,
    CueUnreadable(PathBuf),
    CueTooLarge(u64),
    CueNotUtf8,
    IsoCueMissing(PathBuf),
    IsoCueAmbiguous(Vec<PathBuf>),
    MissingFile,
    DuplicateFile,
    TooManyFileReferences,
    UnsafeFileReference(String),
    AmbiguousFileReference(String),
    BinUnreadable(PathBuf),
    ChdUnreadable(PathBuf),
    DataTooLarge(u64),
    UnsupportedFileType(String),
    UnsupportedTrackMode(String),
    UnsupportedChdTrack(String),
    UnsupportedChdPregap(u8),
    UnsupportedChdPostgap(u8),
    InvalidChdMetadata,
    MalformedLine(usize),
    DuplicateTrack(u8),
    MissingIndex1(u8),
    DuplicateIndex {
        track: u8,
        index: u8,
    },
    DuplicatePregap(u8),
    InvalidIndexOrder(u8),
    InvalidTrackOrder,
    MisalignedBin {
        bytes: usize,
        sector_bytes: usize,
    },
    TrackOutsideBin(u8),
    MixedSectorSizes,
    ArchiveUnreadable(PathBuf),
    ArchiveTooLarge(u64),
    TooManyArchiveEntries(usize),
    NoArchiveCue,
    MultipleArchiveCues,
    UnsafeArchiveEntry(String),
    DuplicateArchiveEntry(String),
    ArchiveLinkUnsupported(String),
    ArchiveCrcRequired(String),
    ArchiveCodecUnsupported(String),
    ArchiveMemoryLimit {
        allowed_mib: usize,
        required_mib: usize,
    },
    ArchiveAllocationFailed,
    ArchiveChecksumMismatch,
    ArchiveDecodedLimit,
    ArchiveChanged,
    ArchiveCancelled,
    ArchiveMemberMissing(String),
    ArchiveMemberSizeMismatch(String),
    NoSupportedArchiveContent,
    UnrecognizedSystemCardFirmware([u8; 32]),
    SystemCardRegionMismatch {
        expected: zeff_firmware::PceSystemCardRegion,
        actual: zeff_firmware::PceSystemCardRegion,
    },
    SystemCardTierTooLow {
        title: &'static str,
        required: zeff_firmware::PceSystemCardTier,
        selected: zeff_firmware::PceSystemCardTier,
    },
    Archive(String),
    Disc(String),
}

impl Display for PceCdLoadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ArchiveMemoryLimit {
                allowed_mib,
                required_mib,
            } => write!(
                formatter,
                "PC Engine CD archive requires {required_mib} MiB of decoder memory; configured limit is {allowed_mib} MiB"
            ),
            Self::ArchiveAllocationFailed => {
                formatter.write_str("archive memory allocation failed")
            }
            Self::NoSupportedArchiveContent => {
                formatter.write_str("archive contains no supported ROM or CUE set")
            }
            Self::SystemCardTierTooLow {
                title,
                required,
                selected,
            } => write!(
                formatter,
                "{title} requires System Card {required:?}, but the selected firmware is {selected:?}"
            ),
            _ => write!(formatter, "unsupported PC Engine CD set: {self:?}"),
        }
    }
}

impl Error for PceCdLoadError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CueSheet {
    pub(super) files: Vec<CueFile>,
    pub(super) tracks: Vec<CueTrack>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CueFile {
    pub(super) reference: String,
    pub(super) track_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CueTrack {
    pub(super) number: u8,
    pub(super) file_index: usize,
    pub(super) mode: CdTrackMode,
    pub(super) index0: Option<u32>,
    pub(super) index1: Option<u32>,
    pub(super) pregap: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CueTrackLayout {
    pub(super) track: CueTrack,
    pub(super) index0: Option<u32>,
    pub(super) index1: u32,
    pub(super) stored_start: u32,
    pub(super) source_bytes: Range<usize>,
    pub(super) virtual_pregap: bool,
}

impl CueTrackLayout {
    pub(super) fn control(&self) -> u8 {
        if self.track.mode == CdTrackMode::Audio {
            0
        } else {
            4
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChdTrack {
    pub(super) number: u8,
    pub(super) mode: CdTrackMode,
    pub(super) frames: u32,
    pub(super) pregap: u32,
}

impl ChdTrack {
    pub(super) fn payload_bytes(&self) -> usize {
        sector_bytes(self.mode)
    }
}

#[cfg(test)]
pub(crate) fn load_direct_cue(cue_path: &Path) -> Result<LoadedPceCd, PceCdLoadError> {
    load_direct_cue_with_mods(cue_path, false)
}

pub(crate) fn load_direct_cue_with_mods(
    cue_path: &Path,
    apply_mods: bool,
) -> Result<LoadedPceCd, PceCdLoadError> {
    let cue_metadata = std::fs::metadata(cue_path)
        .map_err(|_| PceCdLoadError::CueUnreadable(cue_path.to_path_buf()))?;
    if cue_metadata.len() > PCE_CD_CUE_BYTES_LIMIT as u64 {
        return Err(PceCdLoadError::CueTooLarge(cue_metadata.len()));
    }
    let cue_bytes = std::fs::read(cue_path)
        .map_err(|_| PceCdLoadError::CueUnreadable(cue_path.to_path_buf()))?;
    let sheet = parse_cue_bytes(&cue_bytes)?;
    let mut file_backed =
        super::pce_cd_file::load_direct_cue_file_backed(cue_path, &cue_bytes, &sheet)?;
    if !apply_mods {
        return Ok(file_backed);
    }
    let (dir, mods, selected_crc32) = pce_cd_mod_config(
        crc32fast::hash(&file_backed.source_disc_sha256),
        file_backed.content_crc32,
    );
    if !mods.iter().any(|entry| entry.enabled) {
        file_backed.mod_crc32 = selected_crc32;
        return Ok(file_backed);
    }
    if let Some(disc) =
        super::pce_cd_file::try_load_direct_cue_ppf_overlay(cue_path, &sheet, &dir, &mods)?
    {
        file_backed.disc = disc;
        file_backed.mod_crc32 = selected_crc32;
        return Ok(file_backed);
    }
    let parent = cue_path.parent().unwrap_or_else(|| Path::new(""));
    let mut data = Vec::with_capacity(sheet.files.len());
    let mut total = 0_u64;
    for file in &sheet.files {
        let path = resolve_direct_file_reference(parent, &file.reference)?;
        let metadata =
            std::fs::metadata(&path).map_err(|_| PceCdLoadError::BinUnreadable(path.clone()))?;
        total = total
            .checked_add(metadata.len())
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))?;
        if total > PCE_CD_DATA_BYTES_LIMIT as u64 {
            return Err(PceCdLoadError::DataTooLarge(total));
        }
        data.push(std::fs::read(&path).map_err(|_| PceCdLoadError::BinUnreadable(path.clone()))?);
    }
    build_disc_with_mods(cue_bytes, &sheet, data, true)
}

pub(crate) fn cue_path_for_iso(iso_path: &Path) -> Result<PathBuf, PceCdLoadError> {
    let canonical_iso = std::fs::canonicalize(iso_path)
        .map_err(|_| PceCdLoadError::BinUnreadable(iso_path.to_path_buf()))?;
    let parent = iso_path.parent().unwrap_or_else(|| Path::new(""));
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    let entries = std::fs::read_dir(parent)
        .map_err(|_| PceCdLoadError::BinUnreadable(iso_path.to_path_buf()))?;
    let mut matches = Vec::new();
    for entry in entries.flatten() {
        let cue_path = entry.path();
        if !cue_path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("cue"))
        {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > PCE_CD_CUE_BYTES_LIMIT as u64 {
            continue;
        }
        let Ok(cue_bytes) = std::fs::read(&cue_path) else {
            continue;
        };
        let Ok(sheet) = parse_cue_bytes(&cue_bytes) else {
            continue;
        };
        let references_iso = sheet.files.iter().any(|file| {
            resolve_direct_file_reference(parent, &file.reference)
                .and_then(|referenced| {
                    std::fs::canonicalize(referenced)
                        .map_err(|_| PceCdLoadError::BinUnreadable(iso_path.to_path_buf()))
                })
                .is_ok_and(|canonical| canonical == canonical_iso)
        });
        if references_iso {
            matches.push(cue_path);
        }
    }
    matches.sort();
    match matches.as_slice() {
        [cue] => Ok(cue.clone()),
        [] => Err(PceCdLoadError::IsoCueMissing(iso_path.to_path_buf())),
        _ => Err(PceCdLoadError::IsoCueAmbiguous(matches)),
    }
}

pub(super) fn resolve_direct_file_reference(
    parent: &Path,
    reference: &str,
) -> Result<PathBuf, PceCdLoadError> {
    let candidate = parent.join(portable_path(reference));
    let mut resolved = if parent.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        parent.to_path_buf()
    };
    for component in reference.split('/') {
        let entries = std::fs::read_dir(&resolved)
            .map_err(|_| PceCdLoadError::BinUnreadable(candidate.clone()))?;
        let mut matches = entries
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case(component))
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        matches.sort();
        resolved = match matches.as_slice() {
            [path] => path.clone(),
            [] => return Err(PceCdLoadError::BinUnreadable(candidate)),
            _ => {
                return Err(PceCdLoadError::AmbiguousFileReference(reference.to_owned()));
            }
        };
    }
    if std::fs::metadata(&resolved).is_ok_and(|metadata| metadata.is_file()) {
        Ok(resolved)
    } else {
        Err(PceCdLoadError::BinUnreadable(candidate))
    }
}

pub(crate) fn load_direct_chd_with_mods(
    chd_path: &Path,
    apply_mods: bool,
) -> Result<LoadedPceCd, PceCdLoadError> {
    super::pce_cd_chd::load_direct_chd_with_mods(chd_path, apply_mods)
}

pub(super) fn parse_cue_bytes(cue_bytes: &[u8]) -> Result<CueSheet, PceCdLoadError> {
    if cue_bytes.len() > PCE_CD_CUE_BYTES_LIMIT {
        return Err(PceCdLoadError::CueTooLarge(cue_bytes.len() as u64));
    }
    let cue = std::str::from_utf8(cue_bytes).map_err(|_| PceCdLoadError::CueNotUtf8)?;
    parse_cue(cue)
}

fn parse_cue(cue: &str) -> Result<CueSheet, PceCdLoadError> {
    let mut files: Vec<CueFile> = Vec::new();
    let mut tracks: Vec<CueTrack> = Vec::new();
    let mut current_file = None;
    for (line_index, source) in cue.lines().enumerate() {
        let line_number = line_index + 1;
        let line = source.trim();
        if line.is_empty() {
            continue;
        }
        let keyword = line.split_ascii_whitespace().next().unwrap();
        if keyword.eq_ignore_ascii_case("REM") {
            continue;
        }
        if keyword.eq_ignore_ascii_case("FILE") {
            if files.len() == PCE_CD_FILE_REFERENCE_LIMIT {
                return Err(PceCdLoadError::TooManyFileReferences);
            }
            let reference = parse_file_reference(line, line_number)?;
            if files
                .iter()
                .any(|file| file.reference.eq_ignore_ascii_case(reference.as_str()))
            {
                return Err(PceCdLoadError::DuplicateFile);
            }
            current_file = Some(files.len());
            files.push(CueFile {
                reference,
                track_indices: Vec::new(),
            });
            continue;
        }
        if keyword.eq_ignore_ascii_case("TRACK") {
            let file_index = current_file.ok_or(PceCdLoadError::MalformedLine(line_number))?;
            let expected_number = match tracks.last() {
                None => 1,
                Some(track) => track
                    .number
                    .checked_add(1)
                    .ok_or(PceCdLoadError::InvalidTrackOrder)?,
            };
            let mut fields = line.split_ascii_whitespace();
            fields.next();
            let number: u8 = fields
                .next()
                .and_then(|value| value.parse().ok())
                .filter(|number| (1..=99).contains(number))
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            if number != expected_number {
                return Err(PceCdLoadError::InvalidTrackOrder);
            }
            if tracks.iter().any(|track| track.number == number) {
                return Err(PceCdLoadError::DuplicateTrack(number));
            }
            let mode = match fields.next().map(str::to_ascii_uppercase).as_deref() {
                Some("MODE1/2352") => CdTrackMode::Mode1_2352,
                Some("MODE1/2048") => CdTrackMode::Mode1_2048,
                Some("AUDIO") => CdTrackMode::Audio,
                Some(mode) => return Err(PceCdLoadError::UnsupportedTrackMode(mode.to_owned())),
                None => return Err(PceCdLoadError::MalformedLine(line_number)),
            };
            if fields.next().is_some() {
                return Err(PceCdLoadError::MalformedLine(line_number));
            }
            files[file_index].track_indices.push(tracks.len());
            tracks.push(CueTrack {
                number,
                file_index,
                mode,
                index0: None,
                index1: None,
                pregap: None,
            });
            continue;
        }
        if keyword.eq_ignore_ascii_case("PREGAP") {
            let track = tracks
                .last_mut()
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            if track.index0.is_some() || track.index1.is_some() {
                return Err(PceCdLoadError::InvalidIndexOrder(track.number));
            }
            let mut fields = line.split_ascii_whitespace();
            fields.next();
            let pregap = fields
                .next()
                .and_then(parse_msf)
                .filter(|pregap| *pregap != 0)
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            if fields.next().is_some() {
                return Err(PceCdLoadError::MalformedLine(line_number));
            }
            if track.pregap.replace(pregap).is_some() {
                return Err(PceCdLoadError::DuplicatePregap(track.number));
            }
            continue;
        }
        if keyword.eq_ignore_ascii_case("INDEX") {
            let track = tracks
                .last_mut()
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            let mut fields = line.split_ascii_whitespace();
            fields.next();
            let index: u8 = fields
                .next()
                .and_then(|value| value.parse().ok())
                .filter(|index| matches!(index, 0 | 1))
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            let lba = fields
                .next()
                .and_then(parse_msf)
                .ok_or(PceCdLoadError::MalformedLine(line_number))?;
            if fields.next().is_some() {
                return Err(PceCdLoadError::MalformedLine(line_number));
            }
            let destination = if index == 0 {
                if track.index1.is_some() || track.pregap.is_some() {
                    return Err(PceCdLoadError::InvalidIndexOrder(track.number));
                }
                &mut track.index0
            } else {
                &mut track.index1
            };
            if destination.replace(lba).is_some() {
                return Err(PceCdLoadError::DuplicateIndex {
                    track: track.number,
                    index,
                });
            }
            continue;
        }
        if !matches!(
            keyword.to_ascii_uppercase().as_str(),
            "CATALOG" | "TITLE" | "PERFORMER" | "SONGWRITER"
        ) {
            return Err(PceCdLoadError::MalformedLine(line_number));
        }
    }
    if files.is_empty() {
        return Err(PceCdLoadError::MissingFile);
    }
    if tracks.is_empty() || files.iter().any(|file| file.track_indices.is_empty()) {
        return Err(PceCdLoadError::InvalidTrackOrder);
    }
    Ok(CueSheet { files, tracks })
}

fn parse_file_reference(line: &str, line_number: usize) -> Result<String, PceCdLoadError> {
    let arguments = line
        .get(4..)
        .ok_or(PceCdLoadError::MalformedLine(line_number))?
        .trim_start();
    let remainder = arguments
        .strip_prefix('"')
        .ok_or(PceCdLoadError::MalformedLine(line_number))?;
    let end = remainder
        .find('"')
        .ok_or(PceCdLoadError::MalformedLine(line_number))?;
    if !remainder[end + 1..].trim().eq_ignore_ascii_case("BINARY") {
        return Err(PceCdLoadError::UnsupportedFileType(
            remainder[end + 1..].trim().to_owned(),
        ));
    }
    normalize_portable_path(&remainder[..end])
        .map_err(|_| PceCdLoadError::UnsafeFileReference(remainder[..end].to_owned()))
}

pub(super) fn normalize_portable_path(value: &str) -> Result<String, ()> {
    if value.is_empty()
        || value.len() > PCE_CD_PATH_BYTES_LIMIT
        || value.contains('\0')
        || value.contains(':')
        || value.starts_with('/')
        || value.starts_with('\\')
    {
        return Err(());
    }
    let replaced = value.replace('\\', "/");
    if replaced.starts_with('/') || replaced.ends_with('/') {
        return Err(());
    }
    let components = replaced.split('/').collect::<Vec<_>>();
    if components.is_empty()
        || components.len() > PCE_CD_PATH_DEPTH_LIMIT
        || components.iter().any(|component| {
            component.is_empty()
                || matches!(*component, "." | "..")
                || component.len() > PCE_CD_PATH_COMPONENT_BYTES_LIMIT
        })
    {
        return Err(());
    }
    Ok(components.join("/"))
}

pub(super) fn cue_track_layout(
    sheet: &CueSheet,
    file_bytes: &[usize],
) -> Result<Vec<Vec<CueTrackLayout>>, PceCdLoadError> {
    if file_bytes.len() != sheet.files.len() {
        return Err(PceCdLoadError::MissingFile);
    }

    let mut cursor = 0_u32;
    let mut layout = Vec::with_capacity(sheet.files.len());
    for (file_index, (cue_file, &file_bytes)) in sheet.files.iter().zip(file_bytes).enumerate() {
        let file_tracks = cue_file
            .track_indices
            .iter()
            .map(|&index| &sheet.tracks[index])
            .collect::<Vec<_>>();
        let first_track = file_tracks[0];
        let sector_len = sector_bytes(first_track.mode);
        if file_tracks
            .iter()
            .any(|track| sector_bytes(track.mode) != sector_len)
        {
            return Err(PceCdLoadError::MixedSectorSizes);
        }
        if !file_bytes.is_multiple_of(sector_len) {
            return Err(PceCdLoadError::MisalignedBin {
                bytes: file_bytes,
                sector_bytes: sector_len,
            });
        }
        let total_sectors = u32::try_from(file_bytes / sector_len)
            .map_err(|_| PceCdLoadError::TrackOutsideBin(first_track.number))?;
        let anchor = if file_index == 0 {
            first_track
                .index1
                .ok_or(PceCdLoadError::MissingIndex1(first_track.number))?
        } else {
            first_track
                .index0
                .or(first_track.index1)
                .ok_or(PceCdLoadError::MissingIndex1(first_track.number))?
        };
        if anchor >= total_sectors {
            return Err(PceCdLoadError::TrackOutsideBin(first_track.number));
        }

        let base = cursor;
        let mut virtual_offset = 0_u32;
        let mut file_layout = Vec::with_capacity(file_tracks.len());
        for (track_offset, &&track) in file_tracks.iter().enumerate() {
            debug_assert_eq!(track.file_index, file_index);
            let raw_index1 = track
                .index1
                .ok_or(PceCdLoadError::MissingIndex1(track.number))?;
            if track.index0.is_some_and(|index0| index0 > raw_index1) {
                return Err(PceCdLoadError::InvalidIndexOrder(track.number));
            }
            let end = file_tracks
                .get(track_offset + 1)
                .map(|next| next.index0.unwrap_or(next.index1.unwrap_or(u32::MAX)))
                .unwrap_or(total_sectors);
            if raw_index1 >= end || end > total_sectors {
                return Err(PceCdLoadError::TrackOutsideBin(track.number));
            }
            let virtual_pregap = track.pregap.unwrap_or(0);
            let index1 = raw_index1
                .checked_sub(anchor)
                .and_then(|index| base.checked_add(index))
                .and_then(|index| index.checked_add(virtual_offset))
                .and_then(|index| index.checked_add(virtual_pregap))
                .ok_or(PceCdLoadError::InvalidTrackOrder)?;
            let index0 = if virtual_pregap != 0 {
                Some(
                    index1
                        .checked_sub(virtual_pregap)
                        .ok_or(PceCdLoadError::InvalidTrackOrder)?,
                )
            } else {
                track
                    .index0
                    .and_then(|index| index.checked_sub(anchor))
                    .map(|index| {
                        base.checked_add(index)
                            .and_then(|index| index.checked_add(virtual_offset))
                            .ok_or(PceCdLoadError::InvalidTrackOrder)
                    })
                    .transpose()?
            };
            let raw_stored_start = if virtual_pregap == 0 {
                index0.and(track.index0).unwrap_or(raw_index1)
            } else {
                raw_index1
            };
            let source_bytes = raw_stored_start as usize * sector_len..end as usize * sector_len;
            file_layout.push(CueTrackLayout {
                track,
                index0,
                index1,
                stored_start: if virtual_pregap != 0 {
                    index1
                } else {
                    index0.unwrap_or(index1)
                },
                source_bytes,
                virtual_pregap: virtual_pregap != 0,
            });
            virtual_offset = virtual_offset
                .checked_add(virtual_pregap)
                .ok_or(PceCdLoadError::InvalidTrackOrder)?;
        }
        cursor = cursor
            .checked_add(
                total_sectors
                    .checked_sub(anchor)
                    .ok_or(PceCdLoadError::InvalidTrackOrder)?,
            )
            .and_then(|cursor| cursor.checked_add(virtual_offset))
            .ok_or(PceCdLoadError::TrackOutsideBin(first_track.number))?;
        layout.push(file_layout);
    }
    Ok(layout)
}

#[cfg(test)]
pub(super) fn build_disc(
    cue_bytes: Vec<u8>,
    sheet: &CueSheet,
    files: Vec<Vec<u8>>,
) -> Result<LoadedPceCd, PceCdLoadError> {
    build_disc_with_mods(cue_bytes, sheet, files, false)
}

pub(super) fn build_disc_with_mods(
    cue_bytes: Vec<u8>,
    sheet: &CueSheet,
    mut files: Vec<Vec<u8>>,
    apply_mods: bool,
) -> Result<LoadedPceCd, PceCdLoadError> {
    if files.len() != sheet.files.len() {
        return Err(PceCdLoadError::MissingFile);
    }
    let total_bytes = files
        .iter()
        .try_fold(0_u64, |total, file| total.checked_add(file.len() as u64));
    if total_bytes.is_none_or(|total| total > PCE_CD_DATA_BYTES_LIMIT as u64) {
        return Err(PceCdLoadError::DataTooLarge(
            total_bytes.unwrap_or(u64::MAX),
        ));
    }

    let (content_sha256, content_crc32) = content_identity(&cue_bytes, sheet, &files);
    let normalized_source_disc_sha256 = normalized_disc_identity(sheet, &files)?;
    let canonical_mod_crc32 = crc32fast::hash(&normalized_source_disc_sha256);
    let mut mod_crc32 = canonical_mod_crc32;
    let mut source_disc_sha256 = None;
    if apply_mods {
        let (dir, mods, selected_crc32) = pce_cd_mod_config(canonical_mod_crc32, content_crc32);
        mod_crc32 = selected_crc32;
        let enabled = mods.iter().filter(|entry| entry.enabled).count();
        if enabled != 0 {
            source_disc_sha256 = Some(normalized_source_disc_sha256);
            let targets = cue_patch_targets(sheet, &files)?;
            let warnings =
                crate::mods::apply_enabled_pce_cd_mods(&mut files, &targets, &dir, &mods);
            for warning in &warnings {
                log::warn!("Mod warning: {warning}");
            }
            log::info!(
                "Applied {enabled} mod(s) to PC Engine CD set ({} warnings)",
                warnings.len()
            );
        }
    }
    let file_bytes = files.iter().map(Vec::len).collect::<Vec<_>>();
    let layout = cue_track_layout(sheet, &file_bytes)?;
    let mut normalized = Vec::with_capacity(sheet.tracks.len());
    for ((cue_file, mut bytes), file_layout) in sheet.files.iter().zip(files).zip(layout) {
        for track_layout in file_layout {
            let track = track_layout.track;
            let source_start = track_layout.source_bytes.start;
            let source_end = track_layout.source_bytes.end;
            let track_bytes = if cue_file.track_indices.len() == 1 {
                bytes.drain(..source_start);
                bytes.truncate(source_end - source_start);
                std::mem::take(&mut bytes)
            } else {
                bytes[source_start..source_end].to_vec()
            };
            let track = if track_layout.virtual_pregap {
                CdTrack::from_index1_data(
                    track.number,
                    track_layout.control(),
                    track_layout.index0,
                    track_layout.index1,
                    track.mode,
                    track_bytes,
                )
            } else {
                CdTrack::from_stored_data(
                    track.number,
                    track_layout.control(),
                    track_layout.index0,
                    track_layout.index1,
                    track.mode,
                    track_bytes,
                )
            }
            .map_err(|error| PceCdLoadError::Disc(error.to_string()))?;
            normalized.push(track);
        }
    }

    let disc = CdDisc::new(normalized).map_err(|error| PceCdLoadError::Disc(error.to_string()))?;
    Ok(LoadedPceCd {
        source_disc_sha256: source_disc_sha256.unwrap_or_else(|| disc.content_hash()),
        disc,
        content_sha256,
        content_crc32,
        mod_crc32,
    })
}

pub(super) fn pce_cd_mod_config(
    mod_crc32: u32,
    legacy_crc32: u32,
) -> (PathBuf, Vec<crate::mods::ModEntry>, u32) {
    let canonical = crate::mods::mods_dir_for_rom(super::ActiveSystem::Pce, mod_crc32);
    let mods = crate::mods::load_mod_config(&canonical);
    if !mods.is_empty() || mod_crc32 == legacy_crc32 {
        return (canonical, mods, mod_crc32);
    }
    let legacy = crate::mods::mods_dir_for_rom(super::ActiveSystem::Pce, legacy_crc32);
    let legacy_mods = crate::mods::load_mod_config(&legacy);
    if legacy_mods.is_empty() {
        (canonical, mods, mod_crc32)
    } else {
        log::info!("Using legacy PC Engine CD mod directory; move it to {mod_crc32:08x}");
        (legacy, legacy_mods, legacy_crc32)
    }
}

fn cue_patch_targets(
    sheet: &CueSheet,
    files: &[Vec<u8>],
) -> Result<Vec<crate::mods::PceCdPatchTarget>, PceCdLoadError> {
    let mut targets = sheet
        .files
        .iter()
        .enumerate()
        .map(|(segment, file)| crate::mods::PceCdPatchTarget::File {
            reference: file.reference.clone(),
            segment,
        })
        .collect::<Vec<_>>();
    for (file_index, (cue_file, bytes)) in sheet.files.iter().zip(files).enumerate() {
        let file_tracks = cue_file
            .track_indices
            .iter()
            .map(|index| &sheet.tracks[*index])
            .collect::<Vec<_>>();
        let sector_len = sector_bytes(file_tracks[0].mode);
        if file_tracks
            .iter()
            .any(|track| sector_bytes(track.mode) != sector_len)
        {
            return Err(PceCdLoadError::MixedSectorSizes);
        }
        if !bytes.len().is_multiple_of(sector_len) {
            return Err(PceCdLoadError::MisalignedBin {
                bytes: bytes.len(),
                sector_bytes: sector_len,
            });
        }
        let total_sectors = u32::try_from(bytes.len() / sector_len)
            .map_err(|_| PceCdLoadError::TrackOutsideBin(file_tracks[0].number))?;
        for (track_offset, track) in file_tracks.iter().enumerate() {
            let index1 = track
                .index1
                .ok_or(PceCdLoadError::MissingIndex1(track.number))?;
            let end = file_tracks
                .get(track_offset + 1)
                .map(|next| next.index0.unwrap_or(next.index1.unwrap_or(u32::MAX)))
                .unwrap_or(total_sectors);
            let start = track.index0.unwrap_or(index1);
            if index1 >= end || end > total_sectors || start > index1 {
                return Err(PceCdLoadError::TrackOutsideBin(track.number));
            }
            targets.push(crate::mods::PceCdPatchTarget::Track {
                number: track.number,
                segment: file_index,
                bytes: Range {
                    start: start as usize * sector_len,
                    end: end as usize * sector_len,
                },
            });
        }
    }
    Ok(targets)
}

pub(super) fn parse_chd_track_metadata(value: &[u8]) -> Result<ChdTrack, PceCdLoadError> {
    let value = std::str::from_utf8(value)
        .map_err(|_| PceCdLoadError::InvalidChdMetadata)?
        .trim_end_matches('\0');
    let mut fields = value.split_ascii_whitespace();
    let track = fields
        .next()
        .and_then(|field| field.strip_prefix("TRACK:"))
        .and_then(|number| number.parse::<u8>().ok())
        .filter(|number| *number != 0)
        .ok_or(PceCdLoadError::InvalidChdMetadata)?;
    let mode = match fields.next().and_then(|field| field.strip_prefix("TYPE:")) {
        Some("AUDIO") => CdTrackMode::Audio,
        Some("MODE1") => CdTrackMode::Mode1_2048,
        Some("MODE1_RAW") => CdTrackMode::Mode1_2352,
        Some(mode) => return Err(PceCdLoadError::UnsupportedChdTrack(mode.to_owned())),
        None => return Err(PceCdLoadError::InvalidChdMetadata),
    };
    if fields.next() != Some("SUBTYPE:NONE") {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    let frames = fields
        .next()
        .and_then(|field| field.strip_prefix("FRAMES:"))
        .and_then(|frames| frames.parse::<u32>().ok())
        .filter(|frames| *frames != 0 && *frames <= u32::MAX - 3)
        .ok_or(PceCdLoadError::InvalidChdMetadata)?;
    let pregap = fields
        .next()
        .and_then(|field| field.strip_prefix("PREGAP:"))
        .and_then(|pregap| pregap.parse::<u32>().ok())
        .ok_or(PceCdLoadError::InvalidChdMetadata)?;
    let pgtype = fields
        .next()
        .and_then(|field| field.strip_prefix("PGTYPE:"))
        .ok_or(PceCdLoadError::InvalidChdMetadata)?;
    if pregap > frames || (pregap != 0 && pgtype != valid_chd_pregap_type(mode)) {
        return Err(PceCdLoadError::UnsupportedChdPregap(track));
    }
    if fields.next() != Some("PGSUB:NONE") {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    let postgap = fields
        .next()
        .and_then(|field| field.strip_prefix("POSTGAP:"))
        .and_then(|postgap| postgap.parse::<u32>().ok())
        .ok_or(PceCdLoadError::InvalidChdMetadata)?;
    if postgap != 0 {
        return Err(PceCdLoadError::UnsupportedChdPostgap(track));
    }
    if fields.next().is_some() {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    Ok(ChdTrack {
        number: track,
        mode,
        frames,
        pregap,
    })
}

fn valid_chd_pregap_type(mode: CdTrackMode) -> &'static str {
    match mode {
        CdTrackMode::Audio => "VAUDIO",
        CdTrackMode::Mode1_2048 => "VMODE1",
        CdTrackMode::Mode1_2352 => "VMODE1_RAW",
    }
}

pub(super) fn padded_chd_frames(frames: u32) -> u32 {
    frames.next_multiple_of(4)
}

pub(super) fn build_chd_disc_with_mods_and_identity(
    mut tracks: Vec<ChdTrack>,
    mut payloads: Vec<Vec<u8>>,
    apply_mods: bool,
    (content_sha256, content_crc32): ([u8; 32], u32),
) -> Result<LoadedPceCd, PceCdLoadError> {
    let clean_disc = build_chd_disc(&tracks, &payloads)?;
    let source_disc_sha256 = clean_disc.content_hash();
    let canonical_mod_crc32 = crc32fast::hash(&source_disc_sha256);
    let mut mod_crc32 = canonical_mod_crc32;
    if apply_mods {
        let (dir, mods, selected_crc32) = pce_cd_mod_config(canonical_mod_crc32, content_crc32);
        mod_crc32 = selected_crc32;
        let enabled = mods.iter().filter(|entry| entry.enabled).count();
        if enabled != 0 {
            let targets = tracks
                .iter()
                .enumerate()
                .map(|(segment, track)| crate::mods::PceCdPatchTarget::Track {
                    number: track.number,
                    segment,
                    bytes: 0..payloads[segment].len(),
                })
                .collect::<Vec<_>>();
            let warnings =
                crate::mods::apply_enabled_pce_cd_mods(&mut payloads, &targets, &dir, &mods);
            for warning in &warnings {
                log::warn!("Mod warning: {warning}");
            }
            log::info!(
                "Applied {enabled} mod(s) to PC Engine CD set ({} warnings)",
                warnings.len()
            );
            refresh_chd_track_lengths(&mut tracks, &payloads)?;
        }
    }
    let disc = if apply_mods {
        build_chd_disc(&tracks, &payloads)?
    } else {
        clean_disc
    };
    Ok(LoadedPceCd {
        disc,
        content_sha256,
        content_crc32,
        mod_crc32,
        source_disc_sha256,
    })
}

fn refresh_chd_track_lengths(
    tracks: &mut [ChdTrack],
    payloads: &[Vec<u8>],
) -> Result<(), PceCdLoadError> {
    if tracks.len() != payloads.len() {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    for (track, payload) in tracks.iter_mut().zip(payloads) {
        let sector_bytes = track.payload_bytes();
        if !payload.len().is_multiple_of(sector_bytes) {
            return Err(PceCdLoadError::MisalignedBin {
                bytes: payload.len(),
                sector_bytes,
            });
        }
        track.frames = u32::try_from(payload.len() / sector_bytes)
            .map_err(|_| PceCdLoadError::InvalidChdMetadata)?;
        if track.frames == 0 || track.pregap > track.frames {
            return Err(PceCdLoadError::InvalidChdMetadata);
        }
    }
    Ok(())
}

pub(super) fn build_chd_disc(
    tracks: &[ChdTrack],
    payloads: &[Vec<u8>],
) -> Result<CdDisc, PceCdLoadError> {
    if tracks.len() != payloads.len() {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    let mut cursor = 0_u32;
    let mut disc_tracks = Vec::with_capacity(tracks.len());
    for (track, payload) in tracks.iter().zip(payloads) {
        if payload.len() != track.frames as usize * track.payload_bytes() {
            return Err(PceCdLoadError::InvalidChdMetadata);
        }
        let index0 = (track.pregap != 0).then_some(cursor);
        let index1 = cursor
            .checked_add(track.pregap)
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
        disc_tracks.push(
            CdTrack::from_stored_data(
                track.number,
                if track.mode == CdTrackMode::Audio {
                    0
                } else {
                    4
                },
                index0,
                index1,
                track.mode,
                payload.clone(),
            )
            .map_err(|error| PceCdLoadError::Disc(error.to_string()))?,
        );
        cursor = cursor
            .checked_add(track.frames)
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
    }
    CdDisc::new(disc_tracks).map_err(|error| PceCdLoadError::Disc(error.to_string()))
}

#[cfg(test)]
fn normalize_chd_audio_payloads(tracks: &[ChdTrack], payloads: &mut [Vec<u8>]) {
    for (track, payload) in tracks.iter().zip(payloads) {
        if track.mode == CdTrackMode::Audio {
            let (samples, remainder) = payload.as_chunks_mut::<2>();
            debug_assert!(remainder.is_empty());
            for sample in samples {
                sample.swap(0, 1);
            }
        }
    }
}

pub(super) fn chd_content_identity_from_header(
    header: &[u8; 124],
    tracks: &[ChdTrack],
) -> Result<([u8; 32], u32), PceCdLoadError> {
    let mut sha = Sha256::new();
    let mut crc = Crc32::new();
    update_identity(&mut sha, &mut crc, b"zeff-boy:pce-cd-chd:v1");
    update_identity(&mut sha, &mut crc, &header[64..124]);
    for track in tracks {
        update_identity(&mut sha, &mut crc, &[track.number]);
        update_identity(&mut sha, &mut crc, &[track.mode as u8]);
        update_identity(&mut sha, &mut crc, &track.frames.to_le_bytes());
        update_identity(&mut sha, &mut crc, &track.pregap.to_le_bytes());
    }
    Ok((sha.finalize().into(), crc.finalize()))
}

fn normalized_disc_identity(
    sheet: &CueSheet,
    files: &[Vec<u8>],
) -> Result<[u8; 32], PceCdLoadError> {
    let file_bytes = files.iter().map(Vec::len).collect::<Vec<_>>();
    let layout = cue_track_layout(sheet, &file_bytes)?;
    let mut hasher = Sha256::new();
    hasher.update(b"zeff-boy:pce-core-cd-disc:v1\0");
    hasher.update((sheet.tracks.len() as u32).to_le_bytes());
    for file_layout in layout {
        for track_layout in file_layout {
            let track = track_layout.track;
            let control = track_layout.control();
            let stored_data = &files[track.file_index][track_layout.source_bytes];
            hasher.update([track.number, control]);
            match track_layout.index0 {
                Some(index0) => {
                    hasher.update([1]);
                    hasher.update(index0.to_le_bytes());
                }
                None => hasher.update([0]),
            }
            hasher.update(track_layout.index1.to_le_bytes());
            hasher.update(track_layout.stored_start.to_le_bytes());
            hasher.update([match track.mode {
                CdTrackMode::Audio => 0,
                CdTrackMode::Mode1_2048 => 1,
                CdTrackMode::Mode1_2352 => 2,
            }]);
            hasher.update((stored_data.len() as u64).to_le_bytes());
            hasher.update(stored_data);
        }
    }
    Ok(hasher.finalize().into())
}

fn content_identity(cue_bytes: &[u8], sheet: &CueSheet, files: &[Vec<u8>]) -> ([u8; 32], u32) {
    let mut sha = Sha256::new();
    let mut crc = Crc32::new();
    update_identity(&mut sha, &mut crc, CONTENT_ID_DOMAIN);
    update_identity(&mut sha, &mut crc, cue_bytes);
    let count = (sheet.files.len() as u64).to_le_bytes();
    sha.update(count);
    crc.update(&count);
    for (cue_file, bytes) in sheet.files.iter().zip(files) {
        update_identity(&mut sha, &mut crc, cue_file.reference.as_bytes());
        update_identity(&mut sha, &mut crc, bytes);
    }
    (sha.finalize().into(), crc.finalize())
}

fn update_identity(sha: &mut Sha256, crc: &mut Crc32, bytes: &[u8]) {
    let len = (bytes.len() as u64).to_le_bytes();
    sha.update(len);
    sha.update(bytes);
    crc.update(&len);
    crc.update(bytes);
}

fn portable_path(reference: &str) -> PathBuf {
    reference.split('/').collect()
}

fn sector_bytes(mode: CdTrackMode) -> usize {
    match mode {
        CdTrackMode::Mode1_2048 => 2_048,
        CdTrackMode::Mode1_2352 => 2_352,
        CdTrackMode::Audio => 2_352,
    }
}

fn parse_msf(value: &str) -> Option<u32> {
    let mut fields = value.split(':');
    let minutes: u32 = fields.next()?.parse().ok()?;
    let seconds: u32 = fields.next()?.parse().ok()?;
    let frames: u32 = fields.next()?.parse().ok()?;
    if fields.next().is_some() || seconds >= 60 || frames >= 75 {
        return None;
    }
    minutes
        .checked_mul(60)?
        .checked_add(seconds)?
        .checked_mul(75)?
        .checked_add(frames)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emu_backend::{
        ActiveSystem, BackendLoadConfig, EmuBackend, load_backend_from_rom_source,
    };
    use crate::emu_core_trait::EmulatorCore;

    fn temp_set(name: &str, files: &[(&str, &[u8])], cue: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("zeff-pce-cd-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        for (name, bytes) in files {
            let path = root.join(portable_path(name));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let cue_path = root.join("disc.cue");
        std::fs::write(&cue_path, cue).unwrap();
        cue_path
    }

    #[test]
    fn direct_cue_strips_first_index_zero_and_reads_mode1_payload() {
        let mut bin = vec![0xE0; 3 * 2_352];
        bin[2_352 + 16..2_352 + 16 + 2_048].fill(0x4C);
        let cue =
            "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
        let path = temp_set("valid", &[("disc.bin", &bin)], cue);
        let loaded = load_direct_cue(&path).unwrap();
        assert_eq!(loaded.disc.track(1).unwrap().index0_lba(), None);
        assert_eq!(loaded.disc.track(1).unwrap().index1_lba(), 0);
        assert_eq!(loaded.disc.read_user_sector(0).unwrap()[0], 0x4C);
    }

    #[test]
    fn selecting_iso_uses_its_unique_referencing_cue() {
        let data = vec![0x5A; 2 * 2_048];
        let cue = "FILE \"DISC.ISO\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
        let cue_path = temp_set("iso-sidecar", &[("disc.iso", &data)], cue);
        let iso_path = cue_path.with_extension("iso");

        assert_eq!(cue_path_for_iso(&iso_path).unwrap(), cue_path);
        let via_cue = load_direct_cue(&cue_path).unwrap();
        let via_iso = load_direct_cue(&cue_path_for_iso(&iso_path).unwrap()).unwrap();
        assert_eq!(via_iso.disc, via_cue.disc);
        assert_eq!(via_iso.content_sha256, via_cue.content_sha256);
    }

    #[test]
    fn selecting_iso_rejects_missing_and_ambiguous_cue_metadata() {
        let missing_root =
            std::env::temp_dir().join(format!("zeff-pce-cd-{}-iso-missing", std::process::id()));
        std::fs::create_dir_all(&missing_root).unwrap();
        let missing_iso = missing_root.join("disc.iso");
        std::fs::write(&missing_iso, [0; 2_048]).unwrap();
        assert_eq!(
            cue_path_for_iso(&missing_iso),
            Err(PceCdLoadError::IsoCueMissing(missing_iso.clone()))
        );

        let ambiguous_root =
            std::env::temp_dir().join(format!("zeff-pce-cd-{}-iso-ambiguous", std::process::id()));
        std::fs::create_dir_all(&ambiguous_root).unwrap();
        let ambiguous_iso = ambiguous_root.join("disc.iso");
        std::fs::write(&ambiguous_iso, [0; 2_048]).unwrap();
        let cue = "FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
        std::fs::write(ambiguous_root.join("one.cue"), cue).unwrap();
        std::fs::write(ambiguous_root.join("two.cue"), cue).unwrap();
        assert!(matches!(
            cue_path_for_iso(&ambiguous_iso),
            Err(PceCdLoadError::IsoCueAmbiguous(cues)) if cues.len() == 2
        ));
    }

    #[test]
    fn multiple_files_reset_indices_and_allow_distinct_data_sector_sizes() {
        let mut first = vec![0; 2 * 2_048];
        first[0] = 0x11;
        let mut second = vec![0; 3 * 2_352];
        second[16] = 0x12;
        second[2_352 + 16] = 0x22;
        let cue = "FILE \"a.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"sub\\b.bin\" BINARY\nTRACK 02 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
        let path = temp_set("multi", &[("a.bin", &first), ("sub/b.bin", &second)], cue);
        let loaded = load_direct_cue(&path).unwrap();
        let second_track = loaded.disc.track(2).unwrap();
        assert_eq!(second_track.index0_lba(), Some(2));
        assert_eq!(second_track.index1_lba(), 3);
        assert_eq!(loaded.disc.read_user_sector(0).unwrap()[0], 0x11);
        assert_eq!(loaded.disc.read_user_sector(2).unwrap()[0], 0x12);
        assert_eq!(loaded.disc.read_user_sector(3).unwrap()[0], 0x22);
    }

    #[test]
    fn same_file_later_track_retains_index_zero_payload() {
        let mut bin = vec![0; 5 * 2_048];
        bin[2 * 2_048] = 0x20;
        bin[3 * 2_048] = 0x21;
        let cue = "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nTRACK 02 MODE1/2048\nINDEX 00 00:00:02\nINDEX 01 00:00:03\n";
        let path = temp_set("same-file-pregap", &[("disc.bin", &bin)], cue);
        let loaded = load_direct_cue(&path).unwrap();
        let second = loaded.disc.track(2).unwrap();
        assert_eq!(second.stored_start_lba(), 2);
        assert_eq!(second.index1_lba(), 3);
        assert_eq!(loaded.disc.read_user_sector(2).unwrap()[0], 0x20);
        assert_eq!(loaded.disc.read_user_sector(3).unwrap()[0], 0x21);
    }

    #[test]
    fn same_file_virtual_pregap_advances_timeline_without_consuming_payload() {
        let mut data = vec![0; 5 * 2_048];
        data[2 * 2_048] = 0x22;
        let cue = "FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nTRACK 02 MODE1/2048\nPREGAP 00:00:02\nINDEX 01 00:00:02\n";
        let path = temp_set("virtual-pregap", &[("disc.iso", &data)], cue);
        let loaded = load_direct_cue(&path).unwrap();
        let first = loaded.disc.track(1).unwrap();
        let second = loaded.disc.track(2).unwrap();

        assert_eq!(first.end_lba(), 2);
        assert_eq!(second.index0_lba(), Some(2));
        assert_eq!(second.index1_lba(), 4);
        assert_eq!(second.stored_start_lba(), 4);
        assert!(loaded.disc.read_user_sector(2).is_err());
        assert_eq!(loaded.disc.read_user_sector(4).unwrap()[0], 0x22);
    }

    #[test]
    fn cue_rejects_stored_and_virtual_pregap_on_the_same_track() {
        let cue = "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nPREGAP 00:00:02\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
        assert_eq!(parse_cue(cue), Err(PceCdLoadError::InvalidIndexOrder(1)));
    }

    #[test]
    fn lemmings_shaped_multifile_pregaps_keep_track_two_three_and_twenty_five() {
        let mut cue = String::new();
        let mut files = Vec::new();
        for number in 1..=25_u8 {
            let mode = if matches!(number, 2 | 25) {
                CdTrackMode::Mode1_2352
            } else {
                CdTrackMode::Audio
            };
            let pregap = match number {
                2 | 25 => 224,
                3 => 150,
                _ => 0,
            };
            cue.push_str(&format!(
                "FILE \"track{number:02}.bin\" BINARY\nTRACK {number:02} {}\n",
                if mode == CdTrackMode::Audio {
                    "AUDIO"
                } else {
                    "MODE1/2352"
                }
            ));
            if pregap != 0 {
                cue.push_str("INDEX 00 00:00:00\n");
                cue.push_str(&format!(
                    "INDEX 01 00:{:02}:{:02}\n",
                    pregap / 75,
                    pregap % 75
                ));
            } else {
                cue.push_str("INDEX 01 00:00:00\n");
            }
            let mut bytes = vec![0; (pregap + 1) * 2_352];
            if mode == CdTrackMode::Audio {
                bytes[..2].copy_from_slice(&i16::from(number).to_le_bytes());
                bytes[pregap * 2_352..pregap * 2_352 + 2]
                    .copy_from_slice(&i16::from(number + 0x40).to_le_bytes());
            } else {
                bytes[16] = number;
                bytes[pregap * 2_352 + 16] = number + 0x40;
            }
            files.push(bytes);
        }
        let sheet = parse_cue(&cue).unwrap();
        let loaded = build_disc(cue.into_bytes(), &sheet, files).unwrap();
        for (number, pregap) in [(2, 224), (3, 150), (25, 224)] {
            let track = loaded.disc.track(number).unwrap();
            assert_eq!(track.index1_lba() - track.stored_start_lba(), pregap);
        }
        for number in [2, 25] {
            let track = loaded.disc.track(number).unwrap();
            assert_eq!(
                loaded
                    .disc
                    .read_user_sector(track.stored_start_lba())
                    .unwrap()[0],
                number
            );
            assert_eq!(
                loaded.disc.read_user_sector(track.index1_lba()).unwrap()[0],
                number + 0x40
            );
        }
        let track = loaded.disc.track(3).unwrap();
        assert_eq!(
            loaded
                .disc
                .read_audio_sample(track.stored_start_lba(), 0)
                .unwrap()
                .0,
            3
        );
        assert_eq!(
            loaded
                .disc
                .read_audio_sample(track.index1_lba(), 0)
                .unwrap()
                .0,
            0x43
        );
    }

    #[test]
    fn later_files_anchor_at_first_index_zero_or_index_one() {
        let first = vec![0; 2 * 2_048];
        let mut second = vec![0; 10 * 2_048];
        second[7 * 2_048] = 0x27;
        let mut third = vec![0; 8 * 2_048];
        third[5 * 2_048] = 0x35;
        let cue = "FILE \"a.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"b.bin\" BINARY\nTRACK 02 MODE1/2048\nINDEX 00 00:00:05\nINDEX 01 00:00:07\nFILE \"c.bin\" BINARY\nTRACK 03 MODE1/2048\nINDEX 01 00:00:05\n";
        let path = temp_set(
            "file-anchors",
            &[("a.bin", &first), ("b.bin", &second), ("c.bin", &third)],
            cue,
        );
        let loaded = load_direct_cue(&path).unwrap();
        assert_eq!(loaded.disc.track(2).unwrap().index0_lba(), Some(2));
        assert_eq!(loaded.disc.track(2).unwrap().index1_lba(), 4);
        assert_eq!(loaded.disc.track(3).unwrap().index1_lba(), 7);
        assert_eq!(loaded.disc.leadout_lba(), 10);
        assert_eq!(loaded.disc.read_user_sector(4).unwrap()[0], 0x27);
        assert_eq!(loaded.disc.read_user_sector(7).unwrap()[0], 0x35);
    }

    #[test]
    fn portable_references_reject_unsafe_and_colliding_forms() {
        for value in [
            "",
            "/a.bin",
            "\\\\server\\a.bin",
            "C:\\a.bin",
            "a:stream",
            "../a.bin",
            "a/./b.bin",
            "a//b.bin",
            "a.bin/",
            "a\0.bin",
        ] {
            assert!(
                normalize_portable_path(value).is_err(),
                "accepted {value:?}"
            );
        }
        assert_eq!(normalize_portable_path("a\\b.bin").unwrap(), "a/b.bin");

        let cue = "FILE \"A.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"a.BIN\" BINARY\nTRACK 02 MODE1/2048\nINDEX 01 00:00:00\n";
        assert_eq!(parse_cue(cue), Err(PceCdLoadError::DuplicateFile));
    }

    #[test]
    fn cue_validation_accepts_audio_and_rejects_missing_indices_and_track_overflow() {
        let audio = "FILE \"disc.bin\" BINARY\nTRACK 01 AUDIO\nINDEX 01 00:00:00\n";
        assert_eq!(parse_cue(audio).unwrap().tracks[0].mode, CdTrackMode::Audio);
        let missing = "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2352\n";
        let sheet = parse_cue(missing).unwrap();
        assert!(matches!(
            build_disc(vec![], &sheet, vec![vec![0; 2_352]]),
            Err(PceCdLoadError::MissingIndex1(1))
        ));

        let mut maximum = String::from("FILE \"disc.bin\" BINARY\n");
        for track in 1..=99 {
            let lba = track - 1;
            maximum.push_str(&format!(
                "TRACK {track:02} MODE1/2352\nINDEX 01 00:{:02}:{:02}\n",
                lba / 75,
                lba % 75,
            ));
        }
        maximum.push_str("TRACK 99 MODE1/2352\nINDEX 01 00:01:39\n");
        assert_eq!(parse_cue(&maximum), Err(PceCdLoadError::InvalidTrackOrder));
    }

    #[test]
    fn canonical_identity_is_length_framed_and_reference_ordered() {
        let cue = b"FILE \"a.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
        let sheet = parse_cue_bytes(cue).unwrap();
        let first = build_disc(cue.to_vec(), &sheet, vec![vec![1; 2_048]]).unwrap();
        let second = build_disc(cue.to_vec(), &sheet, vec![vec![2; 2_048]]).unwrap();
        assert_ne!(first.content_sha256, second.content_sha256);
        assert_ne!(first.content_crc32, second.content_crc32);
    }

    #[test]
    fn normalized_source_identity_uses_the_exact_built_track_layout() {
        let cue = b"FILE \"one.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 00 00:00:00\nINDEX 01 00:00:01\nTRACK 02 MODE1/2048\nPREGAP 00:00:01\nINDEX 01 00:00:03\nFILE \"two.bin\" BINARY\nTRACK 03 AUDIO\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
        let sheet = parse_cue_bytes(cue).unwrap();
        let files = vec![vec![0x11; 5 * 2_048], vec![0x22; 3 * 2_352]];
        let layout = cue_track_layout(&sheet, &[files[0].len(), files[1].len()]).unwrap();
        assert_eq!(layout[0][0].index0, None);
        assert_eq!(layout[0][0].index1, 0);
        assert_eq!(layout[0][0].source_bytes, 2_048..3 * 2_048);
        assert_eq!(layout[0][1].index0, Some(2));
        assert_eq!(layout[0][1].index1, 3);
        assert_eq!(layout[0][1].stored_start, 3);
        assert_eq!(layout[0][1].source_bytes, 3 * 2_048..5 * 2_048);
        assert_eq!(layout[1][0].index0, Some(5));
        assert_eq!(layout[1][0].index1, 6);
        assert_eq!(layout[1][0].source_bytes, 0..3 * 2_352);
        let source_hash = normalized_disc_identity(&sheet, &files).unwrap();
        let loaded = build_disc(cue.to_vec(), &sheet, files).unwrap();
        assert_eq!(source_hash, loaded.disc.content_hash());
        assert_eq!(source_hash, loaded.source_disc_sha256);
    }

    #[test]
    fn chd_track_metadata_preserves_embedded_pregap_and_rejects_virtual_pregap() {
        let track = parse_chd_track_metadata(
            b"TRACK:2 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:3 PREGAP:1 PGTYPE:VMODE1_RAW PGSUB:NONE POSTGAP:0",
        )
        .unwrap();
        assert_eq!(track.number, 2);
        assert_eq!(track.mode, CdTrackMode::Mode1_2352);
        assert_eq!(track.frames, 3);
        assert_eq!(track.pregap, 1);
        assert_eq!(
            parse_chd_track_metadata(
                b"TRACK:2 TYPE:MODE1_RAW SUBTYPE:NONE FRAMES:3 PREGAP:1 PGTYPE:MODE1_RAW PGSUB:NONE POSTGAP:0",
            ),
            Err(PceCdLoadError::UnsupportedChdPregap(2))
        );
    }

    #[test]
    fn resized_chd_track_payload_updates_the_reconstructed_toc() {
        let mut tracks = vec![ChdTrack {
            number: 1,
            mode: CdTrackMode::Audio,
            frames: 1,
            pregap: 0,
        }];
        let payloads = vec![vec![0; 3 * 2_352]];

        refresh_chd_track_lengths(&mut tracks, &payloads).unwrap();
        let disc = build_chd_disc(&tracks, &payloads).unwrap();

        assert_eq!(tracks[0].frames, 3);
        assert_eq!(disc.tracks()[0].end_lba(), 3);
    }

    #[test]
    fn reconstructed_chd_tracks_match_cue_disc_and_mod_identity() {
        let cue = b"FILE \"one.bin\" BINARY\nTRACK 01 MODE1/2352\nINDEX 01 00:00:00\nFILE \"two.bin\" BINARY\nTRACK 02 AUDIO\nINDEX 00 00:00:00\nINDEX 01 00:00:01\n";
        let sheet = parse_cue_bytes(cue).unwrap();
        let first = vec![0x11; 2 * 2_352];
        let second = vec![0x22; 2 * 2_352];
        let loaded = build_disc(cue.to_vec(), &sheet, vec![first.clone(), second.clone()]).unwrap();
        let tracks = vec![
            ChdTrack {
                number: 1,
                mode: CdTrackMode::Mode1_2352,
                frames: 2,
                pregap: 0,
            },
            ChdTrack {
                number: 2,
                mode: CdTrackMode::Audio,
                frames: 2,
                pregap: 1,
            },
        ];
        let chd_disc = build_chd_disc(&tracks, &[first, second]).unwrap();
        assert_eq!(chd_disc, loaded.disc);
        assert_eq!(crc32fast::hash(&chd_disc.content_hash()), loaded.mod_crc32);
    }

    #[test]
    fn chd_audio_payload_is_normalized_for_the_cd_audio_reader() {
        let tracks = vec![ChdTrack {
            number: 1,
            mode: CdTrackMode::Audio,
            frames: 1,
            pregap: 0,
        }];
        let payload = vec![0x12, 0x34, 0x56, 0x78]
            .into_iter()
            .cycle()
            .take(2_352)
            .collect::<Vec<_>>();
        let mut payloads = vec![payload];
        normalize_chd_audio_payloads(&tracks, &mut payloads);
        let disc = build_chd_disc(&tracks, &payloads).unwrap();
        assert_eq!(disc.read_audio_sample(0, 0), Ok((0x1234, 0x5678)));
    }

    #[test]
    fn chd_audio_xdelta_targets_the_normalized_track_payload() {
        let tracks = vec![ChdTrack {
            number: 1,
            mode: CdTrackMode::Audio,
            frames: 1,
            pregap: 0,
        }];
        let raw = vec![0x12, 0x34, 0x56, 0x78]
            .into_iter()
            .cycle()
            .take(2_352)
            .collect::<Vec<_>>();
        let mut payloads = vec![raw];
        normalize_chd_audio_payloads(&tracks, &mut payloads);
        let source = payloads[0].clone();
        let mut expected = source.clone();
        expected[..4].copy_from_slice(&[0xCD, 0xAB, 0x34, 0x12]);

        let dir =
            std::env::temp_dir().join(format!("zeff-pce-chd-audio-xdelta-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("translation-track01.xdelta"),
            xdelta3::encode(&expected, &source).unwrap(),
        )
        .unwrap();
        let targets = vec![crate::mods::PceCdPatchTarget::Track {
            number: 1,
            segment: 0,
            bytes: 0..payloads[0].len(),
        }];
        let entries = vec![crate::mods::ModEntry {
            filename: "translation-track01.xdelta".to_owned(),
            enabled: true,
            target: None,
        }];

        assert!(
            crate::mods::apply_enabled_pce_cd_mods(&mut payloads, &targets, &dir, &entries)
                .is_empty()
        );
        assert_eq!(payloads[0], expected);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn direct_loader_mounts_cd_with_test_system_card() {
        let bin = vec![0; 2_048];
        let cue = "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
        let cue_path = temp_set("loader", &[("disc.bin", &bin)], cue);
        let system_card: &'static [u8] = Box::leak(vec![0; 262_144].into_boxed_slice());
        let config = BackendLoadConfig {
            pce_cd_system_card_override: Some(system_card),
            pce_cd_system_card_sha256_override: Some(zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256),
            pce_console_wiring: Some(zeff_pce_core::hardware::PceConsoleWiring::TurboGrafx16),
            ..BackendLoadConfig::default()
        };
        let loaded =
            load_backend_from_rom_source(ActiveSystem::Pce, &cue_path, &cue_path, None, config)
                .unwrap();
        let EmuBackend::Pce(backend) = loaded.backend else {
            panic!("CUE loader returned a non-PCE backend");
        };
        assert_eq!(
            backend.hucard_board(),
            zeff_pce_core::hardware::PceHuCardBoard::SystemCardV3
        );
        assert_eq!(backend.source_path(), cue_path);
    }

    #[test]
    fn direct_iso_route_mounts_the_referencing_cue() {
        let data = vec![0; 2_048];
        let cue = "FILE \"disc.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
        let cue_path = temp_set("iso-loader", &[("disc.iso", &data)], cue);
        let iso_path = cue_path.with_extension("iso");
        let system_card: &'static [u8] = Box::leak(vec![0; 262_144].into_boxed_slice());
        let loaded = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &iso_path,
            &iso_path,
            None,
            BackendLoadConfig {
                pce_cd_system_card_override: Some(system_card),
                pce_cd_system_card_sha256_override: Some(
                    zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256,
                ),
                pce_console_wiring: Some(zeff_pce_core::hardware::PceConsoleWiring::TurboGrafx16),
                ..BackendLoadConfig::default()
            },
        )
        .unwrap();
        let EmuBackend::Pce(backend) = loaded.backend else {
            panic!("ISO loader returned a non-PCE backend");
        };
        assert_eq!(backend.rom_path(), cue_path);
        assert_eq!(backend.source_path(), iso_path);
    }

    #[test]
    fn direct_loader_rejects_exact_system_card_from_the_wrong_region() {
        let bin = vec![0; 2_048];
        let cue = "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n";
        let cue_path = temp_set("wrong-region", &[("disc.bin", &bin)], cue);
        let system_card: &'static [u8] = Box::leak(vec![0; 262_144].into_boxed_slice());
        let error = load_backend_from_rom_source(
            ActiveSystem::Pce,
            &cue_path,
            &cue_path,
            None,
            BackendLoadConfig {
                pce_cd_system_card_override: Some(system_card),
                pce_cd_system_card_sha256_override: Some(
                    zeff_firmware::PCE_SYSTEM_CARD_V3_USA_SHA256,
                ),
                pce_console_wiring: Some(zeff_pce_core::hardware::PceConsoleWiring::PcEngine),
                ..BackendLoadConfig::default()
            },
        )
        .err()
        .unwrap()
        .downcast::<PceCdLoadError>()
        .unwrap();
        assert_eq!(
            error,
            PceCdLoadError::SystemCardRegionMismatch {
                expected: zeff_firmware::PceSystemCardRegion::Japan,
                actual: zeff_firmware::PceSystemCardRegion::Usa,
            }
        );
    }
}
