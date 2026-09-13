use std::sync::atomic::AtomicBool;

use super::*;

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 0x3000];
    put32(&mut bytes, 0, 0xea00_002e);
    bytes[0xb2] = 0x96;
    for (at, patterns, state_delta) in [
        (0x100, signatures::INIT, 0x48),
        (0x300, signatures::SELECT, 12),
        (0x500, signatures::START, 0),
        (0x700, signatures::UPDATE, 50),
        (0x900, signatures::IRQ, 12),
    ] {
        for (i, &value) in patterns[0].value.iter().enumerate() {
            put16(&mut bytes, at + i * 2, value);
        }
        let state_at = at + state_delta;
        let literal = at + 0x80;
        let register = half(&bytes, state_at).unwrap() & 0x700;
        put16(
            &mut bytes,
            state_at,
            0x4800 | register | ((literal - ((state_at + 4) & !3)) / 4) as u16,
        );
        put32(&mut bytes, literal, 0x0200_1000);
    }
    let root = 0x2000;
    for (at, value) in [
        (0, 0x20),
        (24, 0x40),
        (28, 0x900),
        (0x20, 0x24),
        (0x40, 1),
        (0x44, 0x80),
        (0x900, 0x904),
        (0x904, 4),
        (0x908, u32::MAX),
    ] {
        put32(&mut bytes, root + at, value);
    }
    put16(&mut bytes, root + 0x90c, 22050);
    put16(&mut bytes, root + 0x90e, 60);
    bytes[root + 0x914..root + 0x918].copy_from_slice(&[0, 127, 0, 128]);
    let anchor = root + 0x80 + 1040;
    for (at, value) in [
        (0, 12),
        (4, 0x50),
        (12, 0x60),
        (0x50, 0x80),
        (0x68, 32),
        (0x6c, u32::MAX),
    ] {
        put32(&mut bytes, anchor + at, value);
    }
    put16(&mut bytes, anchor + 8, 125);
    bytes[anchor + 0x80..anchor + 0x86].copy_from_slice(&[0, 0, 100, 60, 16, 0]);
    put32(&mut bytes, anchor + 0x86, u32::MAX);
    bytes
}

#[test]
fn native_witnesses_and_song_graph_support_revalidated_bootstrap() {
    let bytes = fixture();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut songs = Vec::new();
    assert_eq!(scan(&bytes, &mut songs, &mut budget, 8), Ok(()));
    assert_eq!(songs.len(), 1);
    let song = &songs[0];
    assert_eq!(
        (song.channels, song.patterns, song.notes, song.samples),
        (1, 1, 1, 1)
    );
    let prepared = prepare_rom(&bytes, song, &cancel).unwrap();
    assert!(prepared.wait_loop.effective_offset as usize >= bytes.len());
    assert_eq!(
        &prepared.bytes[0x100..0x108],
        &[8, 0xb4, 1, 0x4b, 0x18, 0x47, 0xc0, 0x46]
    );
    assert_eq!(&prepared.bytes[0x2000..0x2918], &bytes[0x2000..0x2918]);
    let mut changed = song.clone();
    changed.index = 1;
    assert!(prepare_rom(&bytes, &changed, &cancel).is_err());
    let mut changed = bytes.clone();
    put32(&mut changed, 0x380, 0x0200_2000);
    assert!(prepare_rom(&changed, song, &cancel).is_err());
}

#[test]
fn wave_samples_retain_both_native_bank_sizes_and_revalidate() {
    let cancel = AtomicBool::new(false);
    for length in [16, 32] {
        let mut bytes = fixture();
        put32(&mut bytes, 0x2904, length);
        bytes[0x290f] = 1;
        let mut budget = Budget {
            cancel: &cancel,
            remaining: MAX_VALIDATION_WORK,
        };
        let mut songs = Vec::new();
        assert_eq!(scan(&bytes, &mut songs, &mut budget, 8), Ok(()));
        assert_eq!(songs.len(), 1);
        assert!(
            songs[0]
                .mapped_spans
                .contains(&RomSpan::new(0x2904, 16 + length as usize))
        );
        assert!(prepare_rom(&bytes, &songs[0], &cancel).is_ok());
        bytes[0x290f] = 2;
        assert!(prepare_rom(&bytes, &songs[0], &cancel).is_err());
    }
}

