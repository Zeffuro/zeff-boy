use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crc32fast::Hasher as Crc32;
use sha2::{Digest, Sha256};
use zeff_pce_core::hardware::{CdDisc, CdSourceError, CdTrack, CdTrackMode, CdTrackSource};

use super::pce_cd::{
    CONTENT_ID_DOMAIN, CueSheet, LoadedPceCd, PCE_CD_DATA_BYTES_LIMIT, PceCdLoadError,
    cue_track_layout, resolve_direct_file_reference,
};
use super::pce_cd_overlay::{
    PatchOverlayBuilder, PatchOverlayStack, apply_ppf_byte_slices_stack, apply_ppf_bytes_stack,
    apply_ppf_stack, log_ppf_overlay, slice_source,
};

const HASH_BUFFER_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const WINDOWS_REPARSE_POINT: u32 = 0x400;

mod archive_ppf;
mod source_identity;
pub(super) use archive_ppf::try_load_cached_cue_ppf_overlay_byte_slices;
pub(crate) use source_identity::direct_file_sha256;
use source_identity::disc_payload_len;

struct FileBackedCueFile {
    path: PathBuf,
    bytes: usize,
    identity: FileIdentity,
    expected_sha256: Option<[u8; 32]>,
    reject_reparse: bool,
}

#[derive(Clone)]
pub(super) struct CueFileSource {
    pub(super) path: PathBuf,
    pub(super) bytes: u64,
    pub(super) sha256: [u8; 32],
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    bytes: u64,
    modified: Option<std::time::SystemTime>,
}

pub(super) fn load_direct_cue_file_backed(
    cue_path: &Path,
    cue_bytes: &[u8],
    sheet: &CueSheet,
) -> Result<LoadedPceCd, PceCdLoadError> {
    let files = open_files(cue_path, sheet)?;
    load_cue_file_backed(cue_bytes, sheet, files, |_| Ok(()))
}

pub(super) fn load_cached_cue_file_backed(
    cue_bytes: &[u8],
    sheet: &CueSheet,
    sources: Vec<CueFileSource>,
    progress: impl FnMut(u64) -> Result<(), PceCdLoadError>,
) -> Result<LoadedPceCd, PceCdLoadError> {
    let files = cached_files(sources)?;
    load_cue_file_backed(cue_bytes, sheet, files, progress)
}

pub(super) fn try_load_direct_cue_ppf_overlay(
    cue_path: &Path,
    sheet: &CueSheet,
    dir: &Path,
    mods: &[crate::mods::ModEntry],
) -> Result<Option<CdDisc>, PceCdLoadError> {
    let files = open_files(cue_path, sheet)?;
    try_build_ppf_overlay_disc(sheet, &files, |builder| apply_ppf_stack(builder, dir, mods))
}

pub(super) fn try_load_direct_cue_ppf_overlay_bytes(
    cue_path: &Path,
    sheet: &CueSheet,
    patches: &[(String, Vec<u8>)],
) -> Result<Option<CdDisc>, PceCdLoadError> {
    let files = open_files(cue_path, sheet)?;
    try_build_ppf_overlay_disc(sheet, &files, |builder| {
        apply_ppf_bytes_stack(builder, patches)
    })
}

pub(super) fn try_load_cached_cue_ppf_overlay(
    sheet: &CueSheet,
    sources: Vec<CueFileSource>,
    dir: &Path,
    mods: &[crate::mods::ModEntry],
) -> Result<Option<CdDisc>, PceCdLoadError> {
    let files = cached_files(sources)?;
    try_build_ppf_overlay_disc(sheet, &files, |builder| apply_ppf_stack(builder, dir, mods))
}

