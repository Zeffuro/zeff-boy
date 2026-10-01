#[cfg(test)]
use std::sync::atomic::AtomicBool;

#[cfg(test)]
use crate::{Budget, ScanStop};

use super::super::profiles::Profile;
use super::{ScaledProfile, Selectors};

const CALLERS: &[usize] = &[0x85000, 0x85010, 0x85020, 0x85030];

pub(super) const PROFILE: ScaledProfile = ScaledProfile {
    driver: Profile {
        name: "ws-tose-scaled-relocated-synthetic",
        len: 0x100000,
        fixed: 0x0e6000,
        end: 0x0e6500,
        hash: "",
        segment: 0xe000,
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
    rom_group: 0x20,
    selectors: Selectors::Callers(CALLERS),
    rows: 0x6500,
    row_count: 10,
};

pub fn synthetic_relocated_scaled_rom() -> Vec<u8> {
    let source = super::tests::synthetic_scaled_rom();
    let mut bytes = vec![0; PROFILE.driver.len];
    bytes[0x0e6000..0x0e6800].copy_from_slice(&source[0x3f6000..0x3f6800]);

    let selector = PROFILE.driver.offset(0x6098);
    let segment = selector + 32;
    assert_eq!(&bytes[segment..segment + 5], &[0xb8, 0, 0xf0, 0x8e, 0xc0]);
    bytes[segment + 2] = 0xe0;

    for (at, first, entry) in [
        (0x85000, 0, 0x6092),
        (0x85010, 1, 0x608c),
        (0x85020, 3, 0x6086),
        (0x85030, 6, 0x6080),
    ] {
        bytes[at..at + 8].copy_from_slice(&[
            0xb8,
            first,
            0,
            0x9a,
            entry as u8,
            (entry >> 8) as u8,
            0,
            0xe0,
        ]);
    }
    bytes
}

pub(super) fn recognized(bytes: &[u8]) -> bool {
    let source = synthetic_relocated_scaled_rom();
    bytes.len() == PROFILE.driver.len
        && bytes.get(PROFILE.driver.fixed..PROFILE.driver.end)
            == source.get(PROFILE.driver.fixed..PROFILE.driver.end)
        && CALLERS
            .iter()
            .all(|&at| bytes.get(at..at + 8) == source.get(at..at + 8))
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
fn identifies_relocated_callers_with_e_window_mappings() {
    let bytes = synthetic_relocated_scaled_rom();
    let (recognized, songs) = inventory(&bytes);
    assert!(recognized);
    assert_eq!(songs.len(), 4);
    for (index, song) in songs.iter().enumerate() {
        assert_eq!(song.profile, PROFILE.driver.name);
        assert_eq!(song.index, index as u16);
        assert_eq!(song.tracks.len(), index + 1);
        assert_eq!(
            song.table_entry.effective_offset,
            0x85000 + index as u32 * 0x10
        );
        assert_eq!(song.table_entry.byte_len, 8);
        assert_eq!(
            song.table_entry.canonical_cpu_address,
            0x85000 + index as u32 * 0x10
        );
    }
    assert!(songs[3].mapped_spans.iter().any(|span| {
        span.effective_offset == 0x0e6524
            && span.byte_len == 24
            && span.canonical_cpu_address == 0x0e6524
    }));
    assert!(songs[0].mapped_spans.iter().any(|span| {
        span.effective_offset == PROFILE.driver.fixed as u32
            && span.byte_len >= (PROFILE.driver.end - PROFILE.driver.fixed) as u32
            && span.canonical_cpu_address == 0x0e6000
    }));
}

#[test]
fn preserves_relocated_code_data_and_callers() {
    let bytes = synthetic_relocated_scaled_rom();
    let source = super::tests::synthetic_scaled_rom();
    let mut expected = source[0x3f6000..0x3f6800].to_vec();
    expected[0xb8..0xbd].copy_from_slice(&[0xb8, 0, 0xe0, 0x8e, 0xc0]);
    assert_eq!(&bytes[0x0e6000..0x0e6800], expected.as_slice());
    assert_eq!(&bytes[0x0e60b8..0x0e60bd], &[0xb8, 0, 0xe0, 0x8e, 0xc0]);
    assert_eq!(
        &bytes[0x85000..0x85008],
        &[0xb8, 0, 0, 0x9a, 0x92, 0x60, 0, 0xe0]
    );
    assert_eq!(
        &bytes[0x85030..0x85038],
        &[0xb8, 6, 0, 0x9a, 0x80, 0x60, 0, 0xe0]
    );
}

#[test]
fn rejects_altered_caller_segment_opcode_and_entry() {
    for (at, value) in [(0x85007, 0xf0), (0x85013, 0x90), (0x85024, 0x92)] {
        let mut bytes = synthetic_relocated_scaled_rom();
        bytes[at] = value;
        assert!(!inventory(&bytes).0);
    }
}

#[test]
fn rejects_mutated_source_and_forged_caller_inventory() {
    let mut source = synthetic_relocated_scaled_rom();
    source[0x0e6000] ^= 1;
    assert!(!inventory(&source).0);

    let mut songs = inventory(&synthetic_relocated_scaled_rom()).1;
    songs[0].index = 1;
    let cancel = AtomicBool::new(false);
    assert!(super::validate_song(&synthetic_relocated_scaled_rom(), &songs[0], &cancel).is_err());
}

#[test]
fn cancellation_and_candidate_limit_stay_typed_for_relocated_callers() {
    let bytes = synthetic_relocated_scaled_rom();
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
    assert_eq!(
        super::scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(ScanStop::CandidateLimit)
    );
}
