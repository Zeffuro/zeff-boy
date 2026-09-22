use std::{
    collections::{BTreeSet, HashMap},
    sync::atomic::{AtomicBool, Ordering},
};

use anyhow::{Result, ensure};

use super::{
    GbNativeChannel, GbNativeProfile, GbNativeSong, GbNativeTiming, PreparedGbNative, rom_span,
};
use crate::{Budget, MediaIdentity, RomSpan, ScanStop, SourceSpan};

mod bootstrap;
#[cfg(any(test, feature = "test-support"))]
mod fixture;
#[cfg(test)]
mod tests;

#[cfg(any(test, feature = "test-support"))]
pub use fixture::{fixture_rom, fixture_rom_small};

const DRIVER_BANK: u8 = 0x39;
const LAST_AUDIO_BANK: u8 = 0x3e;
const TABLE_ROWS: u16 = 223;
const TABLE_BYTES: usize = TABLE_ROWS as usize * 3;
const PLAYBACK_SECONDS: u64 = 600;
const CGB_DOUBLE_CLOCKS: u64 = 8_388_608;

const AGES_SELECTORS: &[u8] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50,
    51, 52, 53, 54, 56, 57, 60, 62, 63, 64, 70, 74,
];
const SEASONS_SELECTORS: &[u8] = &[
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26,
    27, 28, 29, 30, 31, 32, 33, 34, 35, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 49, 50, 51, 52,
    53, 54, 56, 57, 60, 61, 62, 63, 64, 70, 74,
];
const AGES_EMPTY_EFFECTS: &[u8] = &[
    0x7a, 0x86, 0x92, 0x93, 0x94, 0x97, 0xb7, 0xbd, 0xca, 0xcf, 0xd5,
];
const SEASONS_EMPTY_EFFECTS: &[u8] = &[0x97, 0xa1, 0xad, 0xb6, 0xd4, 0xd5];

pub(super) struct Profile {
    id: &'static str,
    source: &'static str,
    audio_hash: &'static str,
    byte_len: usize,
    table: u16,
    queue: u16,
    init: u16,
    timer_start: u16,
    wrapper_end: u16,
    music_selectors: &'static [u8],
    effect_range: Option<(u8, u8)>,
    empty_effects: &'static [u8],
}

const PROFILES: &[Profile] = &[
    Profile {
        id: "gb-native-cgb-timer-banked-01",
        source: "dbbb897a003654abfef7eadbfd401421e247e0c37319e947e5c80099f8f7ab86",
        audio_hash: "9f393e18758fcd5fc86bf92e3c0fd2a218f832116446d60d86deb4a37a51c191",
        byte_len: 0x20_0000,
        table: 0x5748,
        queue: 0x0cba,
        init: 0x0cd9,
        timer_start: 0x0d08,
        wrapper_end: 0x0d80,
        music_selectors: AGES_SELECTORS,
        effect_range: Some((0x4c, 0xd5)),
        empty_effects: AGES_EMPTY_EFFECTS,
    },
    Profile {
        id: "gb-native-cgb-timer-banked-02",
        source: "35fd809a21df04c7dac16cf76f7cc5d04017e3beabca102d2e378912a48539d9",
        audio_hash: "9c313a23006d3cfe03f1c38fa648842bb3d9a7a1976fc2df0534669c4b20911c",
        byte_len: 0x20_0000,
        table: 0x57cf,
        queue: 0x0c96,
        init: 0x0cb5,
        timer_start: 0x0ce4,
        wrapper_end: 0x0d5c,
        music_selectors: SEASONS_SELECTORS,
        effect_range: Some((0x4c, 0xd5)),
        empty_effects: SEASONS_EMPTY_EFFECTS,
    },
    Profile {
        id: "gb-native-cgb-timer-banked-03",
        source: "0b56b78a9e45452e98c33edd111234931f1e034dc097f6f23082eb8db6055474",
        audio_hash: "9f393e18758fcd5fc86bf92e3c0fd2a218f832116446d60d86deb4a37a51c191",
        byte_len: 0x10_0000,
        table: 0x5748,
        queue: 0x0c98,
        init: 0x0cb7,
        timer_start: 0x0ce6,
        wrapper_end: 0x0d5e,
        music_selectors: AGES_SELECTORS,
        effect_range: Some((0x4c, 0xd5)),
        empty_effects: AGES_EMPTY_EFFECTS,
    },
    Profile {
        id: "gb-native-cgb-timer-banked-04",
        source: "862a51368fb30539279d336b3fe193b43876d2cb15c87a36f5da517804ab3971",
        audio_hash: "9c313a23006d3cfe03f1c38fa648842bb3d9a7a1976fc2df0534669c4b20911c",
        byte_len: 0x10_0000,
        table: 0x57cf,
        queue: 0x0c74,
        init: 0x0c93,
        timer_start: 0x0cc2,
        wrapper_end: 0x0d3a,
        music_selectors: SEASONS_SELECTORS,
        effect_range: Some((0x4c, 0xd5)),
        empty_effects: SEASONS_EMPTY_EFFECTS,
    },
];

