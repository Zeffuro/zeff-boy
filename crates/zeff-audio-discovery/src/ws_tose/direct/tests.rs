#[cfg(test)]
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use crate::{Budget, ScanStop};

use super::super::profiles::Profile;
use super::DirectProfile;

pub(super) const PROFILE: DirectProfile = DirectProfile {
    driver: Profile {
        name: "ws-tose-direct-synthetic",
        len: 0x200000,
        fixed: 0x155e00,
        end: 0x156220,
        hash: "",
        segment: 0x5000,
        init: 0x6100,
        selector: 0x6140,
        tick: 0x61a0,
        status: 0x61e0,
        slots: 0x1e21,
        wave: 0x5e20,
        envelope: 0x5e40,
        frequency: 0x5f50,
        counts: &[],
    },
    selectors: 0x61f0,
    selector_count: 2,
    rows: 0x6200,
    row_count: 5,
    bank: 2,
};

pub fn synthetic_direct_rom() -> Vec<u8> {
    let mut bytes = vec![0; PROFILE.driver.len];
    let fixed = PROFILE.driver.fixed;
    bytes[fixed + 0x20..fixed + 0x30].fill(0xf0);
    bytes[fixed + 0x40..fixed + 0x42].copy_from_slice(&0x5e50_u16.to_le_bytes());
    bytes[fixed + 0x50..fixed + 0x140].fill(0xff);
    bytes[fixed + 0x150..fixed + 0x2a8].fill(1);
    let mut init = vec![
        0x33, 0xc0, 0x8e, 0xd8, 0x8e, 0xc0, 0xe4, 0xc0, 0xa2, 0xf7, 0x1d, 0xc6, 0x06, 0xf4, 0x1d,
        3, 0xc6, 0x06, 0xf8, 0x1d, 0, 0xb0, 0, 0xe6, 0x90, 0xbf, 0x80, 0, 0xb9, 16, 0, 0xb0, 0xf0,
        0xf3, 0xaa, 0xb0, 2, 0xe6, 0x8f, 0xb0, 8, 0xe6, 0x91, 0xbf, 0x21, 0x1e, 0xb9, 8, 0, 0xb8,
        0xff, 0xff,
    ];
    let clear_slots = init.len();
    init.extend([0x89, 0x05, 0x83, 0xc7, 0x34, 0xe2, 0]);
    init[clear_slots + 6] = (clear_slots as isize - init.len() as isize) as u8;
    init.push(0xcb);
    bytes[fixed + 0x300..fixed + 0x300 + init.len()].copy_from_slice(&init);
    let mut selector = vec![
        0x33, 0xdb, 0x8e, 0xdb, 0xa3, 0xf0, 0x1d, 0x89, 0x0e, 0xf2, 0x1d, 0xd1, 0xe0, 0x89, 0xc3,
        0xd1, 0xe0, 0x03, 0xc3, 0x05, 0x00, 0x62, 0x89, 0xc7, 0xb8, 0x00, 0x30, 0x8e, 0xc0, 0xe4,
        0xc3, 0xa2, 0xf6, 0x1d, 0xb0, 0xe2, 0xe6, 0xc3, 0x26, 0x8b, 0x1d, 0xbe, 0x21, 0x1e, 0x03,
        0xf3, 0xc7, 0x04, 0x02, 0x00, 0x26, 0x8a, 0x45, 0x02, 0x88, 0x44, 0x02, 0x26, 0x8b, 0x45,
        0x04, 0x89, 0x44, 0x03, 0xc6, 0x44, 0x05, 0xe2, 0x83, 0xc7, 0x06,
    ];
    let copy_row = 38;
    let loop_branch = selector.len();
    selector.extend([
        0xe2, 0, 0xc6, 0x06, 0xf8, 0x1d, 1, 0xa0, 0xf6, 0x1d, 0xe6, 0xc3, 0xcb,
    ]);
    selector[loop_branch + 1] = (copy_row as isize - (loop_branch + 2) as isize) as u8;
    bytes[fixed + 0x340..fixed + 0x340 + selector.len()].copy_from_slice(&selector);
    let mut tick = vec![
        0x33, 0xc0, 0x8e, 0xd8, 0x80, 0x3e, 0xf8, 0x1d, 0, 0x74, 0, 0xfe, 0x0e, 0xf4, 0x1d, 0x75, 0,
    ];
    tick.extend([
        0xc6, 0x06, 0xf8, 0x1d, 0, 0xbf, 0x21, 0x1e, 0xb9, 8, 0, 0xb8, 0xff, 0xff,
    ]);
    let clear_slots = tick.len();
    tick.extend([
        0x89, 0x05, 0x83, 0xc7, 0x34, 0xe2, 0, 0xb0, 0, 0xe6, 0x90, 0xcb,
    ]);
    let emit = tick.len();
    tick.extend([
        0xb8, 0, 4, 0xe7, 0x80, 0xb0, 0xff, 0xe6, 0x88, 0xb0, 0x41, 0xe6, 0x90, 0xcb,
    ]);
    let quiet = tick.len();
    tick.push(0xcb);
    tick[10] = (quiet as isize - 11) as u8;
    tick[16] = (emit as isize - 17) as u8;
    tick[clear_slots + 6] = (clear_slots as isize - (clear_slots + 7) as isize) as u8;
    bytes[fixed + 0x3a0..fixed + 0x3a0 + tick.len()].copy_from_slice(&tick);
    bytes[fixed + 0x3e0] = 0xcb;
    bytes[fixed + 0x3f0..fixed + 0x3f8].copy_from_slice(&[0, 0, 4, 0, 4, 0, 1, 0]);
    for row in 0..5 {
        let at = 0x26200 + row * 6;
        bytes[at..at + 2].copy_from_slice(&((row * 0x34) as u16).to_le_bytes());
        bytes[at + 2..at + 4].copy_from_slice(&(row as u16 % 4).to_le_bytes());
        bytes[at + 4..at + 6].copy_from_slice(&(0x6300_u16 + (row * 8) as u16).to_le_bytes());
        let sequence = 0x26300 + row * 8;
        bytes[sequence..sequence + 2].copy_from_slice(&8_u16.to_le_bytes());
        bytes[sequence + 2..sequence + 4].copy_from_slice(&1_u16.to_le_bytes());
        bytes[sequence + 4..sequence + 6].copy_from_slice(&1_u16.to_le_bytes());
        bytes[sequence + 6..sequence + 8].copy_from_slice(&0x00ff_u16.to_le_bytes());
    }
    let footer = bytes.len() - 9;
    bytes[footer] = 1;
    bytes
}