fn cached_files(sources: Vec<CueFileSource>) -> Result<Vec<FileBackedCueFile>, PceCdLoadError> {
    let mut files = Vec::with_capacity(sources.len());
    for source in sources {
        let metadata =
            std::fs::symlink_metadata(&source.path).map_err(|_| PceCdLoadError::ArchiveChanged)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata_is_reparse_point(&metadata)
            || metadata.len() != source.bytes
        {
            return Err(PceCdLoadError::ArchiveChanged);
        }
        files.push(FileBackedCueFile {
            path: source.path,
            bytes: usize::try_from(source.bytes)
                .map_err(|_| PceCdLoadError::DataTooLarge(source.bytes))?,
            identity: FileIdentity::from_metadata(&metadata),
            expected_sha256: Some(source.sha256),
            reject_reparse: true,
        });
    }
    Ok(files)
}

fn load_cue_file_backed(
    cue_bytes: &[u8],
    sheet: &CueSheet,
    files: Vec<FileBackedCueFile>,
    progress: impl FnMut(u64) -> Result<(), PceCdLoadError>,
) -> Result<LoadedPceCd, PceCdLoadError> {
    let (content_sha256, content_crc32, file_sha256) =
        content_identity(cue_bytes, sheet, &files, progress)?;
    let disc = build_disc(sheet, &files, &file_sha256)?;
    let source_disc_sha256 = disc.content_hash();
    let raw_source_media_len = disc_payload_len(&disc)?;
    Ok(LoadedPceCd {
        raw_source_media_sha256: source_disc_sha256,
        raw_source_media_len,
        disc,
        content_sha256,
        content_crc32,
        mod_crc32: crc32fast::hash(&source_disc_sha256),
        source_disc_sha256,
    })
}

fn try_build_ppf_overlay_disc(
    sheet: &CueSheet,
    files: &[FileBackedCueFile],
    apply: impl FnOnce(&mut PatchOverlayBuilder) -> PatchOverlayStack,
) -> Result<Option<CdDisc>, PceCdLoadError> {
    let sources = full_file_sources(sheet, files)?;
    let Some(mut builder) = PatchOverlayBuilder::for_tracks(&sources) else {
        return Ok(None);
    };
    let PatchOverlayStack::Applied(applied) = apply(&mut builder) else {
        return Ok(None);
    };
    let Some(sources) = builder.finish_tracks(sources) else {
        return Ok(None);
    };
    let disc = build_disc_from_raw_sources(sheet, files, &sources)?;
    log_ppf_overlay(&applied);
    Ok(Some(disc))
}

fn full_file_sources(
    sheet: &CueSheet,
    files: &[FileBackedCueFile],
) -> Result<Vec<Arc<dyn CdTrackSource>>, PceCdLoadError> {
    if files.len() != sheet.files.len() {
        return Err(PceCdLoadError::MissingFile);
    }
    let mut sources = Vec::with_capacity(files.len());
    for (cue_file, file) in sheet.files.iter().zip(files) {
        let tracks = cue_file
            .track_indices
            .iter()
            .map(|&index| &sheet.tracks[index])
            .collect::<Vec<_>>();
        let file_sector_bytes = sector_bytes(tracks[0].mode);
        if tracks
            .iter()
            .any(|track| sector_bytes(track.mode) != file_sector_bytes)
        {
            return Err(PceCdLoadError::MixedSectorSizes);
        }
        if !file.bytes.is_multiple_of(file_sector_bytes) {
            return Err(PceCdLoadError::MisalignedBin {
                bytes: file.bytes,
                sector_bytes: file_sector_bytes,
            });
        }
        let source: Arc<dyn CdTrackSource> = FileSliceSource::open_unverified(
            &file.path,
            file.identity,
            0,
            file.bytes,
            file_sector_bytes,
            file.reject_reparse,
        )?;
        sources.push(source);
    }
    Ok(sources)
}

