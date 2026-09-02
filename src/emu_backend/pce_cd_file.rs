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
    PatchOverlayBuilder, PatchOverlayStack, apply_ppf_bytes_stack, apply_ppf_stack,
    log_ppf_overlay, slice_source,
};

const HASH_BUFFER_BYTES: usize = 64 * 1024;
#[cfg(windows)]
const WINDOWS_REPARSE_POINT: u32 = 0x400;

mod source_identity;
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
    let (content_sha256, content_crc32) = content_identity(cue_bytes, sheet, &files, progress)?;
    let disc = build_disc(sheet, &files)?;
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
) -> Result<([u8; 32], u32), PceCdLoadError> {
    let mut sha = Sha256::new();
    let mut crc = Crc32::new();
    update_identity(&mut sha, &mut crc, CONTENT_ID_DOMAIN);
    update_identity(&mut sha, &mut crc, cue_bytes);
    let count = (sheet.files.len() as u64).to_le_bytes();
    sha.update(count);
    crc.update(&count);
    for (cue_file, file) in sheet.files.iter().zip(files) {
        update_identity(&mut sha, &mut crc, cue_file.reference.as_bytes());
        update_file_identity(&mut sha, &mut crc, file, &mut progress)?;
    }
    Ok((sha.finalize().into(), crc.finalize()))
}

fn update_file_identity(
    sha: &mut Sha256,
    crc: &mut Crc32,
    source: &FileBackedCueFile,
    progress: &mut impl FnMut(u64) -> Result<(), PceCdLoadError>,
) -> Result<(), PceCdLoadError> {
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
    if source
        .expected_sha256
        .is_some_and(|expected| expected != <[u8; 32]>::from(member_sha.finalize()))
    {
        return Err(PceCdLoadError::ArchiveChanged);
    }
    Ok(())
}

fn build_disc(sheet: &CueSheet, files: &[FileBackedCueFile]) -> Result<CdDisc, PceCdLoadError> {
    build_disc_with_raw_sources(sheet, files, None)
}

fn build_disc_from_raw_sources(
    sheet: &CueSheet,
    files: &[FileBackedCueFile],
    sources: &[Arc<dyn CdTrackSource>],
) -> Result<CdDisc, PceCdLoadError> {
    build_disc_with_raw_sources(sheet, files, Some(sources))
}