impl Profile {
    fn is_effect(&self, raw: u8) -> bool {
        self.effect_range
            .is_some_and(|range| (range.0..=range.1).contains(&raw))
            && !self.empty_effects.contains(&raw)
    }

    fn index(&self, raw: u8) -> Option<usize> {
        self.music_selectors
            .iter()
            .position(|&candidate| candidate == raw)
            .or_else(|| {
                self.is_effect(raw).then_some(
                    self.music_selectors.len()
                        + (0x4c..raw)
                            .filter(|candidate| self.is_effect(*candidate))
                            .count(),
                )
            })
    }
}

pub(super) fn candidate(bytes: &[u8]) -> bool {
    matches!(bytes.len(), 0x10_0000 | 0x20_0000)
        && bytes.get(0x143) == Some(&0xc0)
        && matches!(bytes.get(0x147..0x14a), Some([0x1b, 5, 2] | [0x1b, 6, 2]))
}

pub(super) fn owns(profile: &str) -> bool {
    if PROFILES.iter().any(|candidate| candidate.id == profile) {
        return true;
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        profile == fixture::PROFILE_ID
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        false
    }
}

fn recognized(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> std::result::Result<Option<&'static Profile>, ScanStop> {
    budget.charge()?;
    if !candidate(bytes) {
        return Ok(None);
    }
    for _ in bytes.chunks(256) {
        budget.charge()?;
    }
    let source = zeff_firmware::sha256_hex(bytes);
    let Some(profile) = PROFILES.iter().find(|profile| profile.source == source) else {
        #[cfg(any(test, feature = "test-support"))]
        if fixture::matches(bytes) {
            return Ok(Some(&fixture::PROFILE));
        }
        return Ok(None);
    };
    let audio_start = usize::from(DRIVER_BANK) * 0x4000;
    let audio_end = usize::from(LAST_AUDIO_BANK + 1) * 0x4000;
    if profile.byte_len != bytes.len()
        || zeff_firmware::sha256_hex(&bytes[audio_start..audio_end]) != profile.audio_hash
    {
        return Ok(None);
    }
    Ok(Some(profile))
}

pub(super) fn scan(
    bytes: &[u8],
    songs: &mut Vec<GbNativeSong>,
    budget: &mut Budget<'_>,
    max_candidates: usize,
) -> std::result::Result<(), ScanStop> {
    let Some(profile) = recognized(bytes, budget)? else {
        return Ok(());
    };
    for (index, &raw) in profile.music_selectors.iter().enumerate() {
        budget.charge()?;
        if songs.len() >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        let song = inspect(bytes, profile, index, raw, budget)?;
        songs.push(song);
    }
    for raw in 0x4c..=0xd5 {
        if !profile.is_effect(raw) {
            continue;
        }
        budget.charge()?;
        if songs.len() >= max_candidates {
            return Err(ScanStop::CandidateLimit);
        }
        let song = inspect(
            bytes,
            profile,
            profile.index(raw).ok_or(ScanStop::ValidationLimit)?,
            raw,
            budget,
        )?;
        songs.push(song);
    }
    Ok(())
}