fn open_files(cue_path: &Path, sheet: &CueSheet) -> Result<Vec<FileBackedCueFile>, PceCdLoadError> {
    let parent = cue_path.parent().unwrap_or_else(|| Path::new(""));
    let mut total = 0_u64;
    let mut files = Vec::with_capacity(sheet.files.len());
    for cue_file in &sheet.files {
        let path = resolve_direct_file_reference(parent, &cue_file.reference)?;
        let metadata =
            std::fs::metadata(&path).map_err(|_| PceCdLoadError::BinUnreadable(path.clone()))?;
        total = total
            .checked_add(metadata.len())
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))?;
        if total > PCE_CD_DATA_BYTES_LIMIT as u64 {
            return Err(PceCdLoadError::DataTooLarge(total));
        }
        files.push(FileBackedCueFile {
            bytes: usize::try_from(metadata.len())
                .map_err(|_| PceCdLoadError::DataTooLarge(metadata.len()))?,
            identity: FileIdentity::from_metadata(&metadata),
            expected_sha256: None,
            reject_reparse: false,
            path,
        });
    }
    Ok(files)
}

fn content_identity(
    cue_bytes: &[u8],
    sheet: &CueSheet,
    files: &[FileBackedCueFile],
    mut progress: impl FnMut(u64) -> Result<(), PceCdLoadError>,
) -> Result<([u8; 32], u32, Vec<[u8; 32]>), PceCdLoadError> {
    let mut sha = Sha256::new();
    let mut crc = Crc32::new();
    update_identity(&mut sha, &mut crc, CONTENT_ID_DOMAIN);
    update_identity(&mut sha, &mut crc, cue_bytes);
    let count = (sheet.files.len() as u64).to_le_bytes();
    sha.update(count);
    crc.update(&count);
    let mut file_sha256 = Vec::with_capacity(files.len());
    for (cue_file, file) in sheet.files.iter().zip(files) {
        update_identity(&mut sha, &mut crc, cue_file.reference.as_bytes());
        file_sha256.push(update_file_identity(
            &mut sha,
            &mut crc,
            file,
            &mut progress,
        )?);
    }
    Ok((sha.finalize().into(), crc.finalize(), file_sha256))
}

fn update_file_identity(
    sha: &mut Sha256,
    crc: &mut Crc32,
    source: &FileBackedCueFile,
    progress: &mut impl FnMut(u64) -> Result<(), PceCdLoadError>,
) -> Result<[u8; 32], PceCdLoadError> {
    let bytes = (source.bytes as u64).to_le_bytes();
    sha.update(bytes);
    crc.update(&bytes);
    let mut file = open_checked(&source.path, source.identity, source.reject_reparse)?;
    let mut member_sha = Sha256::new();
    let mut buffer = [0; HASH_BUFFER_BYTES];
    let mut remaining = source.bytes;
    while remaining != 0 {
        let count = remaining.min(buffer.len());
        file.read_exact(&mut buffer[..count])
            .map_err(|_| PceCdLoadError::BinUnreadable(source.path.clone()))?;
        sha.update(&buffer[..count]);
        crc.update(&buffer[..count]);
        member_sha.update(&buffer[..count]);
        remaining -= count;
        progress(count as u64)?;
    }
    let member_sha = member_sha.finalize().into();
    if source
        .expected_sha256
        .is_some_and(|expected| expected != member_sha)
    {
        return Err(PceCdLoadError::ArchiveChanged);
    }
    Ok(member_sha)
}

fn build_disc(
    sheet: &CueSheet,
    files: &[FileBackedCueFile],
    file_sha256: &[[u8; 32]],
) -> Result<CdDisc, PceCdLoadError> {
    build_disc_with_raw_sources(sheet, files, Some(file_sha256), None)
}

fn build_disc_from_raw_sources(
    sheet: &CueSheet,
    files: &[FileBackedCueFile],
    sources: &[Arc<dyn CdTrackSource>],
) -> Result<CdDisc, PceCdLoadError> {
    build_disc_with_raw_sources(sheet, files, None, Some(sources))
}

