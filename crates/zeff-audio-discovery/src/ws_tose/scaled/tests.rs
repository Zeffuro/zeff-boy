#[cfg(test)]
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use crate::{Budget, ScanStop};

use super::super::profiles::Profile;
use super::{ScaledProfile, Selectors};

pub(super) const PROFILE: ScaledProfile = ScaledProfile {
    driver: Profile {
        name: "ws-tose-scaled-synthetic",
        len: 0x400000,
        fixed: 0x3f6000,
        end: 0x3f6500,
        hash: "",
        segment: 0xf000,
        init: 0x6000,
        selector: 0x6080,
        tick: 0x6180,
        status: 0,
        slots: 0x0c00,
        wave: 0x6200,
        envelope: 0x6380,
        frequency: 0x6300,
        counts: &[],
    },
    source_hash: "",
    rom_group: 0x23,
    selectors: Selectors::Pairs {
        offset: 0x5f00,
        count: 4,
    },
    rows: 0x6500,
    row_count: 10,
};

pub fn synthetic_scaled_rom() -> Vec<u8> {
    let mut bytes = vec![0; PROFILE.driver.len];
    let driver = PROFILE.driver;
    let fixed = driver.fixed;
    bytes[driver.offset(driver.wave)..driver.offset(driver.wave) + 16].fill(0xf0);
    bytes[driver.offset(driver.envelope)..driver.offset(driver.envelope) + 2]
        .copy_from_slice(&0x6390_u16.to_le_bytes());
    bytes[driver.offset(0x6390)..driver.offset(0x6390) + 240].fill(0xf0);
    bytes[driver.offset(driver.frequency)..driver.offset(driver.frequency) + 48].fill(1);

    let init = [
        0x60, 0x1e, 0x06, 0x33, 0xc0, 0x8e, 0xd8, 0x8e, 0xc0, 0xbb, 0, 0x0c, 0xb9, 8, 0, 0xb8,
        0xff, 0xff, 0x89, 0x07, 0x83, 0xc3, 0x2e, 0xe2, 0xf9, 0xbf, 0xc0, 0, 0xb9, 16, 0, 0xb0,
        0xf0, 0xf3, 0xaa, 0xb0, 3, 0xe6, 0x8f, 0xb0, 8, 0xe6, 0x91, 0xb0, 0, 0xe6, 0x90, 0x07,
        0x1f, 0x61, 0xcb,
    ];
    write(&mut bytes, fixed, &init);

    for (at, count) in [(0x6080, 4), (0x6086, 3), (0x608c, 2), (0x6092, 1)] {
        let target = 0x6098_i32;
        write(
            &mut bytes,
            driver.offset(at),
            &[
                0x51,
                0xb9,
                count,
                0,
                0xeb,
                (target - i32::from(at) - 6) as u8,
            ],
        );
    }
    let mut selector = vec![
        0x60, 0x1e, 0x06, 0x33, 0xdb, 0x8e, 0xdb, 0xa3, 0xf0, 0x1d, 0x89, 0x0e, 0xf2, 0x1d, 0xc6,
        0x06, 0xf4, 0x1d, 3, 0xd1, 0xe0, 0x89, 0xc2, 0xd1, 0xe0, 0x01, 0xd0, 0x05, 0, 0x65, 0x89,
        0xc3, 0xb8, 0, 0xf0, 0x8e, 0xc0,
    ];
    let activate = selector.len();
    selector.extend([
        0x26, 0x8b, 0x3f, 0x81, 0xc7, 0, 0x0c, 0xc7, 0x05, 2, 0, 0x83, 0xc3, 6, 0xe2, 0, 0x07,
        0x1f, 0x61, 0x59, 0xcb,
    ]);
    selector[activate + 15] = (-16_i8) as u8;
    write(&mut bytes, driver.offset(0x6098), &selector);
    let mut tick = vec![
        0x60, 0x1e, 0x33, 0xc0, 0x8e, 0xd8, 0x80, 0x3e, 0xf4, 0x1d, 0,
    ];
    let muted = tick.len();
    tick.extend([
        0x74, 0, 0xb8, 0, 4, 0xe7, 0x80, 0xb0, 0xff, 0xe6, 0x88, 0xb0, 1, 0xe6, 0x90, 0xfe, 0x0e,
        0xf4, 0x1d,
    ]);
    let active = tick.len();
    tick.extend([0x75, 0]);
    tick.extend([0xbb, 0, 0x0c, 0xb9, 8, 0, 0xb8, 0xff, 0xff]);
    tick.extend([
        0x89, 0x07, 0x83, 0xc3, 0x2e, 0xe2, 0xf9, 0xb0, 0, 0xe6, 0x90,
    ]);
    let quiet = tick.len();
    tick.extend([0x1f, 0x61, 0xcb]);
    tick[muted + 1] = (quiet - (muted + 2)) as u8;
    tick[active + 1] = (quiet - (active + 2)) as u8;
    write(&mut bytes, driver.offset(driver.tick), &tick);

    for (index, (first, count)) in [(0, (0, 1)), (1, (1, 2)), (2, (3, 3)), (3, (6, 4))] {
        bytes[driver.offset(0x5f00) + index * 2] = first;
        bytes[driver.offset(0x5f00) + index * 2 + 1] = count;
    }
    for row in 0..usize::from(PROFILE.row_count) {
        let table = driver.offset(PROFILE.rows) + row * 6;
        bytes[table..table + 2].copy_from_slice(&((row % 8 * 0x2e) as u16).to_le_bytes());
        bytes[table + 2..table + 4].copy_from_slice(&((row % 4) as u16).to_le_bytes());
        let pointer = 0x6600_u16 + row as u16 * 0x20;
        bytes[table + 4..table + 6].copy_from_slice(&pointer.to_le_bytes());
        track(&mut bytes, pointer, &[0, 1, 1, 0x00ff]);
    }
    track(
        &mut bytes,
        0x6600,
        &[0, 1, 0x00a8, 0x02a9, 0x01ac, 0x0010, 1, 0x00ff, 0x00ad],
    );
    track(
        &mut bytes,
        0x6620,
        &[0, 1, 0x01a9, 0xffa9, 0, 0x0006, 1, 0x00ff],
    );
    bytes
}