pub(super) fn prepare(
    bytes: &[u8],
    song: &GbNativeSong,
    cancel: &AtomicBool,
) -> Result<PreparedGbNative> {
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let profile = recognized(bytes, &mut budget)
        .map_err(|stop| anyhow::anyhow!("GB timer-banked validation stopped: {stop:?}"))?
        .ok_or_else(|| anyhow::anyhow!("GB source no longer matches its timer-banked profile"))?;
    let index = profile
        .index(song.raw_index)
        .ok_or_else(|| anyhow::anyhow!("GB native selector is not admitted by its profile"))?;
    let checked = inspect(bytes, profile, index, song.raw_index, &mut budget).map_err(|stop| {
        anyhow::anyhow!("GB timer-banked selection validation stopped: {stop:?}")
    })?;
    ensure!(
        song.profile == profile.id && checked == *song,
        "GB native selection no longer matches its source"
    );
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "GB native preparation cancelled"
    );
    bootstrap::build(bytes, song, profile)
}

fn inspect(
    bytes: &[u8],
    profile: &Profile,
    index: usize,
    raw: u8,
    budget: &mut Budget<'_>,
) -> std::result::Result<GbNativeSong, ScanStop> {
    let is_effect = profile.is_effect(raw);
    if profile.index(raw) != Some(index) || raw >= TABLE_ROWS as u8 {
        return Err(ScanStop::ValidationLimit);
    }
    let table_address = profile
        .table
        .checked_add(u16::from(raw) * 3)
        .ok_or(ScanStop::ValidationLimit)?;
    let table_entry =
        rom_span(bytes, DRIVER_BANK, table_address, 3).ok_or(ScanStop::ValidationLimit)?;
    let row = &bytes[table_entry.effective_offset as usize..][..3];
    let bank = DRIVER_BANK
        .checked_add(row[0])
        .filter(|bank| *bank <= LAST_AUDIO_BANK)
        .ok_or(ScanStop::ValidationLimit)?;
    let header_address = u16::from_le_bytes([row[1], row[2]]);
    let (header, entries) = parse_header(bytes, header_address, is_effect, budget)?;
    let mut channels = Vec::with_capacity(entries.len());
    let mut stream_spans = Vec::new();
    for (slot, (encoded, pointer)) in entries.into_iter().enumerate() {
        budget.charge()?;
        let expected = if is_effect { encoded & 0x0f } else { encoded };
        let entry = rom_span(bytes, DRIVER_BANK, header_address + (slot * 3) as u16, 3)
            .ok_or(ScanStop::ValidationLimit)?;
        let spans = parse_stream(bytes, bank, expected, pointer, budget)?;
        let sequence = *spans.first().ok_or(ScanStop::ValidationLimit)?;
        channels.push(GbNativeChannel {
            number: expected + 1,
            entry,
            sequence,
        });
        stream_spans.extend(spans);
    }
    let native = GbNativeProfile {
        cartridge_type: 0x1b,
        timing: GbNativeTiming::CgbDouble,
        init: rom_span(bytes, DRIVER_BANK, 0x4000, 3).ok_or(ScanStop::ValidationLimit)?,
        tick: rom_span(bytes, DRIVER_BANK, 0x4003, 3).ok_or(ScanStop::ValidationLimit)?,
        driver: rom_span(
            bytes,
            0,
            profile.queue,
            usize::from(profile.wrapper_end - profile.queue),
        )
        .ok_or(ScanStop::ValidationLimit)?,
        tables: rom_span(bytes, DRIVER_BANK, profile.table, TABLE_BYTES)
            .ok_or(ScanStop::ValidationLimit)?,
        bootstrap: rom_span(bytes, 0, 0x150, 0x100).ok_or(ScanStop::ValidationLimit)?,
        startup_hook: rom_span(bytes, 0, 0x100, 3).ok_or(ScanStop::ValidationLimit)?,
    };
    let mut mapped_spans = vec![
        rom_span(bytes, 0, 0x50, 8).ok_or(ScanStop::ValidationLimit)?,
        native.driver,
        native.tables,
    ];
    for bank in DRIVER_BANK..=LAST_AUDIO_BANK {
        mapped_spans.push(rom_span(bytes, bank, 0x4000, 0x4000).ok_or(ScanStop::ValidationLimit)?);
    }
    mapped_spans.extend(stream_spans);
    mapped_spans.sort_by_key(|span| (span.effective_offset, span.byte_len));
    mapped_spans.dedup();
    Ok(GbNativeSong {
        profile: profile.id,
        index: index as u16,
        raw_index: raw,
        title: if is_effect {
            format!("Native effect selector {raw:02X}")
        } else {
            format!("Native music selector {raw:02X}")
        },
        bank,
        header,
        table_entry,
        channels,
        native,
        mapped_spans,
        playback_frames: 60 * PLAYBACK_SECONDS as u32,
        playback_clocks: CGB_DOUBLE_CLOCKS * PLAYBACK_SECONDS,
        loop_start_frame: None,
        warnings: vec![
            "Only the listed source-bound timer-queued music and effect selectors are qualified; controls are not enumerated.".into(),
            "Playback is bounded by the requested duration (at most ten minutes); role, natural ending and loop boundaries are unknown.".into(),
        ],
    })
}

