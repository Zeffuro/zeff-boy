use sha2::{Digest, Sha256};
use std::error::Error;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

pub const CD_USER_SECTOR_BYTES: usize = 2_048;
pub const CD_RAW_SECTOR_BYTES: usize = 2_352;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdTrackMode {
    Audio,
    Mode1_2048,
    Mode1_2352,
}

pub trait CdTrackSource: Send + Sync {
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    fn payload_hash(&self) -> [u8; 32];
    fn read_exact_at(&self, offset: usize, buffer: &mut [u8]) -> Result<(), CdSourceError>;

    fn visit_payload(
        &self,
        sector_bytes: usize,
        visitor: &mut dyn FnMut(&[u8]),
    ) -> Result<(), CdSourceError> {
        if sector_bytes == 0
            || sector_bytes > CD_RAW_SECTOR_BYTES
            || !self.len().is_multiple_of(sector_bytes)
        {
            return Err(CdSourceError::ReadFailed);
        }
        let mut sector = [0; CD_RAW_SECTOR_BYTES];
        for offset in (0..self.len()).step_by(sector_bytes) {
            let buffer = &mut sector[..sector_bytes];
            self.read_exact_at(offset, buffer)?;
            visitor(buffer);
        }
        Ok(())
    }
}

#[derive(Clone)]
struct MemoryCdTrackSource {
    data: Arc<[u8]>,
    payload_hash: [u8; 32],
}

impl MemoryCdTrackSource {
    fn new(data: Vec<u8>) -> Self {
        let payload_hash = Sha256::digest(&data).into();
        Self {
            data: data.into(),
            payload_hash,
        }
    }
}

impl CdTrackSource for MemoryCdTrackSource {
    fn len(&self) -> usize {
        self.data.len()
    }

    fn payload_hash(&self) -> [u8; 32] {
        self.payload_hash
    }

    fn read_exact_at(&self, offset: usize, buffer: &mut [u8]) -> Result<(), CdSourceError> {
        let end = offset
            .checked_add(buffer.len())
            .filter(|&end| end <= self.data.len())
            .ok_or(CdSourceError::OutOfRange {
                offset,
                bytes: buffer.len(),
                source_len: self.data.len(),
            })?;
        buffer.copy_from_slice(&self.data[offset..end]);
        Ok(())
    }
}

#[derive(Clone)]
pub struct CdTrack {
    number: u8,
    control: u8,
    index0_lba: Option<u32>,
    index1_lba: u32,
    stored_start_lba: u32,
    mode: CdTrackMode,
    source: Arc<dyn CdTrackSource>,
    source_len: usize,
    payload_hash: [u8; 32],
    expected_payload_hash: Option<[u8; 32]>,
}

enum CdTrackPayload {
    Index1(Arc<dyn CdTrackSource>),
    UnverifiedIndex1(Arc<dyn CdTrackSource>),
    Retained(Arc<dyn CdTrackSource>),
    UnverifiedRetained(Arc<dyn CdTrackSource>),
}

impl CdTrack {
    pub fn from_index1_data(
        number: u8,
        control: u8,
        index0_lba: Option<u32>,
        index1_lba: u32,
        mode: CdTrackMode,
        index1_data: Vec<u8>,
    ) -> Result<Self, CdDiscError> {
        Self::new(
            number,
            control,
            index0_lba,
            index1_lba,
            mode,
            CdTrackPayload::Index1(Arc::new(MemoryCdTrackSource::new(index1_data))),
        )
    }

    pub fn from_index1_source(
        number: u8,
        control: u8,
        index0_lba: Option<u32>,
        index1_lba: u32,
        mode: CdTrackMode,
        source: Arc<dyn CdTrackSource>,
    ) -> Result<Self, CdDiscError> {
        Self::new(
            number,
            control,
            index0_lba,
            index1_lba,
            mode,
            CdTrackPayload::Index1(source),
        )
    }

    pub fn from_index1_unverified_source(
        number: u8,
        control: u8,
        index0_lba: Option<u32>,
        index1_lba: u32,
        mode: CdTrackMode,
        source: Arc<dyn CdTrackSource>,
    ) -> Result<Self, CdDiscError> {
        Self::new(
            number,
            control,
            index0_lba,
            index1_lba,
            mode,
            CdTrackPayload::UnverifiedIndex1(source),
        )
    }

    pub fn from_stored_data(
        number: u8,
        control: u8,
        index0_lba: Option<u32>,
        index1_lba: u32,
        mode: CdTrackMode,
        stored_data: Vec<u8>,
    ) -> Result<Self, CdDiscError> {
        Self::new(
            number,
            control,
            index0_lba,
            index1_lba,
            mode,
            CdTrackPayload::Retained(Arc::new(MemoryCdTrackSource::new(stored_data))),
        )
    }

