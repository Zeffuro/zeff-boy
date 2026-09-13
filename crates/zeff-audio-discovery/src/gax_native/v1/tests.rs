use std::sync::atomic::AtomicBool;

use super::*;

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn address(at: usize) -> u32 {
    0x0800_0000 + at as u32
}

fn load(bytes: &mut [u8], at: usize, slot: usize, register: u16, value: u32) {
    put16(
        bytes,
        at,
        0x4800 | register << 8 | ((slot - ((at + 4) & !3)) / 4) as u16,
    );
    put32(bytes, slot, value);
}

fn handler(bytes: &mut [u8], at: usize, data: usize) {
    for offset in [0, 4, 8] {
        put32(bytes, at + offset, address(0xc00) | 1);
    }
    put32(bytes, at + 24, address(data));
}

fn fixture(newer: bool, channels: u16) -> Vec<u8> {
    let mut bytes = vec![0; 0x1800];
    let banner = b"GAX Sound Engine v1.99f (Jun 13 2001) Test\0";
    bytes[0x80..0x80 + banner.len()].copy_from_slice(banner);
    let init = if newer {
        &signatures::INIT_NEW
    } else {
        &signatures::INIT_OLD
    };
    let play = if newer {
        &signatures::PLAY_NEW
    } else {
        &signatures::PLAY_OLD
    };
    for (at, pattern) in [(0x200, init), (0x800, &signatures::IRQ), (0xa00, play)] {
        bytes[at..at + pattern.code.len()].copy_from_slice(pattern.code);
    }
    let state = 0x0300_73f0;
    load(
        &mut bytes,
        0x200 + if newer { 34 } else { 30 },
        0x300,
        6,
        state,
    );
    load(&mut bytes, 0x802, 0x900, 4, state);
    load(&mut bytes, 0x800 + 30, 0x904, 1, 0x0400_0084);
    load(&mut bytes, 0x800 + 46, 0x908, 2, 0x0400_0100);
    load(&mut bytes, 0xa02, 0xb00, 5, state);
    if newer {
        load(&mut bytes, 0xa10, 0xb04, 1, 0x202);
    }
    let extra = if newer { 3 } else { 2 };
    put32(&mut bytes, 0x1000, u32::from(channels) + extra);
    put32(&mut bytes, 0x1004, address(0x1100));
    put32(&mut bytes, 0x1008, address(0x1120));
    if newer {
        put32(&mut bytes, 0x100c, address(0x1180));
    }
    handler(&mut bytes, 0x1100, 0x1320);
    put32(&mut bytes, 0x110c, u32::from(channels));
    put32(&mut bytes, 0x1110, address(0x1300));
    handler(&mut bytes, 0x1120, 0x1200);
    put16(&mut bytes, 0x1200, channels);
    let notes = if newer { 12 } else { 8 };
    for (offset, target) in [(notes, 0x1400), (notes + 4, 0x1450), (notes + 8, 0x1500)] {
        put32(&mut bytes, 0x1200 + offset, address(target));
    }
    put32(&mut bytes, 0x1450, address(0x1460));
    for index in 0..usize::from(channels) {
        let at = 0x1140 + index * 32;
        handler(&mut bytes, at, 0x1380 + index * 16);
        put32(&mut bytes, 0x1300 + index * 4, address(at));
        put32(
            &mut bytes,
            0x1004 + (index + extra as usize) * 4,
            address(at),
        );
    }
    let title = b"\"Fixture\" Test";
    bytes[0x1380 - title.len()..0x1380].copy_from_slice(title);
    bytes
}

fn scanned(bytes: &[u8]) -> Vec<GaxNativeSong> {
    let cancel = AtomicBool::new(false);
    let mut songs = Vec::new();
    scan(
        bytes,
        &mut songs,
        &mut Budget {
            cancel: &cancel,
            remaining: 1_000_000,
        },
        8,
        super::super::MAX_RETAINED_BYTES,
    )
    .unwrap();
    songs
}

#[test]
fn both_header_layouts_bind_one_original_driver_and_revalidate_selection() {
    for (newer, channels) in [(false, 1), (false, 2), (true, 2)] {
        let bytes = fixture(newer, channels);
        let songs = scanned(&bytes);
        assert_eq!(songs.len(), 1);
        let song = &songs[0];
        assert_eq!(song.channels, channels);
        assert!(song.title.contains("Fixture"));
        assert!(song.native.new.is_none());
        assert_eq!(song.native.sample_rate, MIXER_REQUEST_HZ);
        assert_eq!(song.native.layout, GaxNativeLayout::V1_99);
        let prepared = super::super::prepare_rom(&bytes, song, &AtomicBool::new(false)).unwrap();
        assert_eq!(&prepared[4..bytes.len()], &bytes[4..]);
        assert!(
            prepared[bytes.len()..]
                .windows(4)
                .any(|word| word == address(0x1000).to_le_bytes())
        );
        let mut altered = song.clone();
        altered.native.sample_rate += 1;
        assert!(super::super::prepare_rom(&bytes, &altered, &AtomicBool::new(false)).is_err());
    }
}

#[test]
fn unrelated_state_and_inconsistent_channel_links_are_rejected() {
    let original = fixture(false, 2);
    for (at, value) in [
        (0x900, 0x0300_7000),
        (0x1300, address(0x1160)),
        (0x1000, 3),
        (0x1208, 0),
    ] {
        let mut bytes = original.clone();
        put32(&mut bytes, at, value);
        assert!(scanned(&bytes).is_empty());
    }
    for end in [0, 1, 63, 0x300, 0x802, 0x1380, 0x1504] {
        assert!(scanned(&original[..end]).is_empty());
    }
}

#[test]
fn inventory_work_cancellation_and_workspace_limits_are_explicit() {
    let bytes = fixture(false, 2);
    for (remaining, cancelled, candidates, limit, expected) in [
        (0, false, 8, usize::MAX, ScanStop::WorkLimit),
        (1_000_000, true, 8, usize::MAX, ScanStop::Cancelled),
        (1_000_000, false, 0, usize::MAX, ScanStop::CandidateLimit),
        (1_000_000, false, 8, 1, ScanStop::InventoryLimit),
    ] {
        let cancel = AtomicBool::new(cancelled);
        assert_eq!(
            scan(
                &bytes,
                &mut Vec::new(),
                &mut Budget {
                    cancel: &cancel,
                    remaining
                },
                candidates,
                limit
            ),
            Err(expected)
        );
    }
    let mut profile = scanned(&bytes).remove(0).native;
    for state in [0x0300_1684, 0x0300_3a58, 0x0300_73f0, 0x0203_fde8] {
        profile.work_ram = state;
        let (start, length) = workspace(&profile).unwrap();
        assert!(start >= 0x0300_00a0 && start + length <= 0x0300_7d00);
        assert!(!(start..start + length).contains(&state));
    }
    profile.ram_copies.push(super::super::GaxRamCopy {
        source: RomSpan::new(0, 0x7000),
        destination: 0x0300_0000,
    });
    assert!(workspace(&profile).is_none());
}
