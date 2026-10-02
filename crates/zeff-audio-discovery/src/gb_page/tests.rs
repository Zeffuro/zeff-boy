#[cfg(test)]
use std::sync::atomic::AtomicBool;

use super::profiles::{Profile, Selection};
#[cfg(test)]
use super::*;

pub(super) const PROFILE: Profile = Profile {
    name: "page-authored-four-channel-v1",
    hash: "",
    setup: 0x400,
    stop: 0x420,
    start: 0x450,
    tick: 0x480,
    page: 0xcd00,
    wrapper: 0xc680,
    double: true,
    protected: &[(0x400, 0x49b), (0x600, 0x603)],
    selections: &[Selection {
        index: 1,
        entry: 0x600,
        bank: 1,
        module: 0x4000,
        spans: &[(0, 0x400, 0xc0), (0, 0x600, 3), (1, 0x4000, 8)],
    }],
};

pub(super) const NORMAL: Profile = Profile {
    name: "page-authored-single-speed-v1",
    double: false,
    wrapper: 0,
    ..PROFILE
};

pub fn synthetic_normal_rom() -> Vec<u8> {
    let mut rom = synthetic_rom();
    rom[0x403] = 0xfe;
    rom
}

pub fn synthetic_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    rom[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    rom[0x143] = 0xc0;
    rom[0x147] = 0x19;
    rom[0x400..0x407].copy_from_slice(&[0x3e, 0xcd, 0xe0, 0xfd, 0xc9, 0, 0]);
    rom[0x420..0x42d].copy_from_slice(&[
        0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0x3e, 0xff, 0xe0, 0x25, 0xc9,
    ]);
    rom[0x450..0x456].copy_from_slice(&[0xaf, 0xea, 8, 0xcd, 0xc9, 0]);
    rom[0x480..0x49b].copy_from_slice(&[
        0x21, 8, 0xcd, 0x34, 0x7e, 0xe6, 1, 0xc6, 0x60, 0xe0, 0x13, 0x3e, 0x80, 0xe0, 0x11, 0x3e,
        0xf1, 0xe0, 0x12, 0x3e, 0x87, 0xe0, 0x14, 0xc9, 0, 0, 0,
    ]);
    rom[0x600..0x603].copy_from_slice(&[0xfe, 0, 0x40]);
    rom[0x4000..0x4008].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
    rom[0x14d] = rom[0x134..=0x14c]
        .iter()
        .fold(0u8, |a, b| a.wrapping_sub(*b).wrapping_sub(1));
    rom
}

#[test]
fn exact_source_and_canonical_descriptor_required() {
    for rom in [synthetic_rom(), synthetic_normal_rom()] {
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 4_000_000,
        };
        let mut songs = Vec::new();
        scan(&rom, &mut songs, &mut budget, 10).unwrap();
        assert_eq!(songs.len(), 1);
        let song = &songs[0];
        validate_song(&rom, song, &cancel).unwrap();
        let mut changed = rom.clone();
        changed[0x4007] ^= 1;
        assert!(validate_song(&changed, song, &cancel).is_err());
        let mut forged = song.clone();
        forged.double_speed = !song.double_speed;
        assert!(validate_song(&rom, &forged, &cancel).is_err());
        forged = song.clone();
        forged.table_entry.byte_len += 1;
        assert!(validate_song(&rom, &forged, &cancel).is_err());
        forged = song.clone();
        forged.mapped_spans.reverse();
        assert!(validate_song(&rom, &forged, &cancel).is_err());
        let prepared = prepare_rom(&rom, song, &cancel).unwrap();
        assert_eq!(prepared.ack_address, 0xfffb);
        assert_eq!(&prepared.bytes[0x480..0x49b], &rom[0x480..0x49b]);
    }
}

#[test]
fn cancellation_and_candidate_budget_are_preserved() {
    let rom = synthetic_rom();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 4_000_000,
    };
    let mut songs = Vec::new();
    assert_eq!(
        scan(&rom, &mut songs, &mut budget, 0),
        Err(ScanStop::CandidateLimit)
    );
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 4_000_000,
    };
    assert_eq!(
        scan(&rom, &mut songs, &mut budget, 10),
        Err(ScanStop::Cancelled)
    );
}