fn build_disc_with_raw_sources(
    sheet: &CueSheet,
    files: &[FileBackedCueFile],
    file_sha256: Option<&[[u8; 32]]>,
    raw_sources: Option<&[Arc<dyn CdTrackSource>]>,
) -> Result<CdDisc, PceCdLoadError> {
    if files.len() != sheet.files.len() {
        return Err(PceCdLoadError::MissingFile);
    }
    if raw_sources.is_some_and(|sources| sources.len() != files.len()) {
        return Err(PceCdLoadError::MissingFile);
    }
    if file_sha256.is_some_and(|hashes| hashes.len() != files.len()) {
        return Err(PceCdLoadError::MissingFile);
    }
    let file_bytes = files.iter().map(|file| file.bytes).collect::<Vec<_>>();
    let layout = cue_track_layout(sheet, &file_bytes)?;
    let mut normalized = Vec::with_capacity(sheet.tracks.len());
    for file_layout in layout {
        for track_layout in file_layout {
            let track = track_layout.track;
            let file_index = track.file_index;
            let file = &files[file_index];
            let start = track_layout.source_bytes.start;
            let bytes = track_layout.source_bytes.len();
            let sector_bytes = sector_bytes(track.mode);
            let source: Arc<dyn CdTrackSource> = if let Some(sources) = raw_sources {
                slice_source(sources[file_index].clone(), start, bytes)
                    .ok_or(PceCdLoadError::TrackOutsideBin(track.number))?
            } else if start == 0 && bytes == file.bytes {
                FileSliceSource::open_prehashed(
                    &file.path,
                    file.identity,
                    bytes,
                    sector_bytes,
                    file.reject_reparse,
                    file_sha256
                        .and_then(|hashes| hashes.get(file_index))
                        .copied()
                        .ok_or(PceCdLoadError::MissingFile)?,
                )?
            } else {
                FileSliceSource::open(
                    &file.path,
                    file.identity,
                    start,
                    bytes,
                    sector_bytes,
                    file.reject_reparse,
                )?
            };
            let track = if track_layout.virtual_pregap {
                if raw_sources.is_some() {
                    CdTrack::from_index1_unverified_source(
                        track.number,
                        track_layout.control(),
                        track_layout.index0,
                        track_layout.index1,
                        track.mode,
                        source,
                    )
                } else {
                    CdTrack::from_index1_source(
                        track.number,
                        track_layout.control(),
                        track_layout.index0,
                        track_layout.index1,
                        track.mode,
                        source,
                    )
                }
            } else if raw_sources.is_some() {
                CdTrack::from_stored_unverified_source(
                    track.number,
                    track_layout.control(),
                    track_layout.index0,
                    track_layout.index1,
                    track.mode,
                    source,
                )
            } else {
                CdTrack::from_stored_source(
                    track.number,
                    track_layout.control(),
                    track_layout.index0,
                    track_layout.index1,
                    track.mode,
                    source,
                )
            }
            .map_err(|error| PceCdLoadError::Disc(error.to_string()))?;
            normalized.push(track);
        }
    }
    CdDisc::new(normalized).map_err(|error| PceCdLoadError::Disc(error.to_string()))
}

struct FileSliceSource {
    reader: Mutex<FileSliceReader>,
    bytes: usize,
    payload_hash: [u8; 32],
    sector_bytes: usize,
    reject_reparse: bool,
    #[cfg(test)]
    reads: std::sync::atomic::AtomicUsize,
}

struct FileSliceReader {
    file: File,
    identity: FileIdentity,
    start: u64,
    cached_sector: Option<usize>,
    cache: [u8; 2_352],
}

#[derive(Clone, Copy)]
struct FileSliceSpec {
    start: usize,
    bytes: usize,
    sector_bytes: usize,
}

#[derive(Clone, Copy)]
enum FileSliceHash {
    Compute,
    Precomputed([u8; 32]),
    Unverified,
}

impl FileSliceSource {
    fn open(
        path: &Path,
        identity: FileIdentity,
        start: usize,
        bytes: usize,
        sector_bytes: usize,
        reject_reparse: bool,
    ) -> Result<Arc<Self>, PceCdLoadError> {
        Self::open_with_hash(
            path,
            identity,
            FileSliceSpec {
                start,
                bytes,
                sector_bytes,
            },
            reject_reparse,
            FileSliceHash::Compute,
        )
    }

