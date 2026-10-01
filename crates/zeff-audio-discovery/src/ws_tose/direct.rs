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
pub(super) struct DirectProfile {
    driver: Profile,
    selectors: u16,
    selector_count: u16,
    rows: u16,
    row_count: u16,
    bank: usize,
}

const PROFILE: DirectProfile = DirectProfile {
    driver: Profile {
        name: "ws-tose-direct-v1",
        len: 0x200000,
        fixed: 0x156ee0,
        end: 0x158107,
        hash: "a169e78dcd3dc8786b14886e195bdd149beaa869607eab78c8c628b13a084f9d",
        segment: 0x5000,
        init: 0x7508,
        selector: 0x766f,
        tick: 0x7849,
        status: 0x75ee,
        slots: 0x1e21,
        wave: 0x6ef5,
        envelope: 0x7025,
        frequency: 0x70d7,
        counts: &[],
    },
    selectors: 0x8107,
    selector_count: 20,
    rows: 0x6200,
    row_count: 35,
    bank: 2,
};

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<DirectProfile>, ScanStop> {
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
                == "83c5f23f014d7b94f23169940c589b5a9749f4a4d3168114e615522c4446721e"
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

pub(super) fn scan(
    bytes: &[u8],
    songs: &mut Vec<WsToseSong>,
    budget: &mut Budget<'_>,
    remaining: usize,
) -> Result<bool, ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(false);
    };
    for index in 0..profile.selector_count {
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

fn selector(bytes: &[u8], profile: DirectProfile, index: u16) -> Result<(u16, u16), ReadError> {
    if index >= profile.selector_count {
        return Err(ReadError::Invalid);
    }
    let at = profile.driver.offset(profile.selectors) + usize::from(index) * 4;
    let first = super::word(bytes, at)?;
    let count = super::word(bytes, at + 2)?;
    if !(1..=8).contains(&count)
        || first
            .checked_add(count)
            .is_none_or(|end| end > profile.row_count)
    {
        return Err(ReadError::Invalid);
    }
    Ok((first, count))
}

fn song(
    bytes: &[u8],
    profile: DirectProfile,
    index: u16,
    budget: &mut Budget<'_>,
) -> Result<WsToseSong, ReadError> {
    budget.charge()?;
    let (first, count) = selector(bytes, profile, index)?;
    let table_entry = RomSpan {
        effective_offset: (profile.driver.offset(profile.selectors) + usize::from(index) * 4)
            as u32,
        byte_len: 4,
        canonical_cpu_address: u32::from(profile.driver.segment) * 16
            + u32::from(profile.selectors)
            + u32::from(index) * 4,
    };
    let mut reader = super::sequence::Reader::new(bytes, profile.bank, profile.driver, budget);
    reader.mapped.push(table_entry);
    let mut slots = 0_u8;
    let mut tracks = Vec::new();
    for row in first..first + count {
        let at = usize::from(profile.rows) + usize::from(row) * 6;
        let slot = reader.word(at)?;
        let channel = reader.word(at + 2)?;
        let pointer = reader.word(at + 4)?;
        if !slot.is_multiple_of(0x34)
            || slot / 0x34 >= 8
            || channel > 3
            || usize::from(pointer) < usize::from(profile.rows) + usize::from(profile.row_count) * 6
        {
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
            note_count: reader.track(pointer, channel as u8)?,
        });
    }
    if tracks.iter().all(|track| track.note_count == 0) {
        return Err(ReadError::Invalid);
    }
    Ok(WsToseSong {
        profile: profile.driver.name,
        hardware: WsToseHardware::Color,
        index,
        title: format!("Audio selector {index}"),
        table_entry,
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
) -> Result<DirectProfile> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("WonderSwan direct driver validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Unrecognized WonderSwan direct driver source"))?;
    ensure!(
        expected.profile == profile.driver.name
            && song(bytes, profile, expected.index, &mut budget)
                .ok()
                .as_ref()
                == Some(expected),
        "WonderSwan direct driver inventory differs from its recognized source"
    );
    Ok(profile)
}

pub(super) fn validate_song(bytes: &[u8], song: &WsToseSong, cancel: &AtomicBool) -> Result<()> {
    checked_profile(bytes, song, cancel).map(|_| ())
}