pub(super) fn recognized(bytes: &[u8]) -> bool {
    bytes.len() == PROFILE.driver.len
        && bytes.get(PROFILE.driver.fixed..PROFILE.driver.end)
            == synthetic_scaled_rom().get(PROFILE.driver.fixed..PROFILE.driver.end)
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> (bool, Vec<super::WsToseSong>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut songs = Vec::new();
    (
        super::scan(bytes, &mut songs, &mut budget, 100).unwrap(),
        songs,
    )
}

#[test]
fn identifies_scaled_groups_and_standard_sequence_controls() {
    let bytes = synthetic_scaled_rom();
    let (recognized, songs) = inventory(&bytes);
    assert!(recognized);
    assert_eq!(songs.len(), 4);
    for (song, count) in songs.iter().zip(1..=4) {
        assert_eq!(song.profile, PROFILE.driver.name);
        assert_eq!(song.index, (count - 1) as u16);
        assert_eq!(song.tracks.len(), count);
        assert!(song.tracks.iter().all(|track| track.note_count == 1));
    }
    assert_eq!(songs[0].table_entry.effective_offset, 0x3f5f00);
    assert_eq!(songs[0].table_entry.canonical_cpu_address, 0xf5f00);
    assert!(songs[3].mapped_spans.iter().any(|span| {
        span.effective_offset == 0x3f6524
            && span.byte_len == 24
            && span.canonical_cpu_address == 0xf6524
    }));
    assert!(songs[0].mapped_spans.iter().any(|span| {
        span.effective_offset == PROFILE.driver.fixed as u32
            && span.byte_len >= (PROFILE.driver.end - PROFILE.driver.fixed) as u32
            && span.canonical_cpu_address == 0xf6000
    }));
    let cancel = AtomicBool::new(false);
    let prepared = super::prepare_rom(&bytes, &songs[0], &cancel).unwrap();
    assert_eq!(
        &prepared.bytes[bytes.len() - 16..bytes.len() - 11],
        &[0xea, 0, 0xe0, 0, 0xf0]
    );
    for span in &songs[0].mapped_spans {
        let start = span.effective_offset as usize;
        let end = start + span.byte_len as usize;
        assert_eq!(&prepared.bytes[start..end], &bytes[start..end]);
    }
}

#[test]
fn rejects_pcm_and_invalid_control_paths() {
    let mut header_pcm = synthetic_scaled_rom();
    header_pcm[PROFILE.driver.offset(0x6620) + 3] = 1;
    assert_eq!(inventory(&header_pcm).1.len(), 3);

    let mut command_pcm = synthetic_scaled_rom();
    track(&mut command_pcm, 0x6620, &[0, 1, 0x01a4, 1, 0x00ff]);
    assert_eq!(inventory(&command_pcm).1.len(), 3);

    let mut immediate_loop = synthetic_scaled_rom();
    track(&mut immediate_loop, 0x6600, &[0, 1, 0x00fd, 0xf0b0]);
    assert_eq!(inventory(&immediate_loop).1.len(), 3);
}

#[test]
fn rejects_bad_rows_and_forged_inventory() {
    let mut source = synthetic_scaled_rom();
    source[PROFILE.driver.offset(PROFILE.driver.init)] ^= 1;
    assert!(!inventory(&source).0);

    let mut slot = synthetic_scaled_rom();
    slot[PROFILE.driver.offset(PROFILE.rows)] = 1;
    assert_eq!(inventory(&slot).1.len(), 3);

    let mut duplicate = synthetic_scaled_rom();
    let row = PROFILE.driver.offset(PROFILE.rows) + 2 * 6;
    duplicate[row..row + 2].copy_from_slice(&(0x2e_u16).to_le_bytes());
    assert_eq!(inventory(&duplicate).1.len(), 3);

    let mut songs = inventory(&synthetic_scaled_rom()).1;
    songs[0].index = 1;
    let cancel = AtomicBool::new(false);
    assert!(super::validate_song(&synthetic_scaled_rom(), &songs[0], &cancel).is_err());
}

#[test]
fn cancellation_work_and_candidate_limits_are_typed() {
    let bytes = synthetic_scaled_rom();
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 1),
        Err(ScanStop::Cancelled)
    );
    cancel.store(false, std::sync::atomic::Ordering::Relaxed);
    budget.remaining = 1;
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 1),
        Err(ScanStop::WorkLimit)
    );
    budget.remaining = 100_000;
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(ScanStop::CandidateLimit)
    );
}

fn track(bytes: &mut [u8], pointer: u16, words: &[u16]) {
    let at = PROFILE.driver.offset(pointer);
    for (index, word) in words.iter().copied().enumerate() {
        bytes[at + index * 2..at + index * 2 + 2].copy_from_slice(&word.to_le_bytes());
    }
}

fn write(bytes: &mut [u8], at: usize, source: &[u8]) {
    bytes[at..at + source.len()].copy_from_slice(source);
}
