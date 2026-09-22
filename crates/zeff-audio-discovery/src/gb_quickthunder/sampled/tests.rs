use super::*;
#[cfg(test)]
use crate::gb_quickthunder::{self, word};

static TEST_PROFILE: Profile = Profile {
    name: "gb-quickthunder-timer-13-01",
    header: HeaderKind::Executable,
    len: 0x100,
    hash: "",
    selector: 0x4000,
    tick: 0x4040,
    stride: 13,
    pattern_bytes: 2,
    operands: [0; 5],
    aliases: &[],
    empty_operand: None,
    default_empty: 0x4400,
    release_operand: 0,
};

fn driver() -> Driver {
    Driver {
        profile: &TEST_PROFILE,
        bank: 62,
        wram: 0xc100,
        table: 0x4100,
        sequences: 0x4180,
        instruments: 0x4500,
        noise: 0x4600,
        frequency: 0x4700,
        empty: 0x4400,
        release: 0x4410,
    }
}

pub(crate) fn recognize(bytes: &[u8], drivers: &mut Vec<Driver>) {
    if bytes.len() == 0x100000
        && bytes[0xf8000..0xf8004] == [0x3e, 0x80, 0xe0, 0x26]
        && bytes == synthetic_rom()
    {
        drivers.push(driver());
    }
}

pub fn synthetic_rom() -> Vec<u8> {
    let source = super::super::tests::synthetic_rom();
    let mut bytes = vec![0; 0x100000];
    bytes[..0x150].copy_from_slice(&source[..0x150]);
    bytes[0x143] = 0xc0;
    bytes[0x148] = 5;
    let base = 62 * 0x4000;
    bytes[base..base + 0x4000].copy_from_slice(&source[0x8000..0xc000]);
    bytes[base + 0x180..base + 0x188].copy_from_slice(&source[0x8110..0x8118]);
    for index in 0..7 {
        let at = base + 0x100 + index * 13;
        bytes[at..at + 13].copy_from_slice(&source[0x8100..0x810d]);
    }
    bytes[base + 0x322] = 0;
    bytes[base + 0x2c54..base + 0x2c5a].copy_from_slice(&[1, 0, 0x50, 1, 0, 63]);
    bytes[63 * 0x4000 + 0x1000..63 * 0x4000 + 0x1010].fill(0x37);
    let timer_setup = [0x78, 0xe0, 5, 0xe0, 6, 0x3e, 4, 0xe0, 7, 0xc9];
    bytes[base + 28..base + 28 + timer_setup.len()].copy_from_slice(&timer_setup);
    let tick = [
        0x21, 0, 0xc1, 0x34, 0x7e, 0xe0, 0x13, 0xf0, 0xff, 0xf6, 4, 0xe0, 0xff, 0xc9,
    ];
    bytes[base + 0x40..base + 0x40 + tick.len()].copy_from_slice(&tick);
    bytes[0x50..0x53].copy_from_slice(&[0xc3, 6, 0xca]);
    let callback = [
        0xf5, 0xc5, 0xe5, 0x3e, 63, 0xea, 0, 0x20, 0x21, 0, 0x50, 0x0e, 0x30, 0x2a, 0xe2, 0x0c,
        0x79, 0xfe, 0x40, 0x20, 0xf8, 0x3e, 62, 0xea, 0, 0x20, 0x21, 0x64, 0xc1, 0x34, 0xe1, 0xc1,
        0xf1, 0xd9,
    ];
    bytes[0x1740..0x1740 + callback.len()].copy_from_slice(&callback);
    bytes
}

#[cfg(test)]
fn parse(
    bytes: &[u8],
    index: u16,
) -> Result<gb_quickthunder::GbQuickThunderSong, gb_quickthunder::ReadError> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    gb_quickthunder::song(bytes, driver(), index, &mut budget)
}

#[test]
fn sample_descriptors_preserve_physical_rom_provenance() {
    let bytes = synthetic_rom();
    let song = parse(&bytes, 1).unwrap();
    assert_eq!(song.tracks[2].note_count, 2);
    let covered = |at: u32| {
        song.mapped_spans.iter().any(|span| {
            (span.effective_offset..span.effective_offset + span.byte_len).contains(&at)
        })
    };
    for at in [0x50, 0x1740, 0x17e3, 0xfac54, 0xfd000, 0xfd00f] {
        assert!(covered(at), "unmapped {at:x}");
    }
    assert!(!covered(0xfd010));
    assert_eq!(
        song.mapped_spans
            .iter()
            .find(|s| s.effective_offset == 0xfd000)
            .unwrap()
            .canonical_cpu_address,
        0x5000
    );
    for index in [0, 2, 3, 7, 8] {
        assert!(parse(&bytes, index).is_err());
    }
}

#[test]
fn sampled_track_holds_and_fd_fe_are_distinct_from_wave_instruments() {
    for note in [0xfd, 0xfe] {
        let mut bytes = synthetic_rom();
        bytes[0xf8321] = note;
        assert_eq!(parse(&bytes, 1).unwrap().tracks[2].note_count, 2);
    }
    let mut bytes = synthetic_rom();
    bytes[0xf8320..0xf8328].copy_from_slice(&[1, 0xff, 1, 48, 0, 0xff, 0x20, 0x42]);
    assert_eq!(parse(&bytes, 1).unwrap().tracks[2].note_count, 1);
    bytes[0xf8301] = 0xff;
    assert!(parse(&bytes, 1).is_err());
}

#[test]
fn malformed_sample_banks_bounds_and_indices_fail_closed() {
    for (at, value) in [
        (0xf8322, 21),
        (0xfac56, 0x3f),
        (0xfac56, 0x80),
        (0xfac57, 0),
        (0xfac58, 0xff),
        (0xfac59, 62),
    ] {
        let mut bytes = synthetic_rom();
        bytes[at] = value;
        assert!(parse(&bytes, 1).is_err(), "mutation {at:x}");
    }
}

#[test]
fn sampled_source_and_metadata_are_revalidated_before_preparation() {
    use std::sync::atomic::AtomicBool;
    let bytes = synthetic_rom();
    let cancel = AtomicBool::new(false);
    let song = parse(&bytes, 1).unwrap();
    let prepared = gb_quickthunder::prepare_rom(&bytes, &song, &cancel).unwrap();
    assert_eq!(&prepared.bytes[0x50..0x53], &bytes[0x50..0x53]);
    assert_eq!(&prepared.bytes[0x1740..0x17e4], &bytes[0x1740..0x17e4]);
    assert_eq!(&prepared.bytes[0x4000..], &bytes[0x4000..]);
    assert_eq!(word(&prepared.bytes, 0x152), 0xdff0);
    for at in [0x143, 0x147, 0x148, 0x1740, 0xf8000, 0xfd000] {
        let mut changed = bytes.clone();
        changed[at] ^= 1;
        assert!(gb_quickthunder::prepare_rom(&changed, &song, &cancel).is_err());
    }
    let mut stale = song.clone();
    stale.mapped_spans.pop();
    assert!(gb_quickthunder::prepare_rom(&bytes, &stale, &cancel).is_err());
    assert!(gb_quickthunder::prepare_rom(&bytes, &song, &AtomicBool::new(true)).is_err());
}
