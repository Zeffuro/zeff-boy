use std::collections::BTreeSet;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

use zeff_pce_core::hardware::{CdDisc, CdSourceError, CdTrack, CdTrackMode, CdTrackSource};

use super::pce_cd::{
    ChdTrack, LoadedPceCd, PCE_CD_DATA_BYTES_LIMIT, PceCdLoadError,
    build_chd_disc_with_mods_and_identity, chd_content_identity_from_header, padded_chd_frames,
    parse_chd_track_metadata, pce_cd_mod_config,
};
#[cfg(test)]
use super::pce_cd_overlay::{PATCH_BYTES_LIMIT, PatchOverlayApply};
use super::pce_cd_overlay::{
    PatchOverlayBuilder, PatchOverlayStack, apply_ppf_stack, log_ppf_overlay,
};

const CHD_HEADER_BYTES: usize = 124;
const CHD_UNIT_BYTES: usize = 2_448;
const CHD_HUNK_BYTES_LIMIT: usize = 16 * 1024 * 1024;
const CHD_MAP_BYTES_LIMIT: u64 = 16 * 1024 * 1024;
const CHD_METADATA_BYTES_LIMIT: u64 = 16 * 1024 * 1024;
const CHD_METADATA_ENTRY_LIMIT: usize = 256;
const CHD_TRACK_METADATA_BYTES_LIMIT: usize = 1024;
const CHD_TRACK_LIMIT: usize = 99;
const CHD_SELF_CHAIN_LIMIT: u8 = 64;
const SOURCE_READ_STAGING_BYTES: usize = 2_352;
const CHD_LOGICAL_BYTES_LIMIT: u64 =
    (PCE_CD_DATA_BYTES_LIMIT as u64 / 2_048) * 2_448 + CHD_TRACK_LIMIT as u64 * 3 * 2_448;

mod source_identity;
use source_identity::{FileIdentity, apply_raw_source_media, hash_file};
#[cfg(test)]
#[path = "pce_cd_chd/test_fixture.rs"]
mod test_fixture;
#[cfg(test)]
pub(crate) use test_fixture::write_synthetic_uncompressed_v5_chd;

struct RawChdHeader {
    bytes: [u8; CHD_HEADER_BYTES],
    compression: [u32; 4],
    logical_bytes: u64,
    map_offset: u64,
    metadata_offset: u64,
    hunk_bytes: usize,
    hunk_count: u32,
}

trait HunkDecoder: Send {
    fn verify_identity(&mut self) -> Result<(), CdSourceError>;
    fn decode_hunk(
        &mut self,
        hunk: u32,
        compressed: &mut Vec<u8>,
        output: &mut [u8],
    ) -> Result<(), CdSourceError>;
}

struct NativeHunkDecoder {
    chd: chd::Chd<BufReader<File>>,
    identity: FileIdentity,
}

struct ChdImage {
    hunk_bytes: usize,
    logical_bytes: u64,
    state: Mutex<ChdImageState>,
}

struct ChdImageState {
    decoder: Box<dyn HunkDecoder>,
    compressed: Vec<u8>,
    cached_hunk: Option<u32>,
    cached: Vec<u8>,
    decode_staging: Vec<u8>,
    read_staging: Box<[u8; SOURCE_READ_STAGING_BYTES]>,
}

struct ChdTrackSource {
    image: Arc<ChdImage>,
    logical_start_frame: u64,
    frames: u32,
    payload_bytes: usize,
    audio: bool,
}

pub(super) fn load_direct_chd_with_mods(
    path: &Path,
    apply_mods: bool,
) -> Result<LoadedPceCd, PceCdLoadError> {
    let mut file = File::open(path).map_err(|_| PceCdLoadError::ChdUnreadable(path.into()))?;
    let metadata = file
        .metadata()
        .map_err(|_| PceCdLoadError::ChdUnreadable(path.into()))?;
    if !metadata.is_file() {
        return Err(PceCdLoadError::ChdUnreadable(path.into()));
    }
    let identity = FileIdentity::from_metadata(&metadata);
    let raw_source_media_sha256 = hash_file(&mut file, identity.bytes, path)?;
    let raw = read_raw_header(&mut file, identity.bytes, path)?;
    let mut tracks = read_tracks(&mut file, &raw, identity.bytes, path)?;
    tracks.sort_by_key(|track| track.number);
    validate_tracks(&tracks, raw.logical_bytes)?;
    let content_identity = chd_content_identity_from_header(&raw.bytes, &tracks)?;

    file.seek(SeekFrom::Start(0))
        .map_err(|_| PceCdLoadError::ChdUnreadable(path.into()))?;
    let chd = chd::Chd::open(BufReader::new(file), None)
        .map_err(|error| PceCdLoadError::Disc(error.to_string()))?;
    validate_open_chd(&chd, &raw, identity.bytes)?;
    let image = Arc::new(ChdImage::new(
        Box::new(NativeHunkDecoder { chd, identity }),
        raw.hunk_bytes,
        raw.logical_bytes,
    )?);
    let sources = track_sources(&tracks, image.clone())?;
    let disc = build_source_disc_from_sources(&tracks, &sources)?;
    let source_disc_sha256 = disc.content_hash();
    let mut loaded = LoadedPceCd {
        disc,
        raw_source_media_sha256,
        raw_source_media_len: usize::try_from(identity.bytes)
            .map_err(|_| PceCdLoadError::DataTooLarge(identity.bytes))?,
        content_sha256: content_identity.0,
        content_crc32: content_identity.1,
        mod_crc32: crc32fast::hash(&source_disc_sha256),
        source_disc_sha256,
    };
    if !apply_mods {
        return Ok(loaded);
    }
    let (dir, mods, selected_crc32) =
        pce_cd_mod_config(crc32fast::hash(&source_disc_sha256), content_identity.1);
    if !mods.iter().any(|entry| entry.enabled) {
        loaded.mod_crc32 = selected_crc32;
        return Ok(loaded);
    }
    if let Some(disc) = try_build_ppf_overlay_disc(&tracks, sources, &dir, &mods)? {
        loaded.disc = disc;
        loaded.mod_crc32 = selected_crc32;
        return Ok(loaded);
    }
    drop(loaded);
    let payloads = materialize_payloads(&tracks, image)?;
    let mut loaded =
        build_chd_disc_with_mods_and_identity(tracks, payloads, true, content_identity)?;
    apply_raw_source_media(&mut loaded, raw_source_media_sha256, identity.bytes)?;
    Ok(loaded)
}

impl HunkDecoder for NativeHunkDecoder {
    fn verify_identity(&mut self) -> Result<(), CdSourceError> {
        let metadata = self
            .chd
            .inner()
            .get_ref()
            .metadata()
            .map_err(|_| CdSourceError::ReadFailed)?;
        (metadata.is_file() && FileIdentity::from_metadata(&metadata) == self.identity)
            .then_some(())
            .ok_or(CdSourceError::ReadFailed)
    }