fn parse_header(
    bytes: &[u8],
    address: u16,
    effect: bool,
    budget: &mut Budget<'_>,
) -> std::result::Result<(RomSpan, Vec<(u8, u16)>), ScanStop> {
    let mut entries = Vec::with_capacity(4);
    for index in 0..=4 {
        budget.charge()?;
        let offset = address
            .checked_add((index * 3) as u16)
            .ok_or(ScanStop::ValidationLimit)?;
        let first = fetch(bytes, DRIVER_BANK, offset, &mut BTreeSet::new())
            .ok_or(ScanStop::ValidationLimit)?;
        if first == 0xff {
            if (!effect && index != 4) || (effect && entries.is_empty()) {
                return Err(ScanStop::ValidationLimit);
            }
            return Ok((
                rom_span(bytes, DRIVER_BANK, address, index * 3 + 1)
                    .ok_or(ScanStop::ValidationLimit)?,
                entries,
            ));
        }
        if index == 4 || (!effect && first != [0, 1, 4, 6][index]) {
            return Err(ScanStop::ValidationLimit);
        }
        let low = fetch(
            bytes,
            DRIVER_BANK,
            offset.wrapping_add(1),
            &mut BTreeSet::new(),
        )
        .ok_or(ScanStop::ValidationLimit)?;
        let high = fetch(
            bytes,
            DRIVER_BANK,
            offset.wrapping_add(2),
            &mut BTreeSet::new(),
        )
        .ok_or(ScanStop::ValidationLimit)?;
        if effect
            && (!matches!(first & 0x0f, 2 | 3 | 5 | 7)
                || entries
                    .iter()
                    .any(|(seen, _)| (*seen & 0x0f) == (first & 0x0f)))
        {
            return Err(ScanStop::ValidationLimit);
        }
        entries.push((first, u16::from_le_bytes([low, high])));
    }
    Err(ScanStop::ValidationLimit)
}