    fn open_unverified(
        path: &Path,
        identity: FileIdentity,
        start: usize,
        bytes: usize,
        sector_bytes: usize,
        reject_reparse: bool,
    ) -> Result<Arc<Self>, PceCdLoadError> {
        Self::open_with_hash(
            path,
            identity,
            FileSliceSpec {
                start,
                bytes,
                sector_bytes,
            },
            reject_reparse,
            FileSliceHash::Unverified,
        )
    }

    fn open_prehashed(
        path: &Path,
        identity: FileIdentity,
        bytes: usize,
        sector_bytes: usize,
        reject_reparse: bool,
        payload_hash: [u8; 32],
    ) -> Result<Arc<Self>, PceCdLoadError> {
        Self::open_with_hash(
            path,
            identity,
            FileSliceSpec {
                start: 0,
                bytes,
                sector_bytes,
            },
            reject_reparse,
            FileSliceHash::Precomputed(payload_hash),
        )
    }

    fn open_with_hash(
        path: &Path,
        identity: FileIdentity,
        spec: FileSliceSpec,
        reject_reparse: bool,
        payload_hash: FileSliceHash,
    ) -> Result<Arc<Self>, PceCdLoadError> {
        let FileSliceSpec {
            start,
            bytes,
            sector_bytes,
        } = spec;
        let mut file = open_checked(path, identity, reject_reparse)?;
        let start = u64::try_from(start).map_err(|_| PceCdLoadError::BinUnreadable(path.into()))?;
        start
            .checked_add(bytes as u64)
            .filter(|&end| end <= identity.bytes)
            .ok_or_else(|| PceCdLoadError::BinUnreadable(path.into()))?;
        let mut hasher = Sha256::new();
        if matches!(payload_hash, FileSliceHash::Compute) {
            file.seek(SeekFrom::Start(start))
                .map_err(|_| PceCdLoadError::BinUnreadable(path.into()))?;
            let mut remaining = bytes;
            let mut buffer = [0; HASH_BUFFER_BYTES];
            while remaining != 0 {
                let count = remaining.min(buffer.len());
                file.read_exact(&mut buffer[..count])
                    .map_err(|_| PceCdLoadError::BinUnreadable(path.into()))?;
                hasher.update(&buffer[..count]);
                remaining -= count;
            }
        }
        let payload_hash = match payload_hash {
            FileSliceHash::Compute => hasher.finalize().into(),
            FileSliceHash::Precomputed(payload_hash) => payload_hash,
            FileSliceHash::Unverified => [0; 32],
        };
        Ok(Arc::new(Self {
            reader: Mutex::new(FileSliceReader {
                file,
                identity,
                start,
                cached_sector: None,
                cache: [0; 2_352],
            }),
            bytes,
            payload_hash,
            sector_bytes,
            reject_reparse,
            #[cfg(test)]
            reads: std::sync::atomic::AtomicUsize::new(0),
        }))
    }