pub(super) fn recognized(bytes: &[u8]) -> bool {
    bytes.len() == PROFILE.driver.len
        && bytes.get(PROFILE.driver.fixed..PROFILE.driver.end)
            == synthetic_direct_rom().get(PROFILE.driver.fixed..PROFILE.driver.end)
}

#[cfg(test)]
fn scan_inventory(bytes: &[u8]) -> (bool, Vec<super::WsToseSong>) {
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
fn identifies_direct_rows_and_preserves_mapped_source() {
    let bytes = synthetic_direct_rom();
    let (recognized, songs) = scan_inventory(&bytes);
    assert!(recognized);
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].profile, PROFILE.driver.name);
    assert_eq!(songs[0].index, 0);
    assert_eq!(songs[0].table_entry.effective_offset, 0x1561f0);
    assert_eq!(songs[0].table_entry.canonical_cpu_address, 0x561f0);
    assert_eq!(songs[0].tracks.len(), 4);
    assert!(songs[0].tracks.iter().all(|track| track.note_count == 1));
    assert_eq!(songs[1].index, 1);
    assert_eq!(songs[1].tracks.len(), 1);
    assert_eq!(songs[1].tracks[0].slot, 4);
    assert_eq!(songs[1].tracks[0].note_count, 1);
    assert!(songs[0].mapped_spans.iter().any(|span| {
        span.effective_offset == 0x26200
            && span.byte_len == 24
            && span.canonical_cpu_address == 0x36200
    }));
    assert!(songs[1].mapped_spans.iter().any(|span| {
        span.effective_offset == 0x26218
            && span.byte_len == 6
            && span.canonical_cpu_address == 0x36218
    }));
    let cancel = AtomicBool::new(false);
    let prepared = super::prepare_rom(&bytes, &songs[0], &cancel).unwrap();
    for span in &songs[0].mapped_spans {
        let start = span.effective_offset as usize;
        let end = start + span.byte_len as usize;
        assert_eq!(&prepared.bytes[start..end], &bytes[start..end]);
    }
}

#[test]
fn retains_direct_ax_cx_pairs() {
    let bytes = synthetic_direct_rom();
    assert_eq!(super::selector(&bytes, PROFILE, 0).unwrap(), (0, 4));
    assert_eq!(super::selector(&bytes, PROFILE, 1).unwrap(), (4, 1));
    assert!(super::selector(&bytes, PROFILE, 2).is_err());
}

#[test]
fn rejects_stop_only_selections() {
    let mut bytes = synthetic_direct_rom();
    for row in 0..4 {
        let command = 0x26304 + row * 8;
        bytes[command..command + 2].copy_from_slice(&0x00ff_u16.to_le_bytes());
    }
    let (recognized, songs) = scan_inventory(&bytes);
    assert!(recognized);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].index, 1);
}

#[test]
fn rejects_corrupt_driver_rows_and_sequences() {
    let mut code = synthetic_direct_rom();
    code[0x156100] ^= 1;
    assert!(!scan_inventory(&code).0);
    let mut table = synthetic_direct_rom();
    table[0x1561f0] ^= 1;
    assert!(!scan_inventory(&table).0);
    let mut pcm = synthetic_direct_rom();
    pcm[0x155e20] ^= 1;
    assert!(!scan_inventory(&pcm).0);
    let mut slot = synthetic_direct_rom();
    slot[0x26200] = 1;
    assert_eq!(scan_inventory(&slot).1.len(), 1);
    let mut sequence = synthetic_direct_rom();
    sequence[0x26304..0x26306].copy_from_slice(&[0xa9, 0xfe]);
    assert_eq!(scan_inventory(&sequence).1.len(), 1);
    let mut immediate_loop = synthetic_direct_rom();
    immediate_loop[0x26304..0x26308].copy_from_slice(&[0xfd, 0, 0xb0, 0xf0]);
    assert_eq!(scan_inventory(&immediate_loop).1.len(), 1);
}

#[test]
fn cancellation_work_and_candidate_limits_are_typed() {
    let bytes = synthetic_direct_rom();
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

#[test]
fn rejects_a_forged_direct_inventory() {
    let bytes = synthetic_direct_rom();
    let mut songs = scan_inventory(&bytes).1;
    songs[0].index = 1;
    let cancel = AtomicBool::new(false);
    assert!(super::prepare_rom(&bytes, &songs[0], &cancel).is_err());
}
