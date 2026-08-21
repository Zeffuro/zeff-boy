use std::error::Error;
use std::fmt::{Display, Formatter};

pub const CD_USER_SECTOR_BYTES: usize = 2_048;
pub const CD_RAW_SECTOR_BYTES: usize = 2_352;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CdTrackMode {
    Audio,
    Mode1_2048,
    Mode1_2352,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CdTrack {
    number: u8,
    control: u8,
    index0_lba: Option<u32>,
    index1_lba: u32,
    stored_start_lba: u32,
    mode: CdTrackMode,
    stored_data: Box<[u8]>,
}

enum CdTrackPayload {
    Index1(Vec<u8>),
    Retained(Vec<u8>),
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
            CdTrackPayload::Index1(index1_data),
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
            CdTrackPayload::Retained(stored_data),
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
        let (stored_start_lba, stored_data) = match payload {
            CdTrackPayload::Index1(data) => (index1_lba, data),
            CdTrackPayload::Retained(data) => (index0_lba.unwrap_or(index1_lba), data),
        };
        let sector_bytes = match mode {
            CdTrackMode::Audio | CdTrackMode::Mode1_2352 => CD_RAW_SECTOR_BYTES,
            CdTrackMode::Mode1_2048 => CD_USER_SECTOR_BYTES,
        };
        if number == 0 || stored_data.is_empty() || !stored_data.len().is_multiple_of(sector_bytes)
        {
            return Err(CdDiscError::InvalidTrackLength {
                number,
                bytes: stored_data.len(),
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
            stored_data: stored_data.into_boxed_slice(),
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
        (self.stored_data.len() / bytes) as u32
    }

    #[inline]
    pub fn end_lba(&self) -> u32 {
        self.stored_start_lba.saturating_add(self.sector_count())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CdDisc {
    tracks: Box<[CdTrack]>,
    leadout_lba: u32,
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
        Ok(Self {
            tracks: tracks.into_boxed_slice(),
            leadout_lba,
        })
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

    fn stored_track_at_lba(&self, lba: u32) -> Option<&CdTrack> {
        self.tracks
            .iter()
            .find(|track| (track.stored_start_lba..track.end_lba()).contains(&lba))
    }

    pub fn read_audio_sample(&self, lba: u32, sample: usize) -> Result<(i16, i16), CdReadError> {
        let track = self
            .stored_track_at_lba(lba)
            .ok_or(CdReadError::LbaOutOfRange(lba))?;
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
        Ok((
            i16::from_le_bytes(track.stored_data[offset..offset + 2].try_into().unwrap()),
            i16::from_le_bytes(
                track.stored_data[offset + 2..offset + 4]
                    .try_into()
                    .unwrap(),
            ),
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
        let source = match track.mode {
            CdTrackMode::Mode1_2048 => &track.stored_data[offset..offset + CD_USER_SECTOR_BYTES],
            CdTrackMode::Mode1_2352 => {
                &track.stored_data[offset + 16..offset + 16 + CD_USER_SECTOR_BYTES]
            }
            CdTrackMode::Audio => unreachable!(),
        };
        Ok(source.try_into().unwrap())
    }
}

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
    AudioTrack { lba: u32, track: u8 },
    DataTrack { lba: u32, track: u8 },
    AudioSampleOutOfRange(usize),
}

impl Display for CdReadError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "PC Engine CD sector read failed: {self:?}")
    }
}

impl Error for CdReadError {}
