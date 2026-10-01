use std::sync::atomic::AtomicBool;

use anyhow::{Result, ensure};

use crate::{Budget, RomSpan, ScanStop};

use super::{
    PreparedWsTose, ReadError, WsToseHardware, WsToseSong, WsToseTrack, profiles::Profile,
};

mod native;
#[cfg(any(test, feature = "test-support"))]
pub(super) mod relocated_tests;
#[cfg(any(test, feature = "test-support"))]
pub(super) mod tests;

pub(super) use native::prepare_rom;

#[derive(Clone, Copy)]
pub(super) struct ScaledProfile {
    driver: Profile,
    source_hash: &'static str,
    rom_group: u8,
    selectors: Selectors,
    rows: u16,
    row_count: u16,
}

#[derive(Clone, Copy)]
enum Selectors {
    Pairs { offset: u16, count: u16 },
    Callers(&'static [usize]),
}

impl Selectors {
    fn count(self) -> u16 {
        match self {
            Self::Pairs { count, .. } => count,
            Self::Callers(callers) => callers.len() as u16,
        }
    }
}

const PROFILE: ScaledProfile = ScaledProfile {
    driver: Profile {
        name: "ws-tose-scaled-v1",
        len: 0x400000,
        fixed: 0x3f9144,
        end: 0x3fa37e,
        hash: "f108ec22c29f8030cb2234eaee4180abce9d00e28edafd9214fc263dc8791329",
        segment: 0xf000,
        init: 0x9152,
        selector: 0x9211,
        tick: 0x9377,
        status: 0,
        slots: 0xc00,
        wave: 0x9eb7,
        envelope: 0x9f87,
        frequency: 0x9a64,
        counts: &[],
    },
    source_hash: "3ec68e02fa964383d6a0792aca7e53ab005e17a768a8546eb6dc5ed482db7c29",
    rom_group: 0x23,
    selectors: Selectors::Pairs {
        offset: 0x4d47,
        count: 52,
    },
    rows: 0xa108,
    row_count: 105,
};

const RELOCATED: ScaledProfile = ScaledProfile {
    driver: Profile {
        name: "ws-tose-scaled-v2",
        len: 0x100000,
        fixed: 0xe41f4,
        end: 0xe53e6,
        hash: "e5cc9ddd285e911cce16c22211550b693478a55c5ec559ee346d56db6d99181a",
        segment: 0xe000,
        init: 0x4202,
        selector: 0x42bc,
        tick: 0x441a,
        status: 0,
        slots: 0xc00,
        wave: 0x4e7a,
        envelope: 0x4f5a,
        frequency: 0x4ac8,
        counts: &[],
    },
    source_hash: "bdd61edcbde0678ca58caffbba9b001bee52bb2edc398254dbf17e64a8fc8d05",
    rom_group: 0x20,
    selectors: Selectors::Callers(&[
        0x86f68, 0x40f8b, 0x80d7e, 0x40422, 0x404f9, 0x40522, 0xc4753, 0xc73d6, 0x81b74, 0xc05b1,
        0x85271, 0x86ea6, 0xc0252, 0xc220f, 0xc05cb, 0xc036e, 0x8227f, 0x815e0, 0x828a8, 0x86e4d,
        0xc6cff, 0xc1a80, 0xc0969, 0x8235e, 0x424bb, 0x41245, 0xc7472, 0x8940a, 0xc019c, 0xc0a68,
        0x824a9,
    ]),
    rows: 0x50bc,
    row_count: 135,
};

fn recognized(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Option<ScaledProfile>, ScanStop> {
    for profile in [PROFILE, RELOCATED] {
        budget.charge()?;
        let driver = profile.driver;
        if bytes.len() != driver.len {
            continue;
        }
        for _ in bytes[driver.fixed..driver.end].chunks(256) {
            budget.charge()?;
        }
        if zeff_firmware::sha256_hex(&bytes[driver.fixed..driver.end]) == driver.hash {
            for _ in bytes.chunks(256) {
                budget.charge()?;
            }
            if zeff_firmware::sha256_hex(bytes) == profile.source_hash {
                return Ok(Some(profile));
            }
        }
    }
    #[cfg(any(test, feature = "test-support"))]
    if tests::recognized(bytes) {
        return Ok(Some(tests::PROFILE));
    }
    #[cfg(any(test, feature = "test-support"))]
    if relocated_tests::recognized(bytes) {
        return Ok(Some(relocated_tests::PROFILE));
    }
    Ok(None)
}

fn selector(
    bytes: &[u8],
    profile: ScaledProfile,
    index: u16,
) -> Result<(u16, u16, RomSpan), ReadError> {
    if index >= profile.selectors.count() {
        return Err(ReadError::Invalid);
    }
    let (first, count, table_entry) = match profile.selectors {
        Selectors::Pairs { offset, .. } => {
            let at = profile.driver.offset(offset) + usize::from(index) * 2;
            let entry = super::word(bytes, at)?;
            (
                entry & 255,
                entry >> 8,
                RomSpan {
                    effective_offset: at as u32,
                    byte_len: 2,
                    canonical_cpu_address: u32::from(profile.driver.segment) * 16
                        + u32::from(offset)
                        + u32::from(index) * 2,
                },
            )
        }
        Selectors::Callers(callers) => {
            let at = callers[usize::from(index)];
            if bytes.get(at) != Some(&0xb8)
                || bytes.get(at + 3) != Some(&0x9a)
                || super::word(bytes, at + 6)? != profile.driver.segment
            {
                return Err(ReadError::Invalid);
            }
            let delta = super::word(bytes, at + 4)?
                .checked_sub(profile.driver.selector)
                .ok_or(ReadError::Invalid)?;
            if delta > 18 || !delta.is_multiple_of(6) {
                return Err(ReadError::Invalid);
            }
            (
                super::word(bytes, at + 1)?,
                4 - delta / 6,
                RomSpan {
                    effective_offset: at as u32,
                    byte_len: 8,
                    canonical_cpu_address: at as u32,
                },
            )
        }
    };
    if !(1..=4).contains(&count)
        || first
            .checked_add(count)
            .is_none_or(|end| end > profile.row_count)
    {
        return Err(ReadError::Invalid);
    }
    Ok((first, count, table_entry))
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
    for index in 0..profile.selectors.count() {
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
    profile: ScaledProfile,
    index: u16,
    budget: &mut Budget<'_>,
) -> Result<WsToseSong, ReadError> {
    budget.charge()?;
    let (first, count, table_entry) = selector(bytes, profile, index)?;
    let driver = profile.driver;
    let mut reader = super::sequence::Reader::scaled(bytes, driver.fixed / 0x10000, driver, budget);
    reader.mapped.push(table_entry);
    let mut tracks = Vec::new();
    let mut slots = 0_u8;
    for row in first..first + count {
        let at = usize::from(profile.rows) + usize::from(row) * 6;
        let slot = reader.word(at)?;
        let channel = reader.word(at + 2)?;
        let pointer = reader.word(at + 4)?;
        if !slot.is_multiple_of(0x2e)
            || slot / 0x2e >= 8
            || channel > 3
            || usize::from(pointer) < usize::from(profile.rows) + usize::from(profile.row_count) * 6
        {
            return Err(ReadError::Invalid);
        }
        let slot = (slot / 0x2e) as u8;
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
        hardware: WsToseHardware::Mono,
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
) -> Result<ScaledProfile> {
    let mut budget = Budget {
        cancel,
        remaining: 4_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("WonderSwan scaled driver validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("Unrecognized WonderSwan scaled driver source"))?;
    ensure!(
        expected.profile == profile.driver.name
            && song(bytes, profile, expected.index, &mut budget)
                .ok()
                .as_ref()
                == Some(expected),
        "WonderSwan scaled driver inventory differs from its recognized source"
    );
    Ok(profile)
}

pub(super) fn validate_song(bytes: &[u8], song: &WsToseSong, cancel: &AtomicBool) -> Result<()> {
    checked_profile(bytes, song, cancel).map(|_| ())
}