#[test]
fn sample_kinds_require_valid_keys_sizes_and_complete_payloads() {
    let cancel = AtomicBool::new(false);
    for (kind, key, length, truncate) in [
        (1, 60, 0, false),
        (1, 60, 4, false),
        (1, 60, 17, false),
        (1, 60, 64, false),
        (2, 60, 16, false),
        (255, 60, 32, false),
        (0, 128, 16, false),
        (1, 128, 16, false),
        (1, 60, 16, true),
        (1, 60, 32, true),
    ] {
        let mut bytes = fixture();
        put32(&mut bytes, 0x2904, length);
        bytes[0x290e] = key;
        bytes[0x290f] = kind;
        if truncate {
            bytes.truncate(0x2914 + length as usize - 1);
        }
        let mut budget = Budget {
            cancel: &cancel,
            remaining: MAX_VALIDATION_WORK,
        };
        let mut songs = Vec::new();
        assert_eq!(scan(&bytes, &mut songs, &mut budget, 8), Ok(()));
        assert!(
            songs.is_empty(),
            "kind {kind}, key {key}, length {length}, truncated {truncate}"
        );
    }
}

#[test]
fn out_of_group_events_and_bank_ranges_are_rejected() {
    let cancel = AtomicBool::new(false);
    for (at, value) in [(0x2044, u32::MAX), (0x24e0, 0x1000), (0x2904, 0x1000000)] {
        let mut bytes = fixture();
        put32(&mut bytes, at, value);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: MAX_VALIDATION_WORK,
        };
        let mut songs = Vec::new();
        let _ = scan(&bytes, &mut songs, &mut budget, 8);
        assert!(songs.is_empty(), "invalid field at {at:x}");
    }
}

#[test]
fn embedded_image_boundaries_require_a_real_startup_after_the_header() {
    let cancel = AtomicBool::new(false);
    let mut bytes = fixture();
    let header = bytes[..0xc0].to_vec();
    bytes[0x1000..0x10c0].copy_from_slice(&header);
    let checksum = bytes[0x10a0..0x10bd]
        .iter()
        .fold(0x19u8, |sum, value| sum.wrapping_add(*value));
    bytes[0x10bd] = checksum.wrapping_neg();
    for (index, value) in [
        0xe3a0_0012,
        0xe129_f000,
        0xe59f_d010,
        0xe3a0_001f,
        0xe129_f000,
        0xe59f_d008,
        0xeaff_fffe,
        0,
        0x0300_7fa0,
        0x0300_7f00,
    ]
    .into_iter()
    .enumerate()
    {
        put32(&mut bytes, 0x10c0 + index * 4, value);
    }
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let inventory = driver::recognize(&bytes, &mut budget).unwrap();
    assert_eq!(inventory.image_start(0x100), 0);
    assert_eq!(inventory.image_start(0x1500), 0x1000);
    put32(&mut bytes, 0x10c0, 0xe92d_40f0);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    assert_eq!(
        driver::recognize(&bytes, &mut budget)
            .unwrap()
            .image_start(0x1500),
        0
    );
}

#[test]
fn empty_sources_and_cancellation_are_bounded() {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1024,
    };
    let mut songs = Vec::new();
    assert_eq!(scan(&[], &mut songs, &mut budget, 8), Ok(()));
    assert_eq!(scan(&[0; 256], &mut songs, &mut budget, 8), Ok(()));
    assert!(songs.is_empty());
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert_eq!(
        scan(&[0; 256], &mut songs, &mut budget, 8),
        Err(ScanStop::Cancelled)
    );
}

#[test]
fn retained_limit_counts_vector_capacity_and_preserves_existing_songs() {
    let bytes = fixture();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut expected = Vec::new();
    assert_eq!(scan(&bytes, &mut expected, &mut budget, 8), Ok(()));
    let limit = retained_owned_bytes(&expected);
    for (available, count) in [(limit - 1, 0), (limit, 1)] {
        let mut songs = Vec::new();
        let mut budget = Budget {
            cancel: &cancel,
            remaining: MAX_VALIDATION_WORK,
        };
        let status = scan_with_retained_limit(&bytes, &mut songs, &mut budget, 8, available);
        assert_eq!(songs.len(), count);
        assert!(retained_owned_bytes(&songs) <= available);
        assert_eq!(
            status,
            if count == 0 {
                Err(ScanStop::InventoryLimit)
            } else {
                Ok(())
            }
        );
    }
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    assert_eq!(
        scan_with_retained_limit(&bytes, &mut expected, &mut budget, 8, limit - 1),
        Err(ScanStop::InventoryLimit),
    );
    assert_eq!(expected.len(), 1);
    assert_eq!(budget.remaining, MAX_VALIDATION_WORK);
}

#[test]
fn full_candidate_inventory_does_not_scan_driver_or_root_bytes() {
    let bytes = fixture();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(budget.remaining, MAX_VALIDATION_WORK);
}