    fn decode_hunk(
        &mut self,
        hunk: u32,
        compressed: &mut Vec<u8>,
        output: &mut [u8],
    ) -> Result<(), CdSourceError> {
        let mut hunk = self.chd.hunk(hunk).map_err(|_| CdSourceError::ReadFailed)?;
        let read = hunk
            .read_hunk_in(compressed, output)
            .map_err(|_| CdSourceError::ReadFailed)?;
        (read == output.len())
            .then_some(())
            .ok_or(CdSourceError::ReadFailed)
    }
}

impl ChdImage {
    fn new(
        decoder: Box<dyn HunkDecoder>,
        hunk_bytes: usize,
        logical_bytes: u64,
    ) -> Result<Self, PceCdLoadError> {
        if hunk_bytes == 0 || hunk_bytes > CHD_HUNK_BYTES_LIMIT {
            return Err(PceCdLoadError::InvalidChdMetadata);
        }
        Ok(Self {
            hunk_bytes,
            logical_bytes,
            state: Mutex::new(ChdImageState {
                decoder,
                compressed: Vec::new(),
                cached_hunk: None,
                cached: vec![0; hunk_bytes],
                decode_staging: vec![0; hunk_bytes],
                read_staging: Box::new([0; SOURCE_READ_STAGING_BYTES]),
            }),
        })
    }

    fn read_track(
        &self,
        logical_start_frame: u64,
        payload_bytes: usize,
        audio: bool,
        offset: usize,
        output: &mut [u8],
    ) -> Result<(), CdSourceError> {
        if output.len() > SOURCE_READ_STAGING_BYTES {
            return Err(CdSourceError::ReadFailed);
        }
        let mut state = self.state.lock().map_err(|_| CdSourceError::ReadFailed)?;
        let mut source_offset = offset;
        let mut destination_offset = 0;
        while destination_offset < output.len() {
            let frame = source_offset / payload_bytes;
            let within_frame = source_offset % payload_bytes;
            let logical_frame = logical_start_frame
                .checked_add(frame as u64)
                .ok_or(CdSourceError::ReadFailed)?;
            let logical_sector = logical_frame
                .checked_mul(CHD_UNIT_BYTES as u64)
                .ok_or(CdSourceError::ReadFailed)?;
            let frame_count = (payload_bytes - within_frame).min(output.len() - destination_offset);
            let raw_offset = logical_sector
                .checked_add(within_frame as u64)
                .ok_or(CdSourceError::ReadFailed)?;
            let raw_end = logical_sector
                .checked_add(payload_bytes as u64)
                .ok_or(CdSourceError::ReadFailed)?;
            if raw_end > self.logical_bytes {
                return Err(CdSourceError::ReadFailed);
            }
            if audio {
                let mut current_hunk = None;
                for index in 0..frame_count {
                    let normalized = within_frame + index;
                    let raw = logical_sector + (normalized ^ 1) as u64;
                    let hunk = u32::try_from(raw / self.hunk_bytes as u64)
                        .map_err(|_| CdSourceError::ReadFailed)?;
                    if current_hunk != Some(hunk) {
                        state.ensure_hunk(hunk)?;
                        current_hunk = Some(hunk);
                    }
                    let source = (raw - hunk as u64 * self.hunk_bytes as u64) as usize;
                    let value = state.cached[source];
                    state.read_staging[destination_offset + index] = value;
                }
                source_offset += frame_count;
                destination_offset += frame_count;
            } else {
                let mut remaining = frame_count;
                let mut raw_cursor = raw_offset;
                while remaining != 0 {
                    let hunk = u32::try_from(raw_cursor / self.hunk_bytes as u64)
                        .map_err(|_| CdSourceError::ReadFailed)?;
                    state.ensure_hunk(hunk)?;
                    let source = (raw_cursor - hunk as u64 * self.hunk_bytes as u64) as usize;
                    let count = remaining.min(self.hunk_bytes - source);
                    let destination = destination_offset + frame_count - remaining;
                    let ChdImageState {
                        cached,
                        read_staging,
                        ..
                    } = &mut *state;
                    read_staging[destination..destination + count]
                        .copy_from_slice(&cached[source..source + count]);
                    remaining -= count;
                    source_offset += count;
                    raw_cursor += count as u64;
                }
                destination_offset += frame_count;
            }
        }
        output.copy_from_slice(&state.read_staging[..output.len()]);
        Ok(())
    }

    fn visit_track(
        &self,
        logical_start_frame: u64,
        frames: u32,
        payload_bytes: usize,
        audio: bool,
        visitor: &mut dyn FnMut(&[u8]),
    ) -> Result<(), CdSourceError> {
        if payload_bytes == 0 || payload_bytes > SOURCE_READ_STAGING_BYTES {
            return Err(CdSourceError::ReadFailed);
        }
        let logical_end = logical_start_frame
            .checked_add(u64::from(frames))
            .and_then(|frame| frame.checked_mul(CHD_UNIT_BYTES as u64))
            .filter(|&end| end <= self.logical_bytes)
            .ok_or(CdSourceError::ReadFailed)?;
        let mut state = self.state.lock().map_err(|_| CdSourceError::ReadFailed)?;
        state.decoder.verify_identity()?;
        let mut logical_sector = logical_start_frame
            .checked_mul(CHD_UNIT_BYTES as u64)
            .ok_or(CdSourceError::ReadFailed)?;
        while logical_sector < logical_end {
            let hunk = u32::try_from(logical_sector / self.hunk_bytes as u64)
                .map_err(|_| CdSourceError::ReadFailed)?;
            if state.cached_hunk != Some(hunk) {
                state.decoder.verify_identity()?;
                state.load_hunk(hunk)?;
            }
            let source = (logical_sector - hunk as u64 * self.hunk_bytes as u64) as usize;
            let source_end = source
                .checked_add(payload_bytes)
                .filter(|&end| end <= self.hunk_bytes)
                .ok_or(CdSourceError::ReadFailed)?;
            let ChdImageState {
                cached,
                read_staging,
                ..
            } = &mut *state;
            read_staging[..payload_bytes].copy_from_slice(&cached[source..source_end]);
            if audio {
                for sample in read_staging[..payload_bytes].as_chunks_mut::<2>().0 {
                    sample.swap(0, 1);
                }
            }
            visitor(&read_staging[..payload_bytes]);
            logical_sector += CHD_UNIT_BYTES as u64;
        }
        state.decoder.verify_identity()?;
        Ok(())
    }
}

impl ChdImageState {
    fn ensure_hunk(&mut self, hunk: u32) -> Result<(), CdSourceError> {
        self.decoder.verify_identity()?;
        self.load_hunk(hunk)
    }

