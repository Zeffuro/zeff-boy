use std::sync::atomic::AtomicBool;

use crate::{Budget, RomSpan, ScanStop};

use super::{NesToseSong, NesToseTrack, ReadError};

pub(super) const PROFILE: &str = "nes-tose-eight-slot-fcg-v1";
pub(super) const WITNESSES: [(u16, u16, &str); 2] = [
    (
        0x8007,
        0x854f,
        "01042c9ed13527996f512ea08f461b90f1d73eb740f2b0c69d1160b475be57b6",
    ),
    (
        0xca6a,
        0xca82,
        "02763432f195ed5c8b649b45d7d94d0528beb2a5d496b30add1ff787a53697bb",
    ),
];

pub(super) fn scan(
    bytes: &[u8],
    songs: &mut Vec<NesToseSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<(), ScanStop> {
    if !recognized(bytes, budget)? {
        return Ok(());
    }
    for index in 0..=225 {
        match song(bytes, index, budget) {
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
    if bytes.len() != 0x80010 || bytes.get(..16) != Some(b"NES\x1a\x10\x20\0\x10\0\0\0\0\0\0\0\0") {
        return Ok(false);
    }
    for &(start, end, hash) in &WITNESSES {
        let value = span(start, usize::from(end - start));
        for _ in 0..value.byte_len.div_ceil(256) {
            budget.charge()?;
        }
        let offset = value.effective_offset as usize;
        let actual = zeff_firmware::sha256_hex(&bytes[offset..offset + value.byte_len as usize]);
        if actual != hash {
            #[cfg(any(test, feature = "test-support"))]
            if super::tests::fcg_fixture_witness(start, &actual) {
                continue;
            }
            return Ok(false);
        }
    }
    Ok(true)
}

fn song(bytes: &[u8], index: u8, budget: &mut Budget<'_>) -> Result<NesToseSong, ReadError> {
    budget.charge()?;
    let table_entry = span(0x854e + u16::from(index) * 4, 16);
    let at = table_entry.effective_offset as usize;
    let entries = &bytes[at..at + 16];
    if !(0..4).all(|c| entries[c * 4] == 84 + c as u8 * 21 && entries[c * 4 + 1] == c as u8) {
        return Err(ReadError::Invalid);
    }
    let mut mapped = WITNESSES
        .iter()
        .map(|&(start, end, _)| span(start, usize::from(end - start)))
        .collect::<Vec<_>>();
    mapped.push(table_entry);
    let mut tracks = Vec::new();
    for (channel, entry) in entries.as_chunks::<4>().0.iter().enumerate() {
        let row = table_entry.canonical_cpu_address as u16 + channel as u16 * 4;
        for column in 1..4 {
            let target = row + column;
            if target & 0xff00 != row & 0xff00 {
                mapped.push(span((row & 0xff00) | (target & 255), 1));
            }
        }
        let pointer = u16::from_le_bytes([entry[2], entry[3]]);
        let note_count =
            super::fcg_sequence::validate(bytes, pointer, channel as u8, &mut mapped, budget)?;
        tracks.push(NesToseTrack {
            number: channel as u8 + 1,
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
        warnings: vec!["Qualified four-channel music groups only; full soundtrack coverage and duration are not established.".into()],
    })
}

pub(super) fn validate_song(
    bytes: &[u8],
    expected: &NesToseSong,
    cancel: &AtomicBool,
) -> anyhow::Result<()> {
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let recognized = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("NES TOSE validation stopped: {stop:?}"))?;
    anyhow::ensure!(
        recognized
            && expected.index <= 225
            && song(bytes, expected.index as u8, &mut budget).ok().as_ref() == Some(expected),
        "NES TOSE selection differs from its recognized source"
    );
    Ok(())
}

pub(super) fn span(address: u16, len: usize) -> RomSpan {
    let offset = if address >= 0xc000 {
        0x3c010 + u32::from(address - 0xc000)
    } else {
        16 + u32::from(address - 0x8000)
    };
    RomSpan {
        effective_offset: offset,
        byte_len: len as u32,
        canonical_cpu_address: u32::from(address),
    }
}
