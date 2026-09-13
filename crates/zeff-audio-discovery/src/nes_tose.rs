use std::sync::atomic::AtomicBool;

use serde::Serialize;

use crate::{Budget, RomSpan, ScanStop};

mod closed;
mod closed_native;
mod closed_profiles;
#[cfg(any(test, feature = "test-support"))]
mod closed_tests;
mod fcg;
mod fcg_native;
mod fcg_sequence;
mod native;
mod sequence;
#[cfg(any(test, feature = "test-support"))]
mod tests;

#[cfg(feature = "test-support")]
pub use closed_tests::synthetic_closed_rom;
pub use native::prepare_rom;
#[cfg(feature = "test-support")]
pub use tests::{synthetic_fcg_rom, synthetic_rom};

const PROFILE: &str = "nes-tose-six-slot-gxrom-v1";
const GROUPS: [(u8, u8); 14] = [
    (0, 4),
    (4, 3),
    (7, 2),
    (9, 4),
    (13, 3),
    (16, 4),
    (20, 4),
    (24, 3),
    (27, 2),
    (29, 4),
    (33, 3),
    (36, 3),
    (39, 3),
    (42, 3),
];
const WITNESSES: [(u8, u16, u16, &str); 3] = [
    (
        0,
        0x98c0,
        0x98e1,
        "9ae20c8433cc898e71cc0fe6ef801b8f55f702beab8c71cbc7f0b16ef4f517cd",
    ),
    (
        3,
        0x8f68,
        0x9419,
        "a1f4e4c28d082c5f5daee2e9c552099bebfc55115999447c868abf9676adae4a",
    ),
    (
        3,
        0x98a8,
        0x98fc,
        "69cf6d5d7b0f6c72bd7056e511323afdb96496c245f3d105ef26b8600979936c",
    ),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesToseSong {
    pub profile: &'static str,
    pub index: u16,
    pub title: String,
    pub table_entry: RomSpan,
    pub tracks: Vec<NesToseTrack>,
    pub mapped_spans: Vec<RomSpan>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NesToseTrack {
    pub number: u8,
    pub note_count: u32,
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
    songs: &mut Vec<NesToseSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    if !recognized(bytes, budget)? {
        fcg::scan(bytes, songs, budget, remaining)?;
        return closed::scan(bytes, songs, budget, remaining);
    }
    for (index, count) in GROUPS {
        match song(bytes, index, count, budget) {
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

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<bool, ScanStop> {
    budget.charge()?;
    if bytes.len() != 0x28010 || bytes.get(..16) != Some(b"NES\x1a\x08\x04\x20\x40\0\0\0\0\0\0\0\0")
    {
        return Ok(false);
    }
    for &(bank, start, end, hash) in &WITNESSES {
        let range = span(bank, start, usize::from(end - start));
        for _ in 0..range.byte_len.div_ceil(256) {
            budget.charge()?;
        }
        let offset = range.effective_offset as usize;
        let actual = zeff_firmware::sha256_hex(&bytes[offset..offset + range.byte_len as usize]);
        if actual != hash {
            #[cfg(any(test, feature = "test-support"))]
            if tests::fixture_witness(bank, start, &actual) {
                continue;
            }
            return Ok(false);
        }
    }
    Ok(true)
}

fn song(
    bytes: &[u8],
    index: u8,
    count: u8,
    budget: &mut Budget<'_>,
) -> Result<NesToseSong, ReadError> {
    let table_entry = span(3, 0x8040 + u16::from(index) * 4, usize::from(count) * 4);
    let offset = table_entry.effective_offset as usize;
    let entries = &bytes[offset..offset + table_entry.byte_len as usize];
    let mut mapped = WITNESSES
        .iter()
        .map(|&(bank, start, end, _)| span(bank, start, usize::from(end - start)))
        .collect::<Vec<_>>();
    mapped.push(table_entry);
    let mut tracks = Vec::new();
    let mut channels = 0_u8;
    for (number, entry) in entries.as_chunks::<4>().0.iter().enumerate() {
        budget.charge()?;
        let expected_slot = if index == 7 { 60 } else { 120 - count * 20 } + number as u8 * 20;
        if entry[0] != expected_slot || entry[1] > 3 || channels & (1 << entry[1]) != 0 {
            return Err(ReadError::Invalid);
        }
        channels |= 1 << entry[1];
        let pointer = u16::from_le_bytes([entry[2], entry[3]]);
        let note_count = sequence::validate(bytes, pointer, entry[1], &mut mapped, budget)?;
        tracks.push(NesToseTrack {
            number: entry[1] + 1,
            note_count,
        });
    }
    if tracks.iter().all(|track| track.note_count == 0) {
        return Err(ReadError::Invalid);
    }
    mapped.sort_unstable();
    let mut merged: Vec<RomSpan> = Vec::new();
    for value in mapped {
        if let Some(previous) = merged.last_mut()
            && previous.effective_offset + previous.byte_len >= value.effective_offset
            && previous.canonical_cpu_address + value.effective_offset - previous.effective_offset
                == value.canonical_cpu_address
        {
            previous.byte_len = previous
                .byte_len
                .max(value.effective_offset + value.byte_len - previous.effective_offset);
        } else {
            merged.push(value);
        }
    }
    Ok(NesToseSong {
        profile: PROFILE,
        index: u16::from(index),
        title: format!("Music selector {index}"),
        table_entry,
        tracks,
        mapped_spans: merged,
        warnings: vec!["Qualified multi-channel music groups only; full soundtrack coverage and duration are not established.".into()],
    })
}

pub fn validate_song(
    bytes: &[u8],
    expected: &NesToseSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    if expected.profile == fcg::PROFILE {
        return fcg::validate_song(bytes, expected, cancel);
    }
    if closed::profile(expected.profile).is_some() {
        return closed::validate_song(bytes, expected, cancel);
    }
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let recognized = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("NES TOSE validation stopped: {stop:?}"))?;
    let valid = recognized
        && GROUPS.iter().any(|&(index, count)| {
            u16::from(index) == expected.index
                && song(bytes, index, count, &mut budget).ok().as_ref() == Some(expected)
        });
    anyhow::ensure!(
        valid,
        "NES TOSE selection differs from its recognized source"
    );
    Ok(())
}

fn span(bank: u8, address: u16, len: usize) -> RomSpan {
    RomSpan {
        effective_offset: 16 + u32::from(bank) * 0x8000 + u32::from(address - 0x8000),
        byte_len: len as u32,
        canonical_cpu_address: u32::from(address),
    }
}