fn parse_stream(
    bytes: &[u8],
    bank: u8,
    channel: u8,
    start: u16,
    budget: &mut Budget<'_>,
) -> std::result::Result<Vec<RomSpan>, ScanStop> {
    let mut consumed = BTreeSet::new();
    let mut seen = HashMap::new();
    let mut timed = Vec::new();
    let mut address = start;
    let mut raw_frequency = false;
    let mut immediate = 0_u8;
    loop {
        budget.charge()?;
        if seen.len() >= 65_536 {
            return Err(ScanStop::ValidationLimit);
        }
        if let Some(&loop_start) = seen.get(&(address, raw_frequency)) {
            if !timed[loop_start..].iter().any(|&timed| timed) {
                return Err(ScanStop::ValidationLimit);
            }
            break;
        }
        seen.insert((address, raw_frequency), timed.len());
        let opcode = fetch(bytes, bank, address, &mut consumed).ok_or(ScanStop::ValidationLimit)?;
        address = address.wrapping_add(1);
        let mut has_timed_note = false;
        match opcode {
            0xff | 0xfc | 0xfb | 0xfa | 0xf7 | 0xf5 | 0xf4 => break,
            0xfe => {
                let low =
                    fetch(bytes, bank, address, &mut consumed).ok_or(ScanStop::ValidationLimit)?;
                let high = fetch(bytes, bank, address.wrapping_add(1), &mut consumed)
                    .ok_or(ScanStop::ValidationLimit)?;
                address = u16::from_le_bytes([low, high]);
            }
            0xfd | 0xf9 | 0xf8 | 0xf6 | 0xf0 => {
                let argument =
                    fetch(bytes, bank, address, &mut consumed).ok_or(ScanStop::ValidationLimit)?;
                address = address.wrapping_add(1);
                if opcode == 0xf0 {
                    raw_frequency = true;
                }
                if opcode == 0xf6 && matches!(channel, 4 | 5) && argument >= 46 {
                    return Err(ScanStop::ValidationLimit);
                }
            }
            0xf1..=0xf3 | 0xd0..=0xdf => (),
            0xe0..=0xef => {
                fetch(bytes, bank, address, &mut consumed).ok_or(ScanStop::ValidationLimit)?;
                address = address.wrapping_add(1);
            }
            _ => {
                if raw_frequency && channel < 6 {
                    fetch(bytes, bank, address, &mut consumed).ok_or(ScanStop::ValidationLimit)?;
                    address = address.wrapping_add(1);
                }
                fetch(bytes, bank, address, &mut consumed).ok_or(ScanStop::ValidationLimit)?;
                address = address.wrapping_add(1);
                has_timed_note = true;
                immediate = 0;
            }
        }
        if !has_timed_note {
            immediate = immediate.checked_add(1).ok_or(ScanStop::ValidationLimit)?;
            if immediate > 7 {
                return Err(ScanStop::ValidationLimit);
            }
        }
        timed.push(has_timed_note);
    }
    let mut spans: Vec<RomSpan> = Vec::new();
    for offset in consumed {
        if let Some(last) = spans.last_mut()
            && last.effective_offset + last.byte_len == offset as u32
        {
            last.byte_len += 1;
        } else {
            spans.push(RomSpan {
                effective_offset: offset as u32,
                byte_len: 1,
                canonical_cpu_address: 0x4000 + offset as u32 % 0x4000,
            });
        }
    }
    (!spans.is_empty())
        .then_some(spans)
        .ok_or(ScanStop::ValidationLimit)
}

fn fetch(bytes: &[u8], bank: u8, address: u16, consumed: &mut BTreeSet<usize>) -> Option<u8> {
    let span = rom_span(bytes, bank, address, 1)?;
    let offset = span.effective_offset as usize;
    consumed.insert(offset);
    bytes.get(offset).copied()
}

pub(super) fn source_span_matches(media: &MediaIdentity, span: SourceSpan) -> bool {
    if !matches!(media.system, "gb" | "gbc") || !valid_source_span(media.byte_len, span) {
        return false;
    }
    if media.sha256.as_deref().is_some_and(|source| {
        PROFILES
            .iter()
            .any(|profile| profile.source == source && profile.byte_len as u64 == media.byte_len)
    }) {
        return true;
    }
    #[cfg(any(test, feature = "test-support"))]
    {
        fixture::source_matches(media)
    }
    #[cfg(not(any(test, feature = "test-support")))]
    {
        false
    }
}

fn valid_source_span(byte_len: u64, span: SourceSpan) -> bool {
    span.byte_len != 0
        && u64::from(span.effective_offset) < byte_len
        && u64::from(span.byte_len) <= 0x4000 - u64::from(span.effective_offset) % 0x4000
        && span.canonical_cpu_address
            == Some(if span.effective_offset < 0x4000 {
                span.effective_offset
            } else {
                0x4000 + span.effective_offset % 0x4000
            })
}
