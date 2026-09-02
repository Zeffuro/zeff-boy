#![cfg(not(target_arch = "wasm32"))]

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crc32fast::Hasher as Crc32;
use sha2::{Digest, Sha256};
use zeff_pce_core::hardware::{CdDisc, CdTrack, CdTrackMode};

mod cue;
mod tas_ppf;
#[cfg(test)]
#[path = "pce_cd/tests.rs"]
mod tests;

#[cfg(test)]
use cue::parse_cue;

pub(crate) use tas_ppf::PceCdTasPpfStack;

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
    pub(crate) raw_source_media_sha256: [u8; 32],
    pub(crate) raw_source_media_len: usize,
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

fn disc_payload_len(disc: &CdDisc) -> Result<usize, PceCdLoadError> {
    disc.tracks().iter().try_fold(0_usize, |total, track| {
        let sector_bytes = match track.mode() {
            CdTrackMode::Mode1_2048 => zeff_pce_core::hardware::CD_USER_SECTOR_BYTES,
            CdTrackMode::Mode1_2352 | CdTrackMode::Audio => {
                zeff_pce_core::hardware::CD_RAW_SECTOR_BYTES
            }
        };
        let bytes = usize::try_from(track.sector_count())
            .map_err(|_| PceCdLoadError::DataTooLarge(u64::MAX))?
            .checked_mul(sector_bytes)
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))?;
        total
            .checked_add(bytes)
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))
    })
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
        raw_source_media_sha256: disc.content_hash(),
        raw_source_media_len: disc_payload_len(&disc)?,
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
        raw_source_media_sha256: source_disc_sha256,
        raw_source_media_len: disc_payload_len(&disc)?,
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

pub(super) use cue::{cue_track_layout, normalize_portable_path, parse_cue_bytes};
use cue::{portable_path, sector_bytes};