    fn fill_sector(
        &self,
        reader: &mut FileSliceReader,
        sector: usize,
    ) -> Result<(), CdSourceError> {
        if reader.cached_sector == Some(sector) {
            return Ok(());
        }
        let offset = reader
            .start
            .checked_add((sector * self.sector_bytes) as u64)
            .ok_or(CdSourceError::ReadFailed)?;
        reader
            .file
            .seek(SeekFrom::Start(offset))
            .and_then(|_| {
                reader
                    .file
                    .read_exact(&mut reader.cache[..self.sector_bytes])
            })
            .map_err(|_| CdSourceError::ReadFailed)?;
        reader.cached_sector = Some(sector);
        #[cfg(test)]
        self.reads
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    fn verify_identity(&self, reader: &FileSliceReader) -> Result<(), CdSourceError> {
        let metadata = reader
            .file
            .metadata()
            .map_err(|_| CdSourceError::ReadFailed)?;
        (metadata.is_file()
            && (!self.reject_reparse || !metadata_is_reparse_point(&metadata))
            && FileIdentity::from_metadata(&metadata) == reader.identity)
            .then_some(())
            .ok_or(CdSourceError::ReadFailed)
    }

    #[cfg(test)]
    fn reset_cache_for_test(&self) {
        self.reader.lock().unwrap().cached_sector = None;
        self.reads.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    #[cfg(test)]
    fn read_count(&self) -> usize {
        self.reads.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl CdTrackSource for FileSliceSource {
    fn len(&self) -> usize {
        self.bytes
    }

    fn payload_hash(&self) -> [u8; 32] {
        self.payload_hash
    }

    fn read_exact_at(&self, offset: usize, buffer: &mut [u8]) -> Result<(), CdSourceError> {
        let end = offset
            .checked_add(buffer.len())
            .filter(|&end| end <= self.bytes)
            .ok_or(CdSourceError::OutOfRange {
                offset,
                bytes: buffer.len(),
                source_len: self.bytes,
            })?;
        let mut reader = self.reader.lock().map_err(|_| CdSourceError::ReadFailed)?;
        self.verify_identity(&reader)?;
        let mut source_offset = offset;
        let mut destination_offset = 0;
        while source_offset != end {
            let sector = source_offset / self.sector_bytes;
            self.fill_sector(&mut reader, sector)?;
            let within_sector = source_offset % self.sector_bytes;
            let count = (self.sector_bytes - within_sector).min(end - source_offset);
            buffer[destination_offset..destination_offset + count]
                .copy_from_slice(&reader.cache[within_sector..within_sector + count]);
            source_offset += count;
            destination_offset += count;
        }
        Ok(())
    }

    fn visit_payload(
        &self,
        sector_bytes: usize,
        visitor: &mut dyn FnMut(&[u8]),
    ) -> Result<(), CdSourceError> {
        if sector_bytes != self.sector_bytes || !self.bytes.is_multiple_of(sector_bytes) {
            return Err(CdSourceError::ReadFailed);
        }
        let mut reader = self.reader.lock().map_err(|_| CdSourceError::ReadFailed)?;
        self.verify_identity(&reader)?;
        reader.cached_sector = None;
        let start = reader.start;
        reader
            .file
            .seek(SeekFrom::Start(start))
            .map_err(|_| CdSourceError::ReadFailed)?;

        let chunk_bytes = (HASH_BUFFER_BYTES / sector_bytes).max(1) * sector_bytes;
        let mut buffer = vec![0; chunk_bytes.min(self.bytes)];
        let mut remaining = self.bytes;
        while remaining != 0 {
            let count = remaining.min(buffer.len());
            reader
                .file
                .read_exact(&mut buffer[..count])
                .map_err(|_| CdSourceError::ReadFailed)?;
            visitor(&buffer[..count]);
            remaining -= count;
        }
        self.verify_identity(&reader)
    }
}

impl FileIdentity {
    fn from_metadata(metadata: &std::fs::Metadata) -> Self {
        Self {
            bytes: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }
}

fn open_checked(
    path: &Path,
    identity: FileIdentity,
    reject_reparse: bool,
) -> Result<File, PceCdLoadError> {
    let file = File::open(path).map_err(|_| PceCdLoadError::BinUnreadable(path.into()))?;
    let metadata = file
        .metadata()
        .map_err(|_| PceCdLoadError::BinUnreadable(path.into()))?;
    (metadata.is_file()
        && (!reject_reparse || !metadata_is_reparse_point(&metadata))
        && FileIdentity::from_metadata(&metadata) == identity)
        .then_some(file)
        .ok_or_else(|| PceCdLoadError::BinUnreadable(path.into()))
}

fn update_identity(sha: &mut Sha256, crc: &mut Crc32, bytes: &[u8]) {
    let len = (bytes.len() as u64).to_le_bytes();
    sha.update(len);
    sha.update(bytes);
    crc.update(&len);
    crc.update(bytes);
}

fn sector_bytes(mode: CdTrackMode) -> usize {
    match mode {
        CdTrackMode::Mode1_2048 => 2_048,
        CdTrackMode::Mode1_2352 | CdTrackMode::Audio => 2_352,
    }
}

#[cfg(windows)]
fn metadata_is_reparse_point(metadata: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;

    metadata.file_attributes() & WINDOWS_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn metadata_is_reparse_point(_metadata: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests;