    pub fn from_stored_source(
        number: u8,
        control: u8,
        index0_lba: Option<u32>,
        index1_lba: u32,
        mode: CdTrackMode,
        source: Arc<dyn CdTrackSource>,
    ) -> Result<Self, CdDiscError> {
        Self::new(
            number,
            control,
            index0_lba,
            index1_lba,
            mode,
            CdTrackPayload::Retained(source),
        )
    }

    pub fn from_stored_unverified_source(
        number: u8,
        control: u8,
        index0_lba: Option<u32>,
        index1_lba: u32,
        mode: CdTrackMode,
        source: Arc<dyn CdTrackSource>,
    ) -> Result<Self, CdDiscError> {
        Self::new(
            number,
            control,
            index0_lba,
            index1_lba,
            mode,
            CdTrackPayload::UnverifiedRetained(source),
        )
    }

    fn new(
        number: u8,
        control: u8,
        index0_lba: Option<u32>,
        index1_lba: u32,
        mode: CdTrackMode,
        payload: CdTrackPayload,
    ) -> Result<Self, CdDiscError> {
        let (stored_start_lba, source, verify_payload_hash) = match payload {
            CdTrackPayload::Index1(data) => (index1_lba, data, true),
            CdTrackPayload::UnverifiedIndex1(data) => (index1_lba, data, false),
            CdTrackPayload::Retained(data) => (index0_lba.unwrap_or(index1_lba), data, true),
            CdTrackPayload::UnverifiedRetained(data) => {
                (index0_lba.unwrap_or(index1_lba), data, false)
            }
        };
        let source_len = source.len();
        let expected_payload_hash = verify_payload_hash.then(|| source.payload_hash());
        let payload_hash = expected_payload_hash.unwrap_or_default();
        let sector_bytes = match mode {
            CdTrackMode::Audio | CdTrackMode::Mode1_2352 => CD_RAW_SECTOR_BYTES,
            CdTrackMode::Mode1_2048 => CD_USER_SECTOR_BYTES,
        };
        if number == 0 || source_len == 0 || !source_len.is_multiple_of(sector_bytes) {
            return Err(CdDiscError::InvalidTrackLength {
                number,
                bytes: source_len,
                sector_bytes,
            });
        }
        if index0_lba.is_some_and(|lba| lba > index1_lba)
            || stored_start_lba > index1_lba
            || index0_lba
                .is_some_and(|index0| stored_start_lba != index0 && stored_start_lba != index1_lba)
        {
            return Err(CdDiscError::InvalidPregap { number });
        }
        Ok(Self {
            number,
            control: control & 0x0F,
            index0_lba,
            index1_lba,
            stored_start_lba,
            mode,
            source,
            source_len,
            payload_hash,
            expected_payload_hash,
        })
    }

    #[inline]
    pub const fn number(&self) -> u8 {
        self.number
    }

    #[inline]
    pub const fn control(&self) -> u8 {
        self.control
    }

    #[inline]
    pub const fn index0_lba(&self) -> Option<u32> {
        self.index0_lba
    }

    #[inline]
    pub const fn index1_lba(&self) -> u32 {
        self.index1_lba
    }

    #[inline]
    pub const fn stored_start_lba(&self) -> u32 {
        self.stored_start_lba
    }

    #[inline]
    pub const fn mode(&self) -> CdTrackMode {
        self.mode
    }

    pub fn sector_count(&self) -> u32 {
        let bytes = match self.mode {
            CdTrackMode::Audio | CdTrackMode::Mode1_2352 => CD_RAW_SECTOR_BYTES,
            CdTrackMode::Mode1_2048 => CD_USER_SECTOR_BYTES,
        };
        (self.source_len / bytes) as u32
    }

    #[inline]
    pub fn end_lba(&self) -> u32 {
        self.stored_start_lba.saturating_add(self.sector_count())
    }

    pub fn payload_hash(&self) -> [u8; 32] {
        self.payload_hash
    }
}

impl std::fmt::Debug for CdTrack {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CdTrack")
            .field("number", &self.number)
            .field("control", &self.control)
            .field("index0_lba", &self.index0_lba)
            .field("index1_lba", &self.index1_lba)
            .field("stored_start_lba", &self.stored_start_lba)
            .field("mode", &self.mode)
            .field("source_len", &self.source_len)
            .field("payload_hash", &self.payload_hash)
            .finish()
    }
}

