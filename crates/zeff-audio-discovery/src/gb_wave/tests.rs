use super::profiles::Profile;

pub(super) const TIMER: Profile = Profile {
    name: "wave-authored-timer",
    hash: "",
    reset: 0x300,
    bank_setter: 0,
    init_bank: 1,
    bank: 1,
    index: 1,
    header: 0x4100,
    entry: 0x4110,
    timer: true,
    spans: &[(0, 0x300, 0x40), (1, 0x4000, 0x120)],
};
pub(super) const FRAME: Profile = Profile {
    name: "wave-authored-frame",
    bank_setter: 0x330,
    timer: false,
    ..TIMER
};

pub(super) fn named(name: &str) -> Option<&'static Profile> {
    match name {
        "wave-authored-timer" => Some(&TIMER),
        "wave-authored-frame" => Some(&FRAME),
        _ => None,
    }
}

pub fn synthetic_rom() -> Vec<u8> {
    let mut rom = vec![0; 0x8000];
    rom[0x100..0x103].copy_from_slice(&[0xc3, 0x50, 1]);
    rom[0x143] = 0xc0;
    rom[0x147] = 0x19;
    put(
        &mut rom,
        0x300,
        &[
            0x21, 0, 0xc0, 0x01, 0, 0x20, 0xaf, 0x22, 0x0b, 0x78, 0xb1, 0x20, 0xf9, 0x21, 0x80,
            0xff, 0x06, 0x70, 0xaf, 0x22, 0x05, 0x20, 0xfc, 0xc9,
        ],
    );
    put(&mut rom, 0x330, &[0x3e, 1, 0xea, 0x85, 0xc1, 0xc9]);
    put(
        &mut rom,
        0x4000,
        &[0xc3, 0x20, 0x40, 0xc3, 0x60, 0x40, 0xc3, 0x50, 0x40],
    );
    put(
        &mut rom,
        0x4020,
        &[
            0xaf, 0xe0, 0x26, 0x3e, 0x80, 0xe0, 0x26, 0x3e, 0x77, 0xe0, 0x24, 0x3e, 0xff, 0xe0,
            0x25, 0x3e, 0x80, 0xe0, 0x11, 0x3e, 0xf0, 0xe0, 0x12, 0xc9,
        ],
    );
    put(&mut rom, 0x4050, &[0xea, 1, 0xc0, 0xc9]);
    put(
        &mut rom,
        0x4060,
        &[
            0x21, 0, 0xc0, 0x34, 0x7e, 0xe0, 0x13, 0x3e, 0x87, 0xe0, 0x14, 0xc9,
        ],
    );
    put(&mut rom, 0x4100, &[1, 2, 3, 4]);
    put(&mut rom, 0x4110, &[0, 0x41]);
    checksum(&mut rom);
    rom
}

pub fn synthetic_frame_rom() -> Vec<u8> {
    let mut rom = synthetic_rom();
    rom[0x411f] = 0x42;
    checksum(&mut rom);
    rom
}

fn put(rom: &mut [u8], offset: usize, bytes: &[u8]) {
    rom[offset..offset + bytes.len()].copy_from_slice(bytes);
}

fn checksum(rom: &mut [u8]) {
    rom[0x14d] = rom[0x134..=0x14c]
        .iter()
        .fold(0u8, |a, b| a.wrapping_sub(*b).wrapping_sub(1));
}

#[cfg(test)]
fn songs(bytes: &[u8]) -> Vec<super::GbWaveSong> {
    let cancel = std::sync::atomic::AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 4_000_000,
    };
    let mut songs = Vec::new();
    super::scan(bytes, &mut songs, &mut budget, 10).unwrap();
    songs
}

#[test]
fn source_and_complete_descriptor_are_authenticated() {
    use std::sync::atomic::AtomicBool;
    let cancel = AtomicBool::new(false);
    for bytes in [synthetic_rom(), synthetic_frame_rom()] {
        let found = songs(&bytes);
        assert_eq!(found.len(), 1);
        let song = &found[0];
        super::validate_song(&bytes, song, &cancel).unwrap();
        for offset in [
            0x100, 0x143, 0x147, 0x148, 0x300, 0x330, 0x4000, 0x4100, 0x7fff,
        ] {
            let mut changed = bytes.clone();
            changed[offset] ^= 1;
            assert!(songs(&changed).is_empty());
            assert!(super::prepare_rom(&changed, song, &cancel).is_err());
        }
        let mut forged = vec![song.clone(); 8];
        forged[0].index += 1;
        forged[1].bank += 1;
        forged[2].header_address += 1;
        forged[3].table_entry.byte_len += 1;
        forged[4].mapped_spans.clear();
        forged[5].warnings.clear();
        forged[6].title.push('x');
        forged[7].profile = "other";
        for descriptor in forged {
            assert!(super::prepare_rom(&bytes, &descriptor, &cancel).is_err());
        }
        let prepared = super::prepare_rom(&bytes, song, &cancel).unwrap();
        assert_eq!(prepared.bytes[0x300..], bytes[0x300..]);
        assert_eq!(
            prepared.timing,
            crate::gb_music::native::GbBankedTiming::CgbDouble
        );
    }
}

#[test]
fn scan_limits_and_cancellation_are_preserved() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let rom = synthetic_rom();
    let cancel = AtomicBool::new(false);
    let mut budget = crate::Budget {
        cancel: &cancel,
        remaining: 0,
    };
    assert_eq!(
        super::scan(&rom, &mut Vec::new(), &mut budget, 10),
        Err(crate::ScanStop::WorkLimit)
    );
    budget.remaining = 4_000_000;
    assert_eq!(
        super::scan(&rom, &mut Vec::new(), &mut budget, 0),
        Err(crate::ScanStop::CandidateLimit)
    );
    let song = songs(&rom).remove(0);
    cancel.store(true, Ordering::Relaxed);
    assert!(super::prepare_rom(&rom, &song, &cancel).is_err());
    assert_eq!(
        super::scan(&rom, &mut Vec::new(), &mut budget, 10),
        Err(crate::ScanStop::Cancelled)
    );
}