fn build_disc_with_raw_sources(
    sheet: &CueSheet,
    files: &[FileBackedCueFile],
    raw_sources: Option<&[Arc<dyn CdTrackSource>]>,
) -> Result<CdDisc, PceCdLoadError> {
    if files.len() != sheet.files.len() {
        return Err(PceCdLoadError::MissingFile);
    }
    if raw_sources.is_some_and(|sources| sources.len() != files.len()) {
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
            true,
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
            false,
        )
    }

    fn open_with_hash(
        path: &Path,
        identity: FileIdentity,
        spec: FileSliceSpec,
        reject_reparse: bool,
        hash: bool,
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
        if hash {
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
        Ok(Arc::new(Self {
            reader: Mutex::new(FileSliceReader {
                file,
                identity,
                start,
                cached_sector: None,
                cache: [0; 2_352],
            }),
            bytes,
            payload_hash: hasher.finalize().into(),
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
mod tests {
    use super::*;
    use crate::emu_backend::pce_cd::{build_disc, parse_cue_bytes};
    use crate::patching::apply_ppf_patch_segments;

    fn temp_file(name: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "zeff-pce-cd-file-{}-{name}.bin",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn ppf1(records: &[(u32, &[u8])]) -> Vec<u8> {
        let mut patch = b"PPF10\0".to_vec();
        patch.resize(56, 0);
        for (offset, bytes) in records {
            patch.extend_from_slice(&offset.to_le_bytes());
            patch.push(bytes.len() as u8);
            patch.extend_from_slice(bytes);
        }
        patch
    }

    fn ppf3(records: &[(u64, &[u8])], block: &[u8]) -> Vec<u8> {
        let mut patch = b"PPF30\x02".to_vec();
        patch.resize(56, 0);
        patch.extend_from_slice(&[0, 1, 0, 0]);
        patch.extend_from_slice(block);
        for (offset, bytes) in records {
            patch.extend_from_slice(&offset.to_le_bytes());
            patch.push(bytes.len() as u8);
            patch.extend_from_slice(bytes);
        }
        patch
    }

    #[test]
    fn audio_samples_share_one_file_sector_read() {
        let mut audio = vec![0; 2 * 2_352];
        for sample in 0..588 {
            let offset = sample * 4;
            audio[offset..offset + 2].copy_from_slice(&(sample as i16).to_le_bytes());
        }
        let path = temp_file("audio", &audio);
        let metadata = std::fs::metadata(&path).unwrap();
        let source = FileSliceSource::open(
            &path,
            FileIdentity::from_metadata(&metadata),
            0,
            audio.len(),
            2_352,
            false,
        )
        .unwrap();
        let track_source: Arc<dyn CdTrackSource> = source.clone();
        let disc = CdDisc::new(vec![
            CdTrack::from_index1_source(1, 0, None, 0, CdTrackMode::Audio, track_source).unwrap(),
        ])
        .unwrap();
        source.reset_cache_for_test();
        for sample in 0..588 {
            assert_eq!(disc.read_audio_sample(0, sample).unwrap().0, sample as i16);
        }
        assert_eq!(source.read_count(), 1);
    }

    #[test]
    fn direct_cue_file_sources_preserve_owned_identity_and_bytes() {
        let root =
            std::env::temp_dir().join(format!("zeff-pce-cd-file-{}-identity", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut raw = vec![0; 4 * 2_352];
        raw[16] = 0xEE;
        raw[2_352 + 16] = 0x11;
        raw[2 * 2_352 + 16] = 0x22;
        raw[3 * 2_352..3 * 2_352 + 2].copy_from_slice(&0x3456_i16.to_le_bytes());
        let cue = b"FILE \"DISC.BIN\" BINARY\nTRACK 01 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\nTRACK 02 AUDIO\nINDEX 01 00:00:03\n";
        let cue_path = root.join("disc.cue");
        std::fs::write(&cue_path, cue).unwrap();
        std::fs::write(root.join("disc.bin"), &raw).unwrap();
        let sheet = parse_cue_bytes(cue).unwrap();
        let owned = build_disc(cue.to_vec(), &sheet, vec![raw]).unwrap();
        let file_backed = load_direct_cue_file_backed(&cue_path, cue, &sheet).unwrap();
        assert_eq!(file_backed.disc, owned.disc);
        assert_eq!(file_backed.content_sha256, owned.content_sha256);
        assert_eq!(file_backed.content_crc32, owned.content_crc32);
        assert_eq!(file_backed.source_disc_sha256, owned.source_disc_sha256);
        assert_eq!(file_backed.disc.read_user_sector(0).unwrap()[0], 0x11);
        assert_eq!(file_backed.disc.read_user_sector(1).unwrap()[0], 0x22);
        assert_eq!(file_backed.disc.read_audio_sample(2, 0).unwrap().0, 0x3456);
    }

    #[test]
    fn multifile_pregap_sources_match_owned_disc() {
        let root =
            std::env::temp_dir().join(format!("zeff-pce-cd-file-{}-multifile", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let mut first = vec![0; 2 * 2_048];
        first[0] = 0x10;
        let mut raw = vec![0; 3 * 2_352];
        raw[16] = 0x20;
        raw[2_352 + 16] = 0x21;
        let mut audio = vec![0; 2 * 2_352];
        audio[..2].copy_from_slice(&0x4567_i16.to_le_bytes());
        let cue = b"FILE \"first.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"raw.bin\" BINARY\nTRACK 02 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\nFILE \"audio.bin\" BINARY\nTRACK 03 AUDIO\nPREGAP 00:00:01\nINDEX 01 00:00:00\n";
        let cue_path = root.join("disc.cue");
        std::fs::write(&cue_path, cue).unwrap();
        std::fs::write(root.join("FIRST.ISO"), &first).unwrap();
        std::fs::write(root.join("RAW.BIN"), &raw).unwrap();
        std::fs::write(root.join("AUDIO.BIN"), &audio).unwrap();
        let sheet = parse_cue_bytes(cue).unwrap();
        let owned = build_disc(cue.to_vec(), &sheet, vec![first, raw, audio]).unwrap();
        let file_backed = load_direct_cue_file_backed(&cue_path, cue, &sheet).unwrap();
        assert_eq!(file_backed.disc, owned.disc);
        assert_eq!(file_backed.content_sha256, owned.content_sha256);
        assert_eq!(file_backed.content_crc32, owned.content_crc32);
        assert_eq!(file_backed.source_disc_sha256, owned.source_disc_sha256);
        assert_eq!(file_backed.disc.read_user_sector(0).unwrap()[0], 0x10);
        assert_eq!(file_backed.disc.read_user_sector(2).unwrap()[0], 0x20);
        assert_eq!(file_backed.disc.read_user_sector(3).unwrap()[0], 0x21);
        assert!(file_backed.disc.read_audio_sample(5, 0).is_err());
        assert_eq!(file_backed.disc.read_audio_sample(6, 0).unwrap().0, 0x4567);
    }

    #[test]
    fn owned_and_file_backed_builders_reject_the_same_invalid_layouts() {
        let cases = [
            (
                "missing-index",
                "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\n",
                vec![("disc.bin", 2_048)],
                PceCdLoadError::MissingIndex1(1),
            ),
            (
                "mixed-sectors",
                "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nTRACK 02 AUDIO\nINDEX 01 00:00:01\n",
                vec![("disc.bin", 2 * 2_352)],
                PceCdLoadError::MixedSectorSizes,
            ),
            (
                "misaligned",
                "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\n",
                vec![("disc.bin", 2_049)],
                PceCdLoadError::MisalignedBin {
                    bytes: 2_049,
                    sector_bytes: 2_048,
                },
            ),
            (
                "outside",
                "FILE \"disc.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:01\n",
                vec![("disc.bin", 2_048)],
                PceCdLoadError::TrackOutsideBin(1),
            ),
            (
                "invalid-index-order",
                "FILE \"first.bin\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"second.bin\" BINARY\nTRACK 02 MODE1/2048\nINDEX 00 00:00:02\nINDEX 01 00:00:01\n",
                vec![("first.bin", 2_048), ("second.bin", 4 * 2_048)],
                PceCdLoadError::InvalidIndexOrder(2),
            ),
        ];

        for (name, cue, file_specs, expected) in cases {
            let root = std::env::temp_dir().join(format!(
                "zeff-pce-cd-file-{}-layout-{name}",
                std::process::id()
            ));
            std::fs::create_dir_all(&root).unwrap();
            let cue_path = root.join("disc.cue");
            std::fs::write(&cue_path, cue).unwrap();
            let mut owned_files = Vec::with_capacity(file_specs.len());
            for (filename, bytes) in file_specs {
                let data = vec![0; bytes];
                std::fs::write(root.join(filename), &data).unwrap();
                owned_files.push(data);
            }
            let sheet = parse_cue_bytes(cue.as_bytes()).unwrap();
            let owned_error = match build_disc(cue.as_bytes().to_vec(), &sheet, owned_files) {
                Ok(_) => panic!("owned builder accepted invalid case {name}"),
                Err(error) => error,
            };
            let file_backed_error =
                match load_direct_cue_file_backed(&cue_path, cue.as_bytes(), &sheet) {
                    Ok(_) => panic!("file-backed builder accepted invalid case {name}"),
                    Err(error) => error,
                };
            assert_eq!(owned_error, expected, "owned case {name}");
            assert_eq!(file_backed_error, expected, "file-backed case {name}");
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn direct_and_cached_ppf_overlays_match_owned_raw_file_domain() {
        let root =
            std::env::temp_dir().join(format!("zeff-pce-cd-file-{}-overlay", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let first = (0..13 * 2_048)
            .map(|index| (index as u8).wrapping_mul(7).wrapping_add(3))
            .collect::<Vec<_>>();
        let second = (0..5 * 2_352)
            .map(|index| (index as u8).wrapping_mul(11).wrapping_add(5))
            .collect::<Vec<_>>();
        let third = (0..2 * 2_352)
            .map(|index| (index as u8).wrapping_mul(13).wrapping_add(9))
            .collect::<Vec<_>>();
        let cue = b"FILE \"first.iso\" BINARY\nTRACK 01 MODE1/2048\nINDEX 01 00:00:00\nFILE \"second.bin\" BINARY\nTRACK 02 MODE1/2352\nINDEX 00 00:00:00\nINDEX 01 00:00:01\nFILE \"third.bin\" BINARY\nTRACK 03 AUDIO\nPREGAP 00:00:01\nINDEX 01 00:00:00\n";
        let cue_path = root.join("disc.cue");
        std::fs::write(&cue_path, cue).unwrap();
        let paths = [
            root.join("first.iso"),
            root.join("second.bin"),
            root.join("third.bin"),
        ];
        for (path, bytes) in paths.iter().zip([&first, &second, &third]) {
            std::fs::write(path, bytes).unwrap();
        }
        let first_boundary = first.len();
        let second_boundary = first.len() + second.len();
        assert!((0x9320..0x9320 + 1024).contains(&second_boundary));
        let first_patch = ppf1(&[(0x9324, &[0xA5])]);
        let mut joined = [&first[..], &second[..], &third[..]].concat();
        crate::patching::apply_ppf_patch(&mut joined, &first_patch).unwrap();
        let second_patch = ppf3(
            &[(
                first_boundary as u64 - 2,
                &[0x10, 0x20, 0x30, 0x40, 0x50, 0x60],
            )],
            &joined[0x9320..0x9320 + 1024],
        );
        std::fs::write(root.join("first.ppf"), &first_patch).unwrap();
        std::fs::write(root.join("second.ppf"), &second_patch).unwrap();
        let mods = [
            crate::mods::ModEntry {
                filename: "first.ppf".to_owned(),
                enabled: true,
                target: None,
            },
            crate::mods::ModEntry {
                filename: "second.ppf".to_owned(),
                enabled: true,
                target: None,
            },
        ];
        let sheet = parse_cue_bytes(cue).unwrap();
        let direct = try_load_direct_cue_ppf_overlay(&cue_path, &sheet, &root, &mods)
            .unwrap()
            .unwrap();

        let mut owned_files = vec![first, second, third];
        apply_ppf_patch_segments(&mut owned_files, &first_patch).unwrap();
        apply_ppf_patch_segments(&mut owned_files, &second_patch).unwrap();
        let owned = build_disc(cue.to_vec(), &sheet, owned_files).unwrap().disc;
        assert_eq!(direct, owned);

        let cached_sources = paths
            .iter()
            .map(|path| {
                let bytes = std::fs::read(path).unwrap();
                CueFileSource {
                    path: path.clone(),
                    bytes: bytes.len() as u64,
                    sha256: Sha256::digest(bytes).into(),
                }
            })
            .collect();
        let cached = try_load_cached_cue_ppf_overlay(&sheet, cached_sources, &root, &mods)
            .unwrap()
            .unwrap();
        assert_eq!(cached, owned);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn file_source_rejects_external_length_changes() {
        let path = temp_file("changed", &[0; 2_352]);
        let metadata = std::fs::metadata(&path).unwrap();
        let source = FileSliceSource::open(
            &path,
            FileIdentity::from_metadata(&metadata),
            0,
            2_352,
            2_352,
            false,
        )
        .unwrap();
        let mut bytes = [0; 4];
        source.read_exact_at(0, &mut bytes).unwrap();
        std::fs::write(&path, [0; 2 * 2_352]).unwrap();
        assert_eq!(
            source.read_exact_at(0, &mut bytes),
            Err(CdSourceError::ReadFailed)
        );
    }
}
