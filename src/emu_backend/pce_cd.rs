#![cfg(not(target_arch = "wasm32"))]

use std::error::Error;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};

use crc32fast::Hasher as Crc32;
use sha2::{Digest, Sha256};
use zeff_pce_core::hardware::{CdDisc, CdTrack, CdTrackMode};

pub(crate) const PCE_CD_CUE_BYTES_LIMIT: usize = 1024 * 1024;
// Typical Redump sets store one track per FILE. Those buffers move directly into CdTrack, keeping
// retained payload near the source size. Multi-track FILEs can briefly retain a second span.
// An 80-minute raw CD can exceed 800 MiB once every 2,352-byte sector is retained.
pub(crate) const PCE_CD_DATA_BYTES_LIMIT: usize = 900 * 1024 * 1024;
const _: () = assert!(PCE_CD_DATA_BYTES_LIMIT >= 80 * 60 * 75 * 2_352);
pub(crate) const PCE_CD_FILE_REFERENCE_LIMIT: usize = 99;
pub(crate) const PCE_CD_PATH_BYTES_LIMIT: usize = 1024;
pub(crate) const PCE_CD_PATH_COMPONENT_BYTES_LIMIT: usize = 255;
pub(crate) const PCE_CD_PATH_DEPTH_LIMIT: usize = 16;

const CONTENT_ID_DOMAIN: &[u8] = b"zeff-boy:pce-cd-data:v2";

pub(crate) struct LoadedPceCd {
    pub(crate) disc: CdDisc,
    pub(crate) content_sha256: [u8; 32],
    pub(crate) content_crc32: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum PceCdLoadError {
    PackagedCdSetUnsupported,
    CueUnreadable(PathBuf),
    CueTooLarge(u64),
    CueNotUtf8,
    MissingFile,
    DuplicateFile,
    TooManyFileReferences,
    UnsafeFileReference(String),
    BinUnreadable(PathBuf),
    DataTooLarge(u64),
    UnsupportedFileType(String),
    UnsupportedTrackMode(String),
    MalformedLine(usize),
    DuplicateTrack(u8),
    MissingIndex1(u8),
    DuplicateIndex {
        track: u8,
        index: u8,
    },
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
            _ => write!(formatter, "unsupported PC Engine CD set: {self:?}"),
        }
    }
}

impl Error for PceCdLoadError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CueSheet {
    pub(super) files: Vec<CueFile>,
    tracks: Vec<CueTrack>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CueFile {
    pub(super) reference: String,
    track_indices: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CueTrack {
    number: u8,
    file_index: usize,
    mode: CdTrackMode,
    index0: Option<u32>,
    index1: Option<u32>,
}

pub(crate) fn load_direct_cue(cue_path: &Path) -> Result<LoadedPceCd, PceCdLoadError> {
    let cue_metadata = std::fs::metadata(cue_path)
        .map_err(|_| PceCdLoadError::CueUnreadable(cue_path.to_path_buf()))?;
    if cue_metadata.len() > PCE_CD_CUE_BYTES_LIMIT as u64 {
        return Err(PceCdLoadError::CueTooLarge(cue_metadata.len()));
    }
    let cue_bytes = std::fs::read(cue_path)
        .map_err(|_| PceCdLoadError::CueUnreadable(cue_path.to_path_buf()))?;
    let sheet = parse_cue_bytes(&cue_bytes)?;
    let parent = cue_path.parent().unwrap_or_else(|| Path::new(""));
    let mut data = Vec::with_capacity(sheet.files.len());
    let mut total = 0_u64;
    for file in &sheet.files {
        let path = parent.join(portable_path(&file.reference));
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
    build_disc(cue_bytes, &sheet, data)
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
            });
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
                if track.index1.is_some() {
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

pub(super) fn build_disc(
    cue_bytes: Vec<u8>,
    sheet: &CueSheet,
    files: Vec<Vec<u8>>,
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
    let mut cursor = 0_u32;
    let mut normalized = Vec::with_capacity(sheet.tracks.len());

    for (file_index, (cue_file, mut bytes)) in sheet.files.iter().zip(files).enumerate() {
        let file_tracks = cue_file
            .track_indices
            .iter()
            .map(|index| &sheet.tracks[*index])
            .collect::<Vec<_>>();
        let first_mode = file_tracks[0].mode;
        if file_tracks
            .iter()
            .any(|track| sector_bytes(track.mode) != sector_bytes(first_mode))
        {
            return Err(PceCdLoadError::MixedSectorSizes);
        }
        let sector_bytes = sector_bytes(first_mode);
        if !bytes.len().is_multiple_of(sector_bytes) {
            return Err(PceCdLoadError::MisalignedBin {
                bytes: bytes.len(),
                sector_bytes,
            });
        }
        let total_sectors = u32::try_from(bytes.len() / sector_bytes)
            .map_err(|_| PceCdLoadError::TrackOutsideBin(file_tracks[0].number))?;
        let anchor = if file_index == 0 {
            file_tracks[0]
                .index1
                .ok_or(PceCdLoadError::MissingIndex1(file_tracks[0].number))?
        } else {
            file_tracks[0]
                .index0
                .or(file_tracks[0].index1)
                .ok_or(PceCdLoadError::MissingIndex1(file_tracks[0].number))?
        };
        if anchor >= total_sectors {
            return Err(PceCdLoadError::TrackOutsideBin(file_tracks[0].number));
        }
        let base = cursor;
        for (track_offset, track) in file_tracks.iter().enumerate() {
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
            let index1 = raw_index1
                .checked_sub(anchor)
                .and_then(|index| base.checked_add(index))
                .ok_or(PceCdLoadError::InvalidTrackOrder)?;
            let index0 = track
                .index0
                .and_then(|index| index.checked_sub(anchor))
                .map(|index| {
                    base.checked_add(index)
                        .ok_or(PceCdLoadError::InvalidTrackOrder)
                })
                .transpose()?;
            let raw_stored_start = if index0.is_some() {
                track.index0.unwrap()
            } else {
                raw_index1
            };
            let start_byte = raw_stored_start as usize * sector_bytes;
            let end_byte = end as usize * sector_bytes;
            let track_bytes = if file_tracks.len() == 1 {
                bytes.drain(..start_byte);
                bytes.truncate(end_byte - start_byte);
                std::mem::take(&mut bytes)
            } else {
                bytes[start_byte..end_byte].to_vec()
            };
            normalized.push(
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
                    track_bytes,
                )
                .map_err(|error| PceCdLoadError::Disc(error.to_string()))?,
            );
        }
        cursor = cursor
            .checked_add(
                total_sectors
                    .checked_sub(anchor)
                    .ok_or(PceCdLoadError::InvalidTrackOrder)?,
            )
            .ok_or(PceCdLoadError::TrackOutsideBin(file_tracks[0].number))?;
    }

    Ok(LoadedPceCd {
        disc: CdDisc::new(normalized).map_err(|error| PceCdLoadError::Disc(error.to_string()))?,
        content_sha256,
        content_crc32,
    })
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