impl PartialEq for CdTrack {
    fn eq(&self, other: &Self) -> bool {
        self.number == other.number
            && self.control == other.control
            && self.index0_lba == other.index0_lba
            && self.index1_lba == other.index1_lba
            && self.stored_start_lba == other.stored_start_lba
            && self.mode == other.mode
            && self.source_len == other.source_len
            && self.payload_hash == other.payload_hash
    }
}

impl Eq for CdTrack {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CdDisc {
    tracks: Box<[CdTrack]>,
    leadout_lba: u32,
    content_hash: [u8; 32],
}

impl CdDisc {
    pub fn new(mut tracks: Vec<CdTrack>) -> Result<Self, CdDiscError> {
        if tracks.is_empty() {
            return Err(CdDiscError::Empty);
        }
        tracks.sort_by_key(CdTrack::number);
        for pair in tracks.windows(2) {
            if pair[0].number == pair[1].number {
                return Err(CdDiscError::DuplicateTrack(pair[0].number));
            }
            if pair[0].end_lba() > pair[1].index0_lba.unwrap_or(pair[1].index1_lba) {
                return Err(CdDiscError::OverlappingTracks {
                    first: pair[0].number,
                    second: pair[1].number,
                });
            }
        }
        let leadout_lba = tracks.iter().map(CdTrack::end_lba).max().unwrap();
        let mut tracks = tracks.into_boxed_slice();
        let content_hash = hash_disc(&mut tracks)?;
        Ok(Self {
            tracks,
            leadout_lba,
            content_hash,
        })
    }

    #[inline]
    pub const fn content_hash(&self) -> [u8; 32] {
        self.content_hash
    }

    #[inline]
    pub fn tracks(&self) -> &[CdTrack] {
        &self.tracks
    }

    #[inline]
    pub fn first_track(&self) -> u8 {
        self.tracks[0].number
    }

    #[inline]
    pub fn last_track(&self) -> u8 {
        self.tracks[self.tracks.len() - 1].number
    }

    #[inline]
    pub const fn leadout_lba(&self) -> u32 {
        self.leadout_lba
    }

    pub fn track(&self, number: u8) -> Option<&CdTrack> {
        self.tracks.iter().find(|track| track.number == number)
    }

    pub fn timeline_track_at_lba(&self, lba: u32) -> Option<&CdTrack> {
        self.tracks.iter().find(|track| {
            (track.index0_lba.unwrap_or(track.index1_lba)..track.end_lba()).contains(&lba)
        })
    }

    pub(crate) fn stored_track_index_at_lba(&self, lba: u32) -> Option<usize> {
        let index = self
            .tracks
            .partition_point(|track| track.stored_start_lba <= lba)
            .checked_sub(1)?;
        (lba < self.tracks[index].end_lba()).then_some(index)
    }

    fn stored_track_at_lba(&self, lba: u32) -> Option<&CdTrack> {
        self.stored_track_index_at_lba(lba)
            .map(|index| &self.tracks[index])
    }

    pub fn read_audio_sample(&self, lba: u32, sample: usize) -> Result<(i16, i16), CdReadError> {
        let track = self
            .stored_track_at_lba(lba)
            .ok_or(CdReadError::LbaOutOfRange(lba))?;
        Self::read_audio_sample_from_track(track, lba, sample)
    }

    pub(crate) fn read_audio_sector_from_track_index(
        &self,
        track_index: usize,
        lba: u32,
        sector: &mut [u8; CD_RAW_SECTOR_BYTES],
    ) -> Option<Result<(), CdReadError>> {
        let track = self.tracks.get(track_index)?;
        ((track.stored_start_lba..track.end_lba()).contains(&lba)).then(|| {
            if track.mode != CdTrackMode::Audio {
                return Err(CdReadError::DataTrack {
                    lba,
                    track: track.number,
                });
            }
            let offset = (lba - track.stored_start_lba) as usize * CD_RAW_SECTOR_BYTES;
            track
                .source
                .read_exact_at(offset, sector)
                .map_err(|source| CdReadError::Source {
                    lba,
                    track: track.number,
                    source,
                })
        })
    }

    fn read_audio_sample_from_track(
        track: &CdTrack,
        lba: u32,
        sample: usize,
    ) -> Result<(i16, i16), CdReadError> {
        if track.mode != CdTrackMode::Audio {
            return Err(CdReadError::DataTrack {
                lba,
                track: track.number,
            });
        }
        if sample >= 588 {
            return Err(CdReadError::AudioSampleOutOfRange(sample));
        }
        let sector = (lba - track.stored_start_lba) as usize;
        let offset = sector * CD_RAW_SECTOR_BYTES + sample * 4;
        let mut frame = [0; 4];
        track
            .source
            .read_exact_at(offset, &mut frame)
            .map_err(|source| CdReadError::Source {
                lba,
                track: track.number,
                source,
            })?;
        Ok((
            i16::from_le_bytes(frame[..2].try_into().unwrap()),
            i16::from_le_bytes(frame[2..].try_into().unwrap()),
        ))
    }

