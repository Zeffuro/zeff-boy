use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop};

mod isolated;
mod native;
mod profiles;
mod sampled;
mod sequence;
#[cfg(any(test, feature = "test-support"))]
mod tests;

pub use isolated::supports_prepared_cartridge;
pub use native::prepare_rom;
#[cfg(feature = "test-support")]
pub use sampled::tests::synthetic_rom as synthetic_rom_sampled;
#[cfg(feature = "test-support")]
pub use tests::synthetic_rom;
#[cfg(feature = "test-support")]
pub use tests::synthetic_rom_rocket;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GbQuickThunderHardware {
    CgbDouble,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbQuickThunderSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub bank: u16,
    pub hardware: GbQuickThunderHardware,
    pub table_entry: RomSpan,
    pub tracks: Vec<GbQuickThunderTrack>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct GbQuickThunderTrack {
    pub number: u8,
    pub note_count: u32,
}

pub struct PreparedGbQuickThunder {
    pub bytes: Vec<u8>,
    pub hardware: GbQuickThunderHardware,
    pub ready_address: u16,
    pub ready_value: u8,
    pub ack_address: u16,
    pub ack_value: u8,
    pub wait_start: u16,
    pub wait_end: u16,
}

pub fn supports_cartridge(bytes: &[u8]) -> bool {
    bytes.len() >= 0x8000
        && matches!(bytes[0x143], 0x80 | 0xc0)
        && (0x19..=0x1e).contains(&bytes[0x147])
        && bytes[0x148] <= 8
        && bytes.len() == (0x8000usize << bytes[0x148])
}

#[derive(Debug)]
enum ReadError {
    Invalid,
    Stop(ScanStop),
}

impl From<ScanStop> for ReadError {
    fn from(value: ScanStop) -> Self {
        Self::Stop(value)
    }
}

pub(crate) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbQuickThunderSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    for driver in profiles::recognized(bytes, budget)? {
        for index in 0..=255 {
            budget.charge()?;
            match song(bytes, driver, index, budget) {
                Ok(song) => {
                    if songs.len() >= remaining {
                        return Err(ScanStop::CandidateLimit);
                    }
                    songs.push(song);
                }
                Err(ReadError::Invalid) => (),
                Err(ReadError::Stop(stop)) => return Err(stop),
            }
        }
    }
    Ok(())
}

fn song(
    bytes: &[u8],
    driver: profiles::Driver,
    index: u16,
    budget: &mut Budget<'_>,
) -> Result<GbQuickThunderSong, ReadError> {
    if matches!(bytes[0x147], 0x97 | 0x99)
        && driver.profile.name == "gb-quickthunder-14-05"
        && index != 3
    {
        return Err(ReadError::Invalid);
    }
    let sampled = sampled::is_sampled(driver);
    if sampled && ![1, 4, 5, 6].contains(&index) {
        return Err(ReadError::Invalid);
    }
    let mut reader = sequence::Reader::new(bytes, driver, budget);
    if driver.profile.len == 2069 {
        if bytes[0] == 1 {
            return Err(ReadError::Invalid);
        }
        reader.mapped.push((0, 1));
    }
    let stride = usize::from(driver.profile.stride);
    let table = u32::from(driver.table) + u32::from(index) * stride as u32;
    if table + stride as u32 > u32::from(driver.sequences) {
        return Err(ReadError::Invalid);
    }
    let header = reader.read(table, stride)?.to_vec();
    let (tracks_at, speed_at, wave_at) = if stride == 14 { (2, 0, 10) } else { (0, 8, 9) };
    if !(1..=15).contains(&header[speed_at]) || stride == 14 && header[1] != 15 {
        return Err(ReadError::Invalid);
    }
    reader.read(u32::from(driver.frequency), 512)?;
    if !sampled {
        reader.effect(u32::from(word(&header, wave_at)), 2, true)?;
        reader.read(u32::from(word(&header, wave_at + 2)), 16)?;
        reader.effect(u32::from(driver.release), 1, false)?;
    }
    let mut tracks = Vec::new();
    for channel in 0..4 {
        tracks.push(GbQuickThunderTrack {
            number: channel as u8 + 1,
            note_count: reader.track(
                u32::from(word(&header, tracks_at + channel * 2)),
                channel as u8,
            )?,
        });
    }
    if tracks.iter().all(|track| track.note_count == 0) {
        return Err(ReadError::Invalid);
    }
    Ok(GbQuickThunderSong {
        profile: driver.profile.name,
        index,
        title: format!("Song {index}"),
        bank: driver.bank,
        hardware: GbQuickThunderHardware::CgbDouble,
        table_entry: span(driver.offset(table), stride),
        tracks,
        mapped_spans: spans(reader.mapped),
        warnings: if sampled {
            vec!["Uses the authenticated timer callback and banked wave samples. Music-only playback resets the APU and driver state, with nominal VBlank music ticks and hardware timer samples; gameplay effects, warm reselection and natural loop/end times are excluded.".to_owned()]
        } else if matches!(bytes[0x147], 0x97 | 0x99) {
            vec!["Isolates the qualified Rocket Games sound bank in a fixed-ROM CGB player; the source board is not MBC5. Music-only playback starts from zeroed driver state, uses nominal VBlank timing and stops at the requested duration.".to_owned()]
        } else {
            Vec::new()
        },
    })
}

fn checked_driver(
    bytes: &[u8],
    expected: &GbQuickThunderSong,
    cancel: &AtomicBool,
) -> anyhow::Result<profiles::Driver> {
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let drivers = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("Game Boy QuickThunder validation stopped: {stop:?}"))?;
    for driver in drivers {
        if driver.profile.name == expected.profile
            && driver.bank == expected.bank
            && expected.index <= 255
            && let Ok(checked) = song(bytes, driver, expected.index, &mut budget)
            && checked == *expected
        {
            return Ok(driver);
        }
    }
    anyhow::bail!("Game Boy QuickThunder inventory differs from its recognized source")
}

pub fn validate_song(
    bytes: &[u8],
    song: &GbQuickThunderSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    checked_driver(bytes, song, cancel).map(|_| ())
}

fn word(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}

fn span(offset: usize, len: usize) -> RomSpan {
    RomSpan {
        effective_offset: offset as u32,
        byte_len: len as u32,
        canonical_cpu_address: if offset < 0x4000 {
            offset
        } else {
            0x4000 + offset % 0x4000
        } as u32,
    }
}

fn spans(mut ranges: Vec<(usize, usize)>) -> Vec<RomSpan> {
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        if let Some(last) = merged.last_mut()
            && start / 0x4000 == last.0 / 0x4000
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged.into_iter().map(|(a, b)| span(a, b - a)).collect()
}