    fn load_hunk(&mut self, hunk: u32) -> Result<(), CdSourceError> {
        if self.cached_hunk == Some(hunk) {
            return Ok(());
        }
        self.decoder
            .decode_hunk(hunk, &mut self.compressed, &mut self.decode_staging)?;
        std::mem::swap(&mut self.cached, &mut self.decode_staging);
        self.cached_hunk = Some(hunk);
        Ok(())
    }
}

impl CdTrackSource for ChdTrackSource {
    fn len(&self) -> usize {
        self.frames as usize * self.payload_bytes
    }

    fn payload_hash(&self) -> [u8; 32] {
        [0; 32]
    }

    fn read_exact_at(&self, offset: usize, output: &mut [u8]) -> Result<(), CdSourceError> {
        offset
            .checked_add(output.len())
            .filter(|&end| end <= self.len())
            .ok_or(CdSourceError::OutOfRange {
                offset,
                bytes: output.len(),
                source_len: self.len(),
            })?;
        self.image.read_track(
            self.logical_start_frame,
            self.payload_bytes,
            self.audio,
            offset,
            output,
        )
    }

    fn visit_payload(
        &self,
        sector_bytes: usize,
        visitor: &mut dyn FnMut(&[u8]),
    ) -> Result<(), CdSourceError> {
        if sector_bytes != self.payload_bytes {
            return Err(CdSourceError::ReadFailed);
        }
        self.image.visit_track(
            self.logical_start_frame,
            self.frames,
            self.payload_bytes,
            self.audio,
            visitor,
        )
    }
}

