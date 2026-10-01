use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};

use crate::{Budget, RomSpan, ScanStop};

use super::{
    PreparedWsTose, ReadError, WsToseHardware, WsToseSong, WsToseTrack, profiles::Profile,
};

mod native;
#[cfg(any(test, feature = "test-support"))]
pub(super) mod tests;

pub(super) use native::prepare_rom;

#[derive(Clone, Copy)]
pub(super) struct VolumeProfile {
    driver: Profile,
    table: u16,
    row_count: u16,
    callers: [(u16, u16); 2],
    control_start: u16,
    control_end: u16,
    volume_setup: u16,
    color_setup: u16,
    interrupt: u16,
}

const PROFILE: VolumeProfile = VolumeProfile {
    driver: Profile {
        name: "ws-tose-volume-v1",
        len: 0x400000,
        fixed: 0x3e0000,
        end: 0x3e1064,
        hash: "999da26cc41d8db67933cc83ddba4e20a14e6cc398eb04169c0ee794d8653a46",
        segment: 0xe000,
        init: 0x22,
        selector: 0x96,
        tick: 0x1e6,
        status: 0,
        slots: 0x2000,
        wave: 0xaef,
        envelope: 0xbcf,
        frequency: 0x7ed,
        counts: &[],
    },
    table: 0xc80,
    row_count: 166,
    callers: [(0x15ae, 50), (0x1984, 17)],
    control_start: 0x150b,
    control_end: 0x1ab5,
    volume_setup: 0x90,
    color_setup: 0x40,
    interrupt: 0x155a,
};

impl VolumeProfile {
    fn control_span(self, start: u16, len: u16) -> RomSpan {
        RomSpan {
            effective_offset: (self.driver.len - 0x10000 + usize::from(start)) as u32,
            byte_len: u32::from(len),
            canonical_cpu_address: 0xf0000 + u32::from(start),
        }
    }
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<VolumeProfile>, ScanStop> {
    budget.charge()?;
    let driver = PROFILE.driver;
    if bytes.len() == driver.len {
        for _ in bytes[driver.fixed..driver.end].chunks(256) {
            budget.charge()?;
        }
        if zeff_firmware::sha256_hex(&bytes[driver.fixed..driver.end]) == driver.hash {
            for _ in bytes.chunks(256) {
                budget.charge()?;
            }
            if zeff_firmware::sha256_hex(bytes)
                == "3311444f8ca5cd4fece4bab66564373bed069fa8b1a5f60ede3403e2c7814cf3"
            {
                return Ok(Some(PROFILE));
            }
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    if tests::recognized(bytes) {
        return Ok(Some(tests::PROFILE));
    }
    Ok(None)
}

fn selector(
    bytes: &[u8],
    profile: VolumeProfile,
    mut index: u16,
) -> Result<(u16, u16, u16), ReadError> {
    for (start, count) in profile.callers {
        if index >= count {
            index -= count;
            continue;
        }
        let caller = usize::from(start) + usize::from(index) * 11;
        let at = profile.driver.len - 0x10000 + caller;
        if bytes.get(at) != Some(&0xb8)
            || bytes.get(at + 3) != Some(&0x9a)
            || super::word(bytes, at + 6)? != profile.driver.segment
        {
            return Err(ReadError::Invalid);
        }
        let first = super::word(bytes, at + 1)?;
        let entry = super::word(bytes, at + 4)?;
        let offset = entry
            .checked_sub(profile.driver.selector)
            .ok_or(ReadError::Invalid)?;
        if offset > 15 || !offset.is_multiple_of(5) {
            return Err(ReadError::Invalid);
        }
        let count = 4 - offset / 5;
        if first
            .checked_add(count)
            .is_none_or(|end| end > profile.row_count)
        {
            return Err(ReadError::Invalid);
        }
        return Ok((first, count, entry));
    }
    Err(ReadError::Invalid)
}

pub(super) fn scan(
    bytes: &[u8],
    songs: &mut Vec<WsToseSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<bool, ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(false);
    };
    for index in 0..profile.callers.iter().map(|&(_, count)| count).sum() {
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
    Ok(true)
}

fn song(
    bytes: &[u8],
    profile: VolumeProfile,
    index: u16,
    budget: &mut Budget<'_>,
) -> Result<WsToseSong, ReadError> {
    budget.charge()?;
    let (first, count, _) = selector(bytes, profile, index)?;
    let driver = profile.driver;
    let mut reader = super::sequence::Reader::legacy(bytes, driver.fixed / 0x10000, driver, budget)
        .with_wrapped_sweep_index();
    reader.mapped.push(profile.control_span(
        profile.control_start,
        profile.control_end - profile.control_start,
    ));
    reader
        .mapped
        .push(profile.control_span(profile.volume_setup, 5));
    reader
        .mapped
        .push(profile.control_span(profile.color_setup, 4));
    let at = usize::from(profile.table) + usize::from(first) * 6;
    let mut tracks = Vec::new();
    let mut slots = 0_u8;
    for row in 0..usize::from(count) {
        let slot = reader.word(at + row * 6)?;
        let channel = reader.word(at + row * 6 + 2)?;
        let pointer = reader.word(at + row * 6 + 4)?;
        if !slot.is_multiple_of(0x2a)
            || slot / 0x2a >= 8
            || channel > 3
            || usize::from(pointer)
                < usize::from(profile.table) + usize::from(profile.row_count) * 6
        {
            return Err(ReadError::Invalid);
        }
        let slot = (slot / 0x2a) as u8;
        if slots & (1 << slot) != 0 {
            return Err(ReadError::Invalid);
        }
        slots |= 1 << slot;
        tracks.push(WsToseTrack {
            number: channel as u8 + 1,
            slot,
            note_count: reader.track(pointer, channel as u8)?,
        });
    }
    if tracks.iter().all(|track| track.note_count == 0) {
        return Err(ReadError::Invalid);
    }
    Ok(WsToseSong {
        profile: driver.name,
        hardware: WsToseHardware::Color,
        index,
        title: format!("Audio selector {index}"),
        table_entry: RomSpan {
            effective_offset: (driver.offset(profile.table) + usize::from(first) * 6) as u32,
            byte_len: u32::from(count) * 6,
            canonical_cpu_address: u32::from(driver.segment) * 16 + at as u32,
        },
        tracks,
        mapped_spans: super::merged(reader.mapped),
        warnings: vec!["Native driver selection; soundtrack membership is not established.".into()],
        wsr_exportable: false,
    })
}

fn checked_profile(
    bytes: &[u8],
    expected: &WsToseSong,
    cancel: &AtomicBool,
) -> Result<VolumeProfile> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("WonderSwan volume driver validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Unrecognized WonderSwan volume driver source"))?;
    ensure!(
        expected.profile == profile.driver.name
            && song(bytes, profile, expected.index, &mut budget)
                .ok()
                .as_ref()
                == Some(expected),
        "WonderSwan volume driver inventory differs from its recognized source"
    );
    Ok(profile)
}

pub(super) fn validate_song(bytes: &[u8], song: &WsToseSong, cancel: &AtomicBool) -> Result<()> {
    checked_profile(bytes, song, cancel).map(|_| ())
}