    pub fn read_user_sector(&self, lba: u32) -> Result<[u8; CD_USER_SECTOR_BYTES], CdReadError> {
        let track = self
            .stored_track_at_lba(lba)
            .ok_or(CdReadError::LbaOutOfRange(lba))?;
        if track.mode == CdTrackMode::Audio {
            return Err(CdReadError::AudioTrack {
                lba,
                track: track.number,
            });
        }
        let relative = (lba - track.stored_start_lba) as usize;
        let stride = match track.mode {
            CdTrackMode::Mode1_2048 => CD_USER_SECTOR_BYTES,
            CdTrackMode::Mode1_2352 => CD_RAW_SECTOR_BYTES,
            CdTrackMode::Audio => unreachable!(),
        };
        let offset = relative * stride;
        let source_offset = match track.mode {
            CdTrackMode::Mode1_2048 => offset,
            CdTrackMode::Mode1_2352 => offset + 16,
            CdTrackMode::Audio => unreachable!(),
        };
        let mut sector = [0; CD_USER_SECTOR_BYTES];
        track
            .source
            .read_exact_at(source_offset, &mut sector)
            .map_err(|source| CdReadError::Source {
                lba,
                track: track.number,
                source,
            })?;
        Ok(sector)
    }
}

fn hash_disc(tracks: &mut [CdTrack]) -> Result<[u8; 32], CdDiscError> {
    let mut hasher = Sha256::new();
    hasher.update(b"zeff-boy:pce-core-cd-disc:v1\0");
    hasher.update((tracks.len() as u32).to_le_bytes());
    for track in tracks {
        hasher.update([track.number, track.control]);
        match track.index0_lba {
            Some(lba) => {
                hasher.update([1]);
                hasher.update(lba.to_le_bytes());
            }
            None => hasher.update([0]),
        }
        hasher.update(track.index1_lba.to_le_bytes());
        hasher.update(track.stored_start_lba.to_le_bytes());
        hasher.update([match track.mode {
            CdTrackMode::Audio => 0,
            CdTrackMode::Mode1_2048 => 1,
            CdTrackMode::Mode1_2352 => 2,
        }]);
        hasher.update((track.source_len as u64).to_le_bytes());
        let sector_bytes = match track.mode {
            CdTrackMode::Audio | CdTrackMode::Mode1_2352 => CD_RAW_SECTOR_BYTES,
            CdTrackMode::Mode1_2048 => CD_USER_SECTOR_BYTES,
        };
        let mut payload_hasher = Sha256::new();
        track
            .source
            .visit_payload(sector_bytes, &mut |buffer| {
                hasher.update(buffer);
                payload_hasher.update(buffer);
            })
            .map_err(|source| CdDiscError::Source {
                track: track.number,
                source,
            })?;
        let payload_hash = <[u8; 32]>::from(payload_hasher.finalize());
        if track
            .expected_payload_hash
            .is_some_and(|expected| expected != payload_hash)
        {
            return Err(CdDiscError::PayloadHashMismatch(track.number));
        }
        track.payload_hash = payload_hash;
    }
    Ok(hasher.finalize().into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdSourceError {
    OutOfRange {
        offset: usize,
        bytes: usize,
        source_len: usize,
    },
    ReadFailed,
}

impl Display for CdSourceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "PC Engine CD source read failed: {self:?}")
    }
}

impl Error for CdSourceError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdDiscError {
    Empty,
    InvalidTrackLength {
        number: u8,
        bytes: usize,
        sector_bytes: usize,
    },
    InvalidPregap {
        number: u8,
    },
    DuplicateTrack(u8),
    OverlappingTracks {
        first: u8,
        second: u8,
    },
    Source {
        track: u8,
        source: CdSourceError,
    },
    PayloadHashMismatch(u8),
}

impl Display for CdDiscError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "invalid PC Engine CD image: {self:?}")
    }
}

impl Error for CdDiscError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdReadError {
    LbaOutOfRange(u32),
    AudioTrack {
        lba: u32,
        track: u8,
    },
    DataTrack {
        lba: u32,
        track: u8,
    },
    AudioSampleOutOfRange(usize),
    Source {
        lba: u32,
        track: u8,
        source: CdSourceError,
    },
}

impl Display for CdReadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "PC Engine CD sector read failed: {self:?}")
    }
}

impl Error for CdReadError {}
