use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop};

mod legacy;
#[cfg(any(test, feature = "test-support"))]
mod legacy_tests;
mod native;
mod profiles;
mod sequence;
#[cfg(any(test, feature = "test-support"))]
mod tests;

#[cfg(feature = "test-support")]
pub use legacy_tests::synthetic_legacy_rom;
pub use native::prepare_rom;
#[cfg(feature = "test-support")]
pub use tests::synthetic_rom;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WsToseHardware {
    Mono,
    Color,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WsToseSong {
    pub profile: &'static str,
    pub hardware: WsToseHardware,
    pub index: u16,
    pub title: String,
    pub table_entry: RomSpan,
    pub tracks: Vec<WsToseTrack>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WsToseTrack {
    pub number: u8,
    pub slot: u8,
    pub note_count: u32,
}

pub struct PreparedWsTose {
    pub bytes: Vec<u8>,
    pub bootstrap: WsToseBootstrap,
    pub hardware: WsToseHardware,
    pub ready_address: u32,
    pub ack_address: u32,
    pub wait_start: u32,
    pub wait_end: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WsToseBootstrap {
    Cartridge,
    Ram,
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
    songs: &mut Vec<WsToseSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    if legacy::scan(bytes, songs, budget, remaining)? {
        return Ok(());
    }
    let Some(profile) = profiles::recognized(bytes, budget)? else {
        return Ok(());
    };
    for index in 0..profile.counts.iter().sum() {
        match song(bytes, profile, index, budget) {
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
    Ok(())
}

fn song(
    bytes: &[u8],
    profile: profiles::Profile,
    index: u16,
    budget: &mut Budget<'_>,
) -> Result<WsToseSong, ReadError> {
    let mut local = index;
    let group = profile
        .counts
        .iter()
        .position(|&count| {
            if local < count {
                true
            } else {
                local -= count;
                false
            }
        })
        .ok_or(ReadError::Invalid)?;
    let threshold = profile.fixed + group * 6;
    let table = usize::from(word(bytes, threshold + 2)?);
    let bank = usize::from(word(bytes, threshold + 4)?) % (bytes.len() / 0x10000);
    let mut reader = sequence::Reader::new(bytes, bank, profile, budget);
    let at = table + usize::from(local) * 2;
    let pointer = usize::from(reader.word(at)?);
    let end = usize::from(reader.word(pointer + 4)?);
    if pointer < table + usize::from(profile.counts[group]) * 2
        || end <= pointer
        || end - pointer > 48
        || !(end - pointer).is_multiple_of(6)
    {
        return Err(ReadError::Invalid);
    }
    let table_entry = span(bank, at, 2);
    let mut slots = 0_u8;
    let mut tracks = Vec::new();
    for row in (pointer..end).step_by(6) {
        let slot = reader.word(row)?;
        let channel = reader.word(row + 2)?;
        let track = reader.word(row + 4)?;
        if !slot.is_multiple_of(0x34) || slot / 0x34 >= 8 || channel > 3 {
            return Err(ReadError::Invalid);
        }
        let slot = (slot / 0x34) as u8;
        if slots & (1 << slot) != 0 {
            return Err(ReadError::Invalid);
        }
        slots |= 1 << slot;
        tracks.push(WsToseTrack {
            number: channel as u8 + 1,
            slot,
            note_count: reader.track(track, channel as u8)?,
        });
    }
    if tracks.iter().all(|track| track.note_count == 0) {
        return Err(ReadError::Invalid);
    }
    let mut warnings = vec![
        "Native driver selection; soundtrack membership and duration are not established.".into(),
    ];
    if bytes[bytes.len() - 9] == 0 {
        warnings.push("The original program requires WonderSwan Color despite its cartridge footer; isolated playback uses Color hardware.".into());
    }
    Ok(WsToseSong {
        profile: profile.name,
        hardware: WsToseHardware::Color,
        index,
        title: format!("Audio selector {index}"),
        table_entry,
        tracks,
        mapped_spans: merged(reader.mapped),
        warnings,
    })
}

fn checked_profile(
    bytes: &[u8],
    expected: &WsToseSong,
    cancel: &AtomicBool,
) -> anyhow::Result<profiles::Profile> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = profiles::recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("WonderSwan TOSE validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Unrecognized WonderSwan TOSE source"))?;
    anyhow::ensure!(
        expected.profile == profile.name
            && song(bytes, profile, expected.index, &mut budget)
                .ok()
                .as_ref()
                == Some(expected),
        "WonderSwan TOSE inventory differs from its recognized source"
    );
    Ok(profile)
}

pub fn validate_song(bytes: &[u8], song: &WsToseSong, cancel: &AtomicBool) -> anyhow::Result<()> {
    if song.profile.starts_with("ws-tose-fixed-") {
        return legacy::validate_song(bytes, song, cancel);
    }
    checked_profile(bytes, song, cancel).map(|_| ())
}

fn word(bytes: &[u8], at: usize) -> Result<u16, ReadError> {
    let data = bytes.get(at..at + 2).ok_or(ReadError::Invalid)?;
    Ok(u16::from_le_bytes([data[0], data[1]]))
}

fn span(bank: usize, address: usize, len: usize) -> RomSpan {
    RomSpan {
        effective_offset: (bank * 0x10000 + address) as u32,
        byte_len: len as u32,
        canonical_cpu_address: 0x30000 + address as u32,
    }
}

fn merged(mut spans: Vec<RomSpan>) -> Vec<RomSpan> {
    spans.sort_unstable();
    let mut result: Vec<RomSpan> = Vec::new();
    for span in spans {
        if let Some(last) = result.last_mut()
            && last.effective_offset + last.byte_len >= span.effective_offset
            && last.canonical_cpu_address + span.effective_offset - last.effective_offset
                == span.canonical_cpu_address
        {
            last.byte_len = last
                .byte_len
                .max(span.effective_offset + span.byte_len - last.effective_offset);
        } else {
            result.push(span);
        }
    }
    result
}