fn read_raw_header(
    file: &mut File,
    file_bytes: u64,
    path: &Path,
) -> Result<RawChdHeader, PceCdLoadError> {
    let mut bytes = [0; CHD_HEADER_BYTES];
    file.seek(SeekFrom::Start(0))
        .and_then(|_| file.read_exact(&mut bytes))
        .map_err(|_| PceCdLoadError::ChdUnreadable(path.into()))?;
    if &bytes[..8] != b"MComprHD" || be_u32(&bytes[8..12]) != 124 || be_u32(&bytes[12..16]) != 5 {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    let compression = [
        be_u32(&bytes[16..20]),
        be_u32(&bytes[20..24]),
        be_u32(&bytes[24..28]),
        be_u32(&bytes[28..32]),
    ];
    let logical_bytes = be_u64(&bytes[32..40]);
    let map_offset = be_u64(&bytes[40..48]);
    let metadata_offset = be_u64(&bytes[48..56]);
    let hunk_bytes = be_u32(&bytes[56..60]) as usize;
    let unit_bytes = be_u32(&bytes[60..64]) as usize;
    if hunk_bytes == 0
        || hunk_bytes > CHD_HUNK_BYTES_LIMIT
        || !hunk_bytes.is_multiple_of(CHD_UNIT_BYTES)
        || unit_bytes != CHD_UNIT_BYTES
        || map_offset < CHD_HEADER_BYTES as u64
        || metadata_offset != 0 && metadata_offset < CHD_HEADER_BYTES as u64
        || bytes[104..124].iter().any(|&byte| byte != 0)
        || logical_bytes == 0
        || logical_bytes > CHD_LOGICAL_BYTES_LIMIT
        || !logical_bytes.is_multiple_of(CHD_UNIT_BYTES as u64)
    {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    let hunk_count = logical_bytes.div_ceil(hunk_bytes as u64);
    let hunk_count = u32::try_from(hunk_count).map_err(|_| PceCdLoadError::InvalidChdMetadata)?;
    validate_map_bounds(
        file,
        file_bytes,
        compression[0],
        map_offset,
        hunk_count,
        path,
    )?;
    Ok(RawChdHeader {
        bytes,
        compression,
        logical_bytes,
        map_offset,
        metadata_offset,
        hunk_bytes,
        hunk_count,
    })
}

fn validate_map_bounds(
    file: &mut File,
    file_bytes: u64,
    compression: u32,
    map_offset: u64,
    hunk_count: u32,
    path: &Path,
) -> Result<(), PceCdLoadError> {
    if compression == 0 {
        let map_bytes = u64::from(hunk_count)
            .checked_mul(4)
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
        map_offset
            .checked_add(map_bytes)
            .filter(|&end| end <= file_bytes)
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
        return Ok(());
    }
    let mut bytes = [0; 4];
    file.seek(SeekFrom::Start(map_offset))
        .and_then(|_| file.read_exact(&mut bytes))
        .map_err(|_| PceCdLoadError::ChdUnreadable(path.into()))?;
    let map_bytes = u64::from(be_u32(&bytes));
    if map_bytes > CHD_MAP_BYTES_LIMIT {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    map_offset
        .checked_add(16)
        .and_then(|offset| offset.checked_add(map_bytes))
        .filter(|&end| end <= file_bytes)
        .ok_or(PceCdLoadError::InvalidChdMetadata)?;
    Ok(())
}

fn read_tracks(
    file: &mut File,
    header: &RawChdHeader,
    file_bytes: u64,
    path: &Path,
) -> Result<Vec<ChdTrack>, PceCdLoadError> {
    let mut offset = header.metadata_offset;
    let mut visited = BTreeSet::new();
    let mut entries = 0;
    let mut total_bytes = 0_u64;
    let mut tracks = Vec::new();
    while offset != 0 {
        if offset < CHD_HEADER_BYTES as u64
            || entries == CHD_METADATA_ENTRY_LIMIT
            || !visited.insert(offset)
        {
            return Err(PceCdLoadError::InvalidChdMetadata);
        }
        offset
            .checked_add(16)
            .filter(|&end| end <= file_bytes)
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
        let mut raw = [0; 16];
        file.seek(SeekFrom::Start(offset))
            .and_then(|_| file.read_exact(&mut raw))
            .map_err(|_| PceCdLoadError::ChdUnreadable(path.into()))?;
        let tag = be_u32(&raw[..4]);
        let length = (be_u32(&raw[4..8]) & 0x00FF_FFFF) as usize;
        let next = be_u64(&raw[8..16]);
        let record_bytes = 16_u64
            .checked_add(length as u64)
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
        total_bytes = total_bytes
            .checked_add(record_bytes)
            .filter(|&total| total <= CHD_METADATA_BYTES_LIMIT)
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
        offset
            .checked_add(record_bytes)
            .filter(|&end| end <= file_bytes)
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
        if tag == u32::from_be_bytes(*b"CHT2") {
            if length > CHD_TRACK_METADATA_BYTES_LIMIT || tracks.len() == CHD_TRACK_LIMIT {
                return Err(PceCdLoadError::InvalidChdMetadata);
            }
            let mut value = vec![0; length];
            file.read_exact(&mut value)
                .map_err(|_| PceCdLoadError::ChdUnreadable(path.into()))?;
            tracks.push(parse_chd_track_metadata(&value)?);
        }
        entries += 1;
        offset = next;
    }
    Ok(tracks)
}

fn validate_tracks(tracks: &[ChdTrack], logical_bytes: u64) -> Result<(), PceCdLoadError> {
    if tracks.is_empty() || tracks.len() > CHD_TRACK_LIMIT {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    for (index, track) in tracks.iter().enumerate() {
        if track.number != (index + 1) as u8 {
            return Err(PceCdLoadError::InvalidTrackOrder);
        }
    }
    let total_frames = tracks.iter().try_fold(0_u64, |total, track| {
        total
            .checked_add(u64::from(padded_chd_frames(track.frames)))
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))
    })?;
    if total_frames
        .checked_mul(CHD_UNIT_BYTES as u64)
        .filter(|&bytes| bytes == logical_bytes)
        .is_none()
    {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    let data_bytes = tracks.iter().try_fold(0_u64, |total, track| {
        total
            .checked_add(track.frames as u64 * track.payload_bytes() as u64)
            .ok_or(PceCdLoadError::DataTooLarge(u64::MAX))
    })?;
    if data_bytes > PCE_CD_DATA_BYTES_LIMIT as u64 {
        return Err(PceCdLoadError::DataTooLarge(data_bytes));
    }
    Ok(())
}

fn validate_open_chd(
    chd: &chd::Chd<BufReader<File>>,
    raw: &RawChdHeader,
    file_bytes: u64,
) -> Result<(), PceCdLoadError> {
    let header = chd.header();
    let chd::header::Header::V5Header(v5) = header else {
        return Err(PceCdLoadError::InvalidChdMetadata);
    };
    if header.has_parent()
        || v5.compression != raw.compression
        || v5.map_offset != raw.map_offset
        || v5.meta_offset != raw.metadata_offset
        || v5.unit_bytes != CHD_UNIT_BYTES as u32
        || v5.hunk_bytes as usize != raw.hunk_bytes
        || v5.hunk_count != raw.hunk_count
        || v5.logical_bytes != raw.logical_bytes
        || chd.map().len() != raw.hunk_count as usize
    {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    use chd::map::{CompressionTypeV5, MapEntry};
    let mut self_depths = Vec::with_capacity(chd.map().len());
    for (index, entry) in chd.map().iter().enumerate() {
        let self_depth = match entry {
            MapEntry::V5Compressed(entry) => match entry
                .hunk_type()
                .map_err(|_| PceCdLoadError::InvalidChdMetadata)?
            {
                kind @ (CompressionTypeV5::CompressionType0
                | CompressionTypeV5::CompressionType1
                | CompressionTypeV5::CompressionType2
                | CompressionTypeV5::CompressionType3
                | CompressionTypeV5::CompressionNone) => {
                    let bytes = u64::from(
                        entry
                            .block_size()
                            .map_err(|_| PceCdLoadError::InvalidChdMetadata)?,
                    );
                    let offset = entry
                        .block_offset()
                        .map_err(|_| PceCdLoadError::InvalidChdMetadata)?;
                    if bytes == 0
                        || bytes > CHD_HUNK_BYTES_LIMIT as u64
                        || matches!(kind, CompressionTypeV5::CompressionNone)
                            && bytes != raw.hunk_bytes as u64
                        || offset < CHD_HEADER_BYTES as u64
                        || offset
                            .checked_add(bytes)
                            .filter(|&end| end <= file_bytes)
                            .is_none()
                    {
                        return Err(PceCdLoadError::InvalidChdMetadata);
                    }
                    0
                }
                CompressionTypeV5::CompressionSelf => {
                    let target = entry
                        .block_offset()
                        .map_err(|_| PceCdLoadError::InvalidChdMetadata)?;
                    if target >= index as u64 {
                        return Err(PceCdLoadError::InvalidChdMetadata);
                    }
                    self_depth(&self_depths, target)?
                }
                _ => return Err(PceCdLoadError::InvalidChdMetadata),
            },
            MapEntry::V5Uncompressed(entry) => {
                let offset = entry
                    .block_offset()
                    .map_err(|_| PceCdLoadError::InvalidChdMetadata)?;
                if offset != 0
                    && (offset < CHD_HEADER_BYTES as u64
                        || offset
                            .checked_add(u64::from(entry.block_size()))
                            .filter(|&end| end <= file_bytes)
                            .is_none())
                {
                    return Err(PceCdLoadError::InvalidChdMetadata);
                }
                0
            }
            MapEntry::LegacyEntry(_) => return Err(PceCdLoadError::InvalidChdMetadata),
        };
        self_depths.push(self_depth);
    }
    Ok(())
}

fn self_depth(depths: &[u8], target: u64) -> Result<u8, PceCdLoadError> {
    usize::try_from(target)
        .ok()
        .and_then(|target| depths.get(target))
        .and_then(|depth| depth.checked_add(1))
        .filter(|&depth| depth <= CHD_SELF_CHAIN_LIMIT)
        .ok_or(PceCdLoadError::InvalidChdMetadata)
}

fn track_sources(
    tracks: &[ChdTrack],
    image: Arc<ChdImage>,
) -> Result<Vec<Arc<dyn CdTrackSource>>, PceCdLoadError> {
    let mut logical_start = 0_u64;
    let mut sources = Vec::with_capacity(tracks.len());
    for track in tracks {
        let source: Arc<dyn CdTrackSource> = make_track_source(image.clone(), logical_start, track);
        sources.push(source);
        logical_start = logical_start
            .checked_add(u64::from(padded_chd_frames(track.frames)))
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
    }
    Ok(sources)
}

fn build_source_disc_from_sources(
    tracks: &[ChdTrack],
    sources: &[Arc<dyn CdTrackSource>],
) -> Result<CdDisc, PceCdLoadError> {
    if tracks.len() != sources.len() {
        return Err(PceCdLoadError::InvalidChdMetadata);
    }
    let mut cursor = 0_u32;
    let mut disc_tracks = Vec::with_capacity(tracks.len());
    for (track, source) in tracks.iter().zip(sources) {
        let index0 = (track.pregap != 0).then_some(cursor);
        let index1 = cursor
            .checked_add(track.pregap)
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
        disc_tracks.push(
            CdTrack::from_stored_unverified_source(
                track.number,
                if track.mode == CdTrackMode::Audio {
                    0
                } else {
                    4
                },
                index0,
                index1,
                track.mode,
                source.clone(),
            )
            .map_err(|error| PceCdLoadError::Disc(error.to_string()))?,
        );
        cursor = cursor
            .checked_add(track.frames)
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
    }
    CdDisc::new(disc_tracks).map_err(|error| PceCdLoadError::Disc(error.to_string()))
}

fn try_build_ppf_overlay_disc(
    tracks: &[ChdTrack],
    sources: Vec<Arc<dyn CdTrackSource>>,
    dir: &Path,
    mods: &[crate::mods::ModEntry],
) -> Result<Option<CdDisc>, PceCdLoadError> {
    let Some(mut builder) = PatchOverlayBuilder::for_tracks(&sources) else {
        return Ok(None);
    };
    let PatchOverlayStack::Applied(applied) = apply_ppf_stack(&mut builder, dir, mods) else {
        return Ok(None);
    };
    let Some(sources) = builder.finish_tracks(sources) else {
        return Ok(None);
    };
    let disc = build_source_disc_from_sources(tracks, &sources)?;
    log_ppf_overlay(&applied);
    Ok(Some(disc))
}

#[cfg(test)]
fn build_ppf_overlay_disc(
    tracks: &[ChdTrack],
    sources: Vec<Arc<dyn CdTrackSource>>,
    patches: &[(&str, Vec<u8>)],
) -> Result<Option<CdDisc>, PceCdLoadError> {
    let Some(mut builder) = PatchOverlayBuilder::for_tracks(&sources) else {
        return Ok(None);
    };
    let mut applied = Vec::with_capacity(patches.len());
    for (filename, patch) in patches {
        let Ok(result) = builder.apply_ppf(patch) else {
            return Ok(None);
        };
        if !matches!(result, PatchOverlayApply::Applied) {
            return Ok(None);
        }
        applied.push((
            (*filename).to_owned(),
            !crate::patching::ppf_has_source_validation(patch),
        ));
    }
    let Some(sources) = builder.finish_tracks(sources) else {
        return Ok(None);
    };
    let disc = build_source_disc_from_sources(tracks, &sources)?;
    log_ppf_overlay(&applied);
    Ok(Some(disc))
}

#[cfg(test)]
fn build_source_disc(tracks: &[ChdTrack], image: Arc<ChdImage>) -> Result<CdDisc, PceCdLoadError> {
    let sources = track_sources(tracks, image)?;
    build_source_disc_from_sources(tracks, &sources)
}

fn make_track_source(
    image: Arc<ChdImage>,
    logical_start_frame: u64,
    track: &ChdTrack,
) -> Arc<ChdTrackSource> {
    let payload_bytes = track.payload_bytes();
    Arc::new(ChdTrackSource {
        image,
        logical_start_frame,
        frames: track.frames,
        payload_bytes,
        audio: track.mode == CdTrackMode::Audio,
    })
}

fn materialize_payloads(
    tracks: &[ChdTrack],
    image: Arc<ChdImage>,
) -> Result<Vec<Vec<u8>>, PceCdLoadError> {
    let mut logical_start = 0_u64;
    let mut payloads = Vec::with_capacity(tracks.len());
    for track in tracks {
        let source = make_track_source(image.clone(), logical_start, track);
        let mut payload = vec![0; source.len()];
        for offset in (0..payload.len()).step_by(track.payload_bytes()) {
            source
                .read_exact_at(offset, &mut payload[offset..offset + track.payload_bytes()])
                .map_err(|error| PceCdLoadError::Disc(error.to_string()))?;
        }
        payloads.push(payload);
        logical_start = logical_start
            .checked_add(u64::from(padded_chd_frames(track.frames)))
            .ok_or(PceCdLoadError::InvalidChdMetadata)?;
    }
    Ok(payloads)
}

fn be_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

fn be_u64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use super::*;
    use crate::emu_backend::pce_cd::build_chd_disc;
    use crate::patching::apply_ppf_patch_segments;

    struct SyntheticDecoder {
        data: Vec<u8>,
        hunk_bytes: usize,
        reads: Arc<AtomicUsize>,
        valid: Arc<AtomicBool>,
        fail_once: Arc<Mutex<Option<u32>>>,
    }

    impl HunkDecoder for SyntheticDecoder {
        fn verify_identity(&mut self) -> Result<(), CdSourceError> {
            self.valid
                .load(Ordering::Acquire)
                .then_some(())
                .ok_or(CdSourceError::ReadFailed)
        }

        fn decode_hunk(
            &mut self,
            hunk: u32,
            _compressed: &mut Vec<u8>,
            output: &mut [u8],
        ) -> Result<(), CdSourceError> {
            self.reads.fetch_add(1, Ordering::Relaxed);
            let mut fail_once = self.fail_once.lock().unwrap();
            if *fail_once == Some(hunk) {
                *fail_once = None;
                output[0] ^= 0xFF;
                return Err(CdSourceError::ReadFailed);
            }
            drop(fail_once);
            let start = hunk as usize * self.hunk_bytes;
            output.copy_from_slice(&self.data[start..start + self.hunk_bytes]);
            Ok(())
        }
    }

    struct SyntheticControl {
        reads: Arc<AtomicUsize>,
        valid: Arc<AtomicBool>,
        fail_once: Arc<Mutex<Option<u32>>>,
    }

    fn synthetic_image(data: Vec<u8>, hunk_bytes: usize) -> (Arc<ChdImage>, SyntheticControl) {
        let reads = Arc::new(AtomicUsize::new(0));
        let valid = Arc::new(AtomicBool::new(true));
        let fail_once = Arc::new(Mutex::new(None));
        let decoder = SyntheticDecoder {
            data: data.clone(),
            hunk_bytes,
            reads: reads.clone(),
            valid: valid.clone(),
            fail_once: fail_once.clone(),
        };
        let image =
            Arc::new(ChdImage::new(Box::new(decoder), hunk_bytes, data.len() as u64).unwrap());
        (
            image,
            SyntheticControl {
                reads,
                valid,
                fail_once,
            },
        )
    }

    fn tracks() -> Vec<ChdTrack> {
        vec![
            ChdTrack {
                number: 1,
                mode: CdTrackMode::Mode1_2048,
                frames: 1,
                pregap: 0,
            },
            ChdTrack {
                number: 2,
                mode: CdTrackMode::Mode1_2352,
                frames: 1,
                pregap: 0,
            },
            ChdTrack {
                number: 3,
                mode: CdTrackMode::Audio,
                frames: 1,
                pregap: 0,
            },
        ]
    }

    fn logical_bytes(tracks: &[ChdTrack]) -> Vec<u8> {
        let frames = tracks
            .iter()
            .map(|track| padded_chd_frames(track.frames) as usize)
            .sum::<usize>();
        vec![0; frames * CHD_UNIT_BYTES]
    }

    fn sector(bytes: &mut [u8], frame: usize) -> &mut [u8] {
        &mut bytes[frame * CHD_UNIT_BYTES..(frame + 1) * CHD_UNIT_BYTES]
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

    fn normalized_payloads(tracks: &[ChdTrack], data: &[u8]) -> Vec<Vec<u8>> {
        let mut logical_start = 0;
        tracks
            .iter()
            .map(|track| {
                let mut payload = Vec::with_capacity(track.frames as usize * track.payload_bytes());
                for frame in 0..track.frames as usize {
                    let start = (logical_start + frame) * CHD_UNIT_BYTES;
                    payload.extend_from_slice(&data[start..start + track.payload_bytes()]);
                }
                if track.mode == CdTrackMode::Audio {
                    for sample in payload.as_chunks_mut::<2>().0 {
                        sample.swap(0, 1);
                    }
                }
                logical_start += padded_chd_frames(track.frames) as usize;
                payload
            })
            .collect()
    }

    #[test]
    fn source_maps_data_raw_audio_and_padded_track_starts() {
        let tracks = tracks();
        let mut data = logical_bytes(&tracks);
        sector(&mut data, 0)[0] = 0x11;
        sector(&mut data, 4)[16] = 0x22;
        sector(&mut data, 8)[..4].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        let (image, _) = synthetic_image(data, 4 * CHD_UNIT_BYTES);
        let audio = make_track_source(image.clone(), 8, &tracks[2]);
        let mut odd = [0; 2];
        audio.read_exact_at(1, &mut odd).unwrap();
        let disc = build_source_disc(&tracks, image).unwrap();

        assert_eq!(odd, [0x12, 0x78]);
        assert_eq!(disc.read_user_sector(0).unwrap()[0], 0x11);
        assert_eq!(disc.read_user_sector(1).unwrap()[0], 0x22);
        assert_eq!(disc.read_audio_sample(2, 0).unwrap(), (0x1234, 0x5678));
    }

    #[test]
    fn single_scan_source_disc_matches_owned_normalized_payloads() {
        let tracks = tracks();
        let mut data = logical_bytes(&tracks);
        sector(&mut data, 0)[0] = 0x11;
        sector(&mut data, 4)[16] = 0x22;
        sector(&mut data, 8)[..4].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        let (image, control) = synthetic_image(data.clone(), 4 * CHD_UNIT_BYTES);
        let source = build_source_disc(&tracks, image).unwrap();
        let mut audio = data[8 * CHD_UNIT_BYTES..8 * CHD_UNIT_BYTES + 2_352].to_vec();
        for sample in audio.as_chunks_mut::<2>().0 {
            sample.swap(0, 1);
        }
        let owned = build_chd_disc(
            &tracks,
            &[
                data[..2_048].to_vec(),
                data[4 * CHD_UNIT_BYTES..4 * CHD_UNIT_BYTES + 2_352].to_vec(),
                audio,
            ],
        )
        .unwrap();

        assert_eq!(source, owned);
        assert_eq!(source.content_hash(), owned.content_hash());
        assert_eq!(
            source
                .tracks()
                .iter()
                .map(CdTrack::payload_hash)
                .collect::<Vec<_>>(),
            owned
                .tracks()
                .iter()
                .map(CdTrack::payload_hash)
                .collect::<Vec<_>>()
        );
        assert_eq!(control.reads.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn ppf_overlay_matches_owned_mixed_tracks_with_bounded_single_visits() {
        let tracks = vec![
            ChdTrack {
                number: 1,
                mode: CdTrackMode::Mode1_2048,
                frames: 7,
                pregap: 0,
            },
            ChdTrack {
                number: 2,
                mode: CdTrackMode::Mode1_2352,
                frames: 10,
                pregap: 0,
            },
            ChdTrack {
                number: 3,
                mode: CdTrackMode::Audio,
                frames: 2,
                pregap: 0,
            },
        ];
        let mut data = logical_bytes(&tracks);
        for (index, byte) in data.iter_mut().enumerate() {
            *byte = (index as u8).wrapping_mul(13).wrapping_add(5);
        }
        let mut owned_payloads = normalized_payloads(&tracks, &data);
        let mut joined = owned_payloads.concat();
        let boundary = owned_payloads[0].len() + owned_payloads[1].len();
        let first = ppf1(&[(0x9324, &[0xA5])]);
        crate::patching::apply_ppf_patch(&mut joined, &first).unwrap();
        let second = ppf3(
            &[(boundary as u64 - 2, &[0x10, 0x20, 0x30, 0x40, 0x50, 0x60])],
            &joined[0x9320..0x9320 + 1024],
        );
        let (image, control) = synthetic_image(data, 4 * CHD_UNIT_BYTES);
        let sources = track_sources(&tracks, image).unwrap();
        let overlay = build_ppf_overlay_disc(
            &tracks,
            sources,
            &[
                ("first.ppf", first.clone()),
                ("cross-track.ppf", second.clone()),
            ],
        )
        .unwrap()
        .unwrap();

        apply_ppf_patch_segments(&mut owned_payloads, &first).unwrap();
        apply_ppf_patch_segments(&mut owned_payloads, &second).unwrap();
        let owned = build_chd_disc(&tracks, &owned_payloads).unwrap();
        assert_eq!(overlay, owned);
        assert_eq!(overlay.content_hash(), owned.content_hash());
        assert_eq!(
            overlay
                .tracks()
                .iter()
                .map(CdTrack::payload_hash)
                .collect::<Vec<_>>(),
            owned
                .tracks()
                .iter()
                .map(CdTrack::payload_hash)
                .collect::<Vec<_>>()
        );
        assert_eq!(control.reads.load(Ordering::Relaxed), 8);
        assert_eq!(
            overlay.read_user_sector(0).unwrap(),
            owned.read_user_sector(0).unwrap()
        );
        assert_eq!(
            overlay.read_user_sector(7).unwrap(),
            owned.read_user_sector(7).unwrap()
        );
        assert_eq!(
            overlay.read_audio_sample(17, 0).unwrap(),
            owned.read_audio_sample(17, 0).unwrap()
        );
        assert_eq!(control.reads.load(Ordering::Relaxed), 11);
    }

    #[test]
    fn invalid_later_ppf_falls_back_without_publishing_partial_stack() {
        let track = ChdTrack {
            number: 1,
            mode: CdTrackMode::Mode1_2048,
            frames: 1,
            pregap: 0,
        };
        let data = logical_bytes(std::slice::from_ref(&track));
        let (image, control) = synthetic_image(data, 4 * CHD_UNIT_BYTES);
        let sources = track_sources(std::slice::from_ref(&track), image).unwrap();
        let valid = ppf1(&[(0, &[1, 2, 3])]);

        assert!(
            build_ppf_overlay_disc(
                std::slice::from_ref(&track),
                sources,
                &[("first.ppf", valid), ("invalid.ppf", b"PPF10".to_vec())],
            )
            .unwrap()
            .is_none()
        );
        assert_eq!(control.reads.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn non_ppf_and_oversized_ppf_select_owned_fallback_before_reads() {
        let track = ChdTrack {
            number: 1,
            mode: CdTrackMode::Mode1_2048,
            frames: 1,
            pregap: 0,
        };
        let data = logical_bytes(std::slice::from_ref(&track));
        let (image, control) = synthetic_image(data, 4 * CHD_UNIT_BYTES);
        let sources = track_sources(std::slice::from_ref(&track), image).unwrap();
        let xdelta = crate::mods::ModEntry {
            filename: "track01.xdelta".to_owned(),
            enabled: true,
            target: Some("Track 01".to_owned()),
        };
        assert!(
            try_build_ppf_overlay_disc(
                std::slice::from_ref(&track),
                sources.clone(),
                Path::new("."),
                &[xdelta],
            )
            .unwrap()
            .is_none()
        );

        let dir =
            std::env::temp_dir().join(format!("zeff-pce-chd-overlay-limit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("huge.ppf");
        File::create(&path)
            .unwrap()
            .set_len(PATCH_BYTES_LIMIT as u64 + 1)
            .unwrap();
        let oversized = crate::mods::ModEntry {
            filename: "huge.ppf".to_owned(),
            enabled: true,
            target: None,
        };
        assert!(
            try_build_ppf_overlay_disc(std::slice::from_ref(&track), sources, &dir, &[oversized],)
                .unwrap()
                .is_none()
        );
        assert_eq!(control.reads.load(Ordering::Relaxed), 0);
        std::fs::remove_file(path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn embedded_pregap_and_tail_padding_stay_out_of_payload_mapping() {
        let tracks = vec![
            ChdTrack {
                number: 1,
                mode: CdTrackMode::Mode1_2048,
                frames: 3,
                pregap: 1,
            },
            ChdTrack {
                number: 2,
                mode: CdTrackMode::Mode1_2048,
                frames: 1,
                pregap: 0,
            },
        ];
        let mut data = logical_bytes(&tracks);
        sector(&mut data, 0)[0] = 0x10;
        sector(&mut data, 2)[0] = 0x12;
        sector(&mut data, 3)[0] = 0xEE;
        sector(&mut data, 4)[0] = 0x20;
        let (image, _) = synthetic_image(data, 4 * CHD_UNIT_BYTES);
        let disc = build_source_disc(&tracks, image).unwrap();

        assert_eq!(disc.track(1).unwrap().index0_lba(), Some(0));
        assert_eq!(disc.track(1).unwrap().index1_lba(), 1);
        assert_eq!(disc.read_user_sector(0).unwrap()[0], 0x10);
        assert_eq!(disc.read_user_sector(2).unwrap()[0], 0x12);
        assert_eq!(disc.read_user_sector(3).unwrap()[0], 0x20);
    }

    #[test]
    fn clones_share_one_hunk_cache_and_cross_hunk_reads() {
        let track = ChdTrack {
            number: 1,
            mode: CdTrackMode::Mode1_2048,
            frames: 8,
            pregap: 0,
        };
        let mut data = logical_bytes(std::slice::from_ref(&track));
        for frame in 0..8 {
            sector(&mut data, frame)[..2_048].fill(frame as u8);
        }
        let (image, control) = synthetic_image(data, 4 * CHD_UNIT_BYTES);
        let source = make_track_source(image, 0, &track);
        let clone = source.clone();
        let before = control.reads.load(Ordering::Relaxed);
        let mut crossing = [0; 32];
        source.read_exact_at(4 * 2_048 - 8, &mut crossing).unwrap();
        let after = control.reads.load(Ordering::Relaxed);
        let mut repeated = [0; 16];
        clone.read_exact_at(4 * 2_048, &mut repeated).unwrap();

        assert_eq!(&crossing[..8], &[3; 8]);
        assert_eq!(&crossing[8..], &[4; 24]);
        assert_eq!(after, before + 2);
        assert_eq!(control.reads.load(Ordering::Relaxed), after);
    }

    #[test]
    fn failed_decode_keeps_destination_and_retries_without_publishing() {
        let track = ChdTrack {
            number: 1,
            mode: CdTrackMode::Mode1_2048,
            frames: 8,
            pregap: 0,
        };
        let mut data = logical_bytes(std::slice::from_ref(&track));
        sector(&mut data, 0)[..32].fill(0x42);
        let (image, control) = synthetic_image(data, 4 * CHD_UNIT_BYTES);
        let source = make_track_source(image, 0, &track);
        *control.fail_once.lock().unwrap() = Some(0);
        let mut output = [0xCC; 32];

        assert_eq!(
            source.read_exact_at(0, &mut output),
            Err(CdSourceError::ReadFailed)
        );
        assert_eq!(output, [0xCC; 32]);
        source.read_exact_at(0, &mut output).unwrap();
        assert_eq!(output, [0x42; 32]);
    }

    #[test]
    fn payload_visitor_failure_does_not_construct_disc() {
        let track = ChdTrack {
            number: 1,
            mode: CdTrackMode::Mode1_2048,
            frames: 8,
            pregap: 0,
        };
        let data = logical_bytes(std::slice::from_ref(&track));
        let (image, control) = synthetic_image(data, 4 * CHD_UNIT_BYTES);
        *control.fail_once.lock().unwrap() = Some(1);

        assert!(matches!(
            build_source_disc(std::slice::from_ref(&track), image.clone()),
            Err(PceCdLoadError::Disc(_))
        ));
        assert_eq!(control.reads.load(Ordering::Relaxed), 2);
        assert!(build_source_disc(std::slice::from_ref(&track), image).is_ok());
        assert_eq!(control.reads.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn cached_hunk_rechecks_file_identity() {
        let track = ChdTrack {
            number: 1,
            mode: CdTrackMode::Mode1_2048,
            frames: 4,
            pregap: 0,
        };
        let data = logical_bytes(std::slice::from_ref(&track));
        let (image, control) = synthetic_image(data, 4 * CHD_UNIT_BYTES);
        let source = make_track_source(image, 0, &track);
        let mut too_large = [0xCC; SOURCE_READ_STAGING_BYTES + 1];
        assert_eq!(
            source.read_exact_at(0, &mut too_large),
            Err(CdSourceError::ReadFailed)
        );
        assert_eq!(too_large, [0xCC; SOURCE_READ_STAGING_BYTES + 1]);
        control.valid.store(false, Ordering::Release);
        let mut output = [0xCC; 16];

        assert_eq!(
            source.read_exact_at(0, &mut output),
            Err(CdSourceError::ReadFailed)
        );
        assert_eq!(output, [0xCC; 16]);
    }

    fn raw_header(
        hunk_bytes: u32,
        logical_bytes: u64,
        map_offset: u64,
        metadata: u64,
    ) -> [u8; 124] {
        let mut header = [0; 124];
        header[..8].copy_from_slice(b"MComprHD");
        header[8..12].copy_from_slice(&124_u32.to_be_bytes());
        header[12..16].copy_from_slice(&5_u32.to_be_bytes());
        header[32..40].copy_from_slice(&logical_bytes.to_be_bytes());
        header[40..48].copy_from_slice(&map_offset.to_be_bytes());
        header[48..56].copy_from_slice(&metadata.to_be_bytes());
        header[56..60].copy_from_slice(&hunk_bytes.to_be_bytes());
        header[60..64].copy_from_slice(&(CHD_UNIT_BYTES as u32).to_be_bytes());
        header
    }

    fn temp_chd(name: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "zeff-pce-chd-source-{}-{name}.chd",
            std::process::id()
        ));
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn uncompressed_v5_chd() -> Vec<u8> {
        let hunk_bytes = 4 * CHD_UNIT_BYTES;
        let first =
            b"TRACK:1 TYPE:MODE1 SUBTYPE:NONE FRAMES:4 PREGAP:0 PGTYPE:VMODE1 PGSUB:NONE POSTGAP:0";
        let second =
            b"TRACK:2 TYPE:AUDIO SUBTYPE:NONE FRAMES:4 PREGAP:0 PGTYPE:VAUDIO PGSUB:NONE POSTGAP:0";
        let metadata_offset = 132_usize;
        let second_offset = metadata_offset + 16 + first.len();
        let mut bytes = vec![0; 3 * hunk_bytes];
        let header = raw_header(
            hunk_bytes as u32,
            (2 * hunk_bytes) as u64,
            CHD_HEADER_BYTES as u64,
            metadata_offset as u64,
        );
        bytes[..CHD_HEADER_BYTES].copy_from_slice(&header);
        bytes[124..128].copy_from_slice(&1_u32.to_be_bytes());
        bytes[128..132].copy_from_slice(&2_u32.to_be_bytes());
        bytes[metadata_offset..metadata_offset + 4].copy_from_slice(b"CHT2");
        bytes[metadata_offset + 4..metadata_offset + 8]
            .copy_from_slice(&(first.len() as u32).to_be_bytes());
        bytes[metadata_offset + 8..metadata_offset + 16]
            .copy_from_slice(&(second_offset as u64).to_be_bytes());
        bytes[metadata_offset + 16..metadata_offset + 16 + first.len()].copy_from_slice(first);
        bytes[second_offset..second_offset + 4].copy_from_slice(b"CHT2");
        bytes[second_offset + 4..second_offset + 8]
            .copy_from_slice(&(second.len() as u32).to_be_bytes());
        bytes[second_offset + 8..second_offset + 16].fill(0);
        bytes[second_offset + 16..second_offset + 16 + second.len()].copy_from_slice(second);
        bytes[hunk_bytes] = 0x31;
        bytes[2 * hunk_bytes..2 * hunk_bytes + 4].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        bytes
    }

    #[test]
    fn real_uncompressed_v5_loader_uses_native_sources() {
        let bytes = uncompressed_v5_chd();
        let path = temp_chd("uncompressed-v5", &bytes);
        let loaded = load_direct_chd_with_mods(&path, false).unwrap();

        assert_eq!(loaded.disc.read_user_sector(0).unwrap()[0], 0x31);
        assert_eq!(
            loaded.disc.read_audio_sample(4, 0).unwrap(),
            (0x1234, 0x5678)
        );
        drop(loaded);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn self_chain_depth_is_bounded() {
        let mut depths = vec![0];
        for target in 0..CHD_SELF_CHAIN_LIMIT {
            depths.push(self_depth(&depths, u64::from(target)).unwrap());
        }
        assert_eq!(depths.last(), Some(&CHD_SELF_CHAIN_LIMIT));
        assert!(matches!(
            self_depth(&depths, CHD_SELF_CHAIN_LIMIT as u64),
            Err(PceCdLoadError::InvalidChdMetadata)
        ));
    }

    #[test]
    fn preflight_rejects_oversized_hunks_and_compressed_maps() {
        let header = raw_header(
            CHD_HUNK_BYTES_LIMIT as u32 + CHD_UNIT_BYTES as u32,
            CHD_UNIT_BYTES as u64 * 4,
            124,
            0,
        );
        let path = temp_chd("large-hunk", &header);
        let mut file = File::open(&path).unwrap();
        assert!(matches!(
            read_raw_header(&mut file, header.len() as u64, &path),
            Err(PceCdLoadError::InvalidChdMetadata)
        ));

        let mut compressed = raw_header(
            (4 * CHD_UNIT_BYTES) as u32,
            CHD_UNIT_BYTES as u64 * 4,
            124,
            0,
        );
        compressed[16..20].copy_from_slice(&u32::from_be_bytes(*b"cdzl").to_be_bytes());
        let mut bytes = compressed.to_vec();
        bytes.extend_from_slice(&(CHD_MAP_BYTES_LIMIT as u32 + 1).to_be_bytes());
        let path = temp_chd("large-map", &bytes);
        let mut file = File::open(&path).unwrap();
        assert!(matches!(
            read_raw_header(&mut file, bytes.len() as u64, &path),
            Err(PceCdLoadError::InvalidChdMetadata)
        ));
    }

    #[test]
    fn metadata_parser_rejects_cycles_and_oversized_records() {
        let metadata_offset = 140_u64;
        let header = raw_header(
            (4 * CHD_UNIT_BYTES) as u32,
            CHD_UNIT_BYTES as u64 * 4,
            124,
            metadata_offset,
        );
        let mut bytes = header.to_vec();
        bytes.resize(metadata_offset as usize + 32, 0);
        bytes[metadata_offset as usize..metadata_offset as usize + 4].copy_from_slice(b"TEST");
        bytes[metadata_offset as usize + 4..metadata_offset as usize + 8]
            .copy_from_slice(&1_u32.to_be_bytes());
        bytes[metadata_offset as usize + 8..metadata_offset as usize + 16]
            .copy_from_slice(&metadata_offset.to_be_bytes());
        let path = temp_chd("metadata-cycle", &bytes);
        let mut file = File::open(&path).unwrap();
        let raw = read_raw_header(&mut file, bytes.len() as u64, &path).unwrap();
        assert!(matches!(
            read_tracks(&mut file, &raw, bytes.len() as u64, &path),
            Err(PceCdLoadError::InvalidChdMetadata)
        ));

        bytes[metadata_offset as usize + 4..metadata_offset as usize + 8]
            .copy_from_slice(&0x00FF_FFFF_u32.to_be_bytes());
        bytes[metadata_offset as usize + 8..metadata_offset as usize + 16].fill(0);
        let path = temp_chd("metadata-large", &bytes);
        let mut file = File::open(&path).unwrap();
        let raw = read_raw_header(&mut file, bytes.len() as u64, &path).unwrap();
        assert!(matches!(
            read_tracks(&mut file, &raw, bytes.len() as u64, &path),
            Err(PceCdLoadError::InvalidChdMetadata)
        ));
    }
}
