use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};

use crate::{Budget, ScanStop};

use super::{
    PreparedWsTose, ReadError, WsToseHardware, WsToseSong, WsToseTrack, profiles::Profile,
};

#[path = "legacy_profiles.rs"]
mod profiles;

#[derive(Clone, Copy)]
pub(super) struct FixedProfile {
    pub driver: Profile,
    pub table: u16,
    pub count: u16,
    pub single: u16,
    pub hardware: WsToseHardware,
    pub initial_word: Option<InitialWord>,
}

#[derive(Clone, Copy)]
pub(super) struct InitialWord {
    pub address: u16,
    pub value: u16,
    pub caller: usize,
}

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<FixedProfile>, ScanStop> {
    for &profile in profiles::PROFILES
        .iter()
        .flat_map(|profiles| profiles.iter())
    {
        budget.charge()?;
        let driver = profile.driver;
        if bytes.len() != driver.len {
            continue;
        }
        for _ in 0..(driver.end - driver.fixed).div_ceil(256) {
            budget.charge()?;
        }
        if zeff_firmware::sha256_hex(&bytes[driver.fixed..driver.end]) == driver.hash
            && profile.initial_word.is_none_or(|word| {
                let [lo, hi] = word.value.to_le_bytes();
                let [address_lo, address_hi] = word.address.to_le_bytes();
                bytes.get(word.caller..word.caller + 6)
                    == Some(&[0xb8, lo, hi, 0xa3, address_lo, address_hi])
            })
        {
            return Ok(Some(profile));
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    if super::legacy_tests::recognized(bytes) {
        return Ok(Some(super::legacy_tests::PROFILE));
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
    let mut index = 0;
    while index < profile.count {
        let count = track_count(bytes, profile, index);
        match song(bytes, profile, index, count, budget) {
            Ok(song) => {
                if songs.len() >= remaining {
                    return Err(ScanStop::CandidateLimit);
                }
                songs.push(song);
            }
            Err(ReadError::Invalid) => (),
            Err(ReadError::Stop(stop)) => return Err(stop),
        }
        index += count;
    }
    Ok(true)
}

fn track_count(bytes: &[u8], profile: FixedProfile, index: u16) -> u16 {
    if index + 4 > profile.count {
        return 1;
    }
    let at = profile.driver.offset(profile.table) + usize::from(index) * 6;
    let first = super::word(bytes, at).unwrap_or(u16::MAX);
    let mut channels = 0_u8;
    if [0, 4 * 0x2a].contains(&first)
        && (0..4).all(|i| {
            let Some(channel) = super::word(bytes, at + i * 6 + 2)
                .ok()
                .filter(|&value| value < 4)
            else {
                return false;
            };
            channels |= 1 << channel;
            super::word(bytes, at + i * 6).ok() == Some(first + i as u16 * 0x2a)
        })
        && channels == 15
    {
        4
    } else {
        1
    }
}

fn song(
    bytes: &[u8],
    profile: FixedProfile,
    index: u16,
    count: u16,
    budget: &mut Budget<'_>,
) -> Result<WsToseSong, ReadError> {
    let driver = profile.driver;
    let bank = driver.fixed / 0x10000;
    let mut reader = super::sequence::Reader::legacy(bytes, bank, driver, budget);
    if let Some(word) = profile.initial_word {
        reader.mapped.push(crate::RomSpan {
            effective_offset: word.caller as u32,
            byte_len: 6,
            canonical_cpu_address: 0x40000 + (word.caller & 0xffff) as u32,
        });
    }
    let at = usize::from(profile.table) + usize::from(index) * 6;
    let mut tracks = Vec::new();
    for i in 0..usize::from(count) {
        let row = at + i * 6;
        let slot = reader.word(row)?;
        let channel = reader.word(row + 2)?;
        let pointer = reader.word(row + 4)?;
        if !slot.is_multiple_of(0x2a)
            || slot / 0x2a >= 8
            || channel > 3
            || usize::from(pointer) < usize::from(profile.table) + usize::from(profile.count) * 6
        {
            return Err(ReadError::Invalid);
        }
        tracks.push(WsToseTrack {
            number: channel as u8 + 1,
            slot: (slot / 0x2a) as u8,
            note_count: reader.track(pointer, channel as u8)?,
        });
    }
    if tracks.iter().all(|track| track.note_count == 0) {
        return Err(ReadError::Invalid);
    }
    let mut table_entry = super::span(bank, at, usize::from(count) * 6);
    table_entry.canonical_cpu_address = u32::from(driver.segment) * 16 + at as u32;
    let mut warnings = vec![
        "Native driver selection; soundtrack membership and duration are not established.".into(),
    ];
    if profile.hardware == WsToseHardware::Color && bytes[bytes.len() - 9] == 0 {
        warnings.push("The original program requires WonderSwan Color despite its cartridge footer; isolated playback uses Color hardware.".into());
    }
    Ok(WsToseSong {
        profile: driver.name,
        hardware: profile.hardware,
        index,
        title: format!("Audio selector {index}"),
        table_entry,
        tracks,
        mapped_spans: super::merged(reader.mapped),
        warnings,
    })
}

fn checked_profile(
    bytes: &[u8],
    expected: &WsToseSong,
    cancel: &AtomicBool,
) -> Result<FixedProfile> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("WonderSwan fixed driver validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Unrecognized WonderSwan fixed driver source"))?;
    let mut index = 0;
    while index < expected.index && index < profile.count {
        index += track_count(bytes, profile, index);
    }
    ensure!(
        index == expected.index
            && index < profile.count
            && song(
                bytes,
                profile,
                index,
                track_count(bytes, profile, index),
                &mut budget
            )
            .ok()
            .as_ref()
                == Some(expected),
        "WonderSwan fixed driver inventory differs from its recognized source"
    );
    Ok(profile)
}

pub(super) fn validate_song(bytes: &[u8], song: &WsToseSong, cancel: &AtomicBool) -> Result<()> {
    checked_profile(bytes, song, cancel).map(|_| ())
}

pub(super) fn prepare_rom(
    bytes: &[u8],
    song: &WsToseSong,
    cancel: &AtomicBool,
) -> Result<PreparedWsTose> {
    let profile = checked_profile(bytes, song, cancel)?;
    let driver = profile.driver;
    let bootstrap = (0..127)
        .map(|index| bytes.len() - 0x10000 + ((index + 112) % 127) * 512)
        .find(|&at| {
            song.mapped_spans.iter().all(|span| {
                let start = span.effective_offset as usize;
                let end = start + span.byte_len as usize;
                end <= at || start >= at + 512
            })
        })
        .ok_or_else(|| anyhow::anyhow!("WonderSwan fixed driver has no bootstrap storage"))?;
    ensure!(
        (u32::from(driver.slots) + 8 * 0x2a + 64 < 0x3b00 || driver.slots >= 0x3f00)
            && (u32::from(driver.slots) + 8 * 0x2a + 64 <= 0x4000
                || profile.hardware == WsToseHardware::Color),
        "WonderSwan fixed driver overlaps bootstrap storage"
    );
    // The driver may switch the linear ROM window containing the reset vector.
    let bank = driver.fixed / 0x10000;
    let mut code = vec![
        0xb0,
        (bank >> 4) as u8,
        0xe6,
        0xc0,
        0xb0,
        bank as u8,
        0xe6,
        0xc3,
        0xe6,
        0xc2,
    ];
    if let Some(word) = profile.initial_word {
        code.extend([0xc7, 0x06]);
        code.extend(word.address.to_le_bytes());
        code.extend(word.value.to_le_bytes());
    }
    far_call(&mut code, driver.init, driver.segment);
    code.extend([0xc6, 0x06, 0x00, 0x3e, 1]);
    let wait_start = 0x3c00 + code.len() as u32;
    code.extend([0x80, 0x3e, 0x01, 0x3e, 1, 0x75, 0xf9, 0xb8]);
    code.extend(song.index.to_le_bytes());
    let selector = if song.tracks.len() == 4 {
        driver.selector
    } else {
        profile.single
    };
    far_call(&mut code, selector, driver.segment);
    code.extend([0xc7, 0x06, 0x38, 0]);
    let vector = code.len();
    code.extend([0, 0, 0xc7, 0x06, 0x3a, 0, 0, 0]);
    code.extend([
        0xb0, 8, 0xe6, 0xb0, 0xb0, 0x40, 0xe6, 0xb6, 0xe6, 0xb2, 0xfb,
    ]);
    code.extend([0xf4, 0xeb, 0xfd]);
    let interrupt = 0x3c00 + code.len() as u16;
    code.extend([
        0x60, 0x1e, 0x06, 0x33, 0xc0, 0x8e, 0xd8, 0x8e, 0xc0, 0xb0, 0x40, 0xe6, 0xb6,
    ]);
    far_call(&mut code, driver.tick, driver.segment);
    code.extend([0x07, 0x1f, 0x61, 0xcf]);
    code[vector..vector + 2].copy_from_slice(&interrupt.to_le_bytes());
    ensure!(
        code.len() < 256,
        "WonderSwan fixed driver bootstrap is too large"
    );
    let mut stage = vec![
        0xfa, 0xfc, 0x33, 0xc0, 0x8e, 0xd0, 0xbc, 0xf0, 0x3b, 0x8e, 0xd8, 0x8e, 0xc0, 0x33, 0xff,
        0xb9, 0, 0x20, 0xf3, 0xab, 0xe6, 0xb2, 0x0e, 0x1f, 0xbe,
    ];
    stage.extend(((bootstrap & 0xffff) as u16 + 128).to_le_bytes());
    stage.extend([0xbf, 0, 0x3c, 0xb9]);
    stage.extend((code.len() as u16).to_le_bytes());
    stage.extend([0xf3, 0xa4, 0x33, 0xc0, 0x8e, 0xd8, 0xea, 0, 0x3c, 0, 0]);
    let mut result = bytes.to_vec();
    result[bootstrap..bootstrap + stage.len()].copy_from_slice(&stage);
    result[bootstrap + 128..bootstrap + 128 + code.len()].copy_from_slice(&code);
    let reset = result.len() - 16;
    result[reset..reset + 5].copy_from_slice(&[
        0xea,
        bootstrap as u8,
        (bootstrap >> 8) as u8,
        0,
        0xf0,
    ]);
    let footer_model = result.len() - 9;
    result[footer_model] = u8::from(song.hardware == WsToseHardware::Color);
    let checksum = result[..result.len() - 2]
        .iter()
        .fold(0_u16, |sum, &byte| sum.wrapping_add(u16::from(byte)));
    let end = result.len();
    result[end - 2..].copy_from_slice(&checksum.to_le_bytes());
    Ok(PreparedWsTose {
        bytes: result,
        bootstrap: super::WsToseBootstrap::Ram,
        hardware: song.hardware,
        ready_address: 0x3e00,
        ack_address: 0x3e01,
        wait_start,
        wait_end: wait_start + 7,
    })
}

fn far_call(code: &mut Vec<u8>, offset: u16, segment: u16) {
    code.push(0x9a);
    code.extend(offset.to_le_bytes());
    code.extend(segment.to_le_bytes());
}
