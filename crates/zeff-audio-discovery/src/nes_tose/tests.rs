use super::*;

pub fn synthetic_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x28010];
    bytes[..16].copy_from_slice(b"NES\x1a\x08\x04\x20\x40\0\0\0\0\0\0\0\0");
    for &(bank, start, end, _) in &WITNESSES {
        let at = span(bank, start, usize::from(end - start));
        bytes[at.effective_offset as usize..at.effective_offset as usize + at.byte_len as usize]
            .fill(0xea);
    }
    put(&mut bytes, 0, 0x98c0, &[0xa9, 0, 0x8d, 0x15, 0x40, 0x60]);
    put(&mut bytes, 3, 0x98a8, &[0x8d, 0xb0, 0x06, 0x60]);
    put(
        &mut bytes,
        3,
        0x8f68,
        &[
            0xa9, 1, 0x8d, 0x15, 0x40, 0xa9, 0xbf, 0x8d, 0, 0x40, 0xa9, 0, 0x8d, 1, 0x40, 0xa9,
            0x80, 0x8d, 2, 0x40, 0xa9, 8, 0x8d, 3, 0x40, 0xee, 0xfa, 3, 0x60,
        ],
    );
    for channel in 0..4_u8 {
        let pointer = 0x8200 + u16::from(channel) * 32;
        let mut entry = vec![40 + channel * 20, channel];
        entry.extend(pointer.to_le_bytes());
        put(&mut bytes, 3, 0x8040 + u16::from(channel) * 4, &entry);
        put(&mut bytes, 3, pointer, &[0, 2, 31, 0, 3, 4, 0xff]);
    }
    bytes
}

fn put(bytes: &mut [u8], bank: u8, address: u16, data: &[u8]) {
    let at = span(bank, address, data.len()).effective_offset as usize;
    bytes[at..at + data.len()].copy_from_slice(data);
}

pub(super) fn fixture_witness(bank: u8, start: u16, hash: &str) -> bool {
    static HASHES: std::sync::OnceLock<Vec<(u8, u16, String)>> = std::sync::OnceLock::new();
    HASHES
        .get_or_init(|| {
            let bytes = synthetic_rom();
            WITNESSES
                .iter()
                .map(|&(bank, start, end, _)| {
                    let at = span(bank, start, usize::from(end - start));
                    (
                        bank,
                        start,
                        zeff_firmware::sha256_hex(
                            &bytes[at.effective_offset as usize
                                ..at.effective_offset as usize + at.byte_len as usize],
                        ),
                    )
                })
                .collect()
        })
        .iter()
        .any(|(b, s, h)| *b == bank && *s == start && h == hash)
}

pub fn synthetic_fcg_rom() -> Vec<u8> {
    let mut bytes = vec![0; 0x80010];
    bytes[..16].copy_from_slice(b"NES\x1a\x10\x20\0\x10\0\0\0\0\0\0\0\0");
    for &(start, end, _) in &fcg::WITNESSES {
        let at = fcg::span(start, usize::from(end - start));
        bytes[at.effective_offset as usize..at.effective_offset as usize + at.byte_len as usize]
            .fill(0xea);
    }
    bytes[0x3ca7a..0x3ca80].copy_from_slice(&[0xa9, 0, 0x8d, 0x15, 0x40, 0x60]);
    bytes[0x50b..0x50f].copy_from_slice(&[0x8c, 0xb0, 6, 0x60]);
    let source = synthetic_rom();
    bytes[0x17..0x17 + 29].copy_from_slice(&source[0x18f78..0x18f78 + 29]);
    for channel in 0..4 {
        let entry = 0x55e + channel * 4;
        bytes[entry] = 84 + channel as u8 * 21;
        bytes[entry + 1] = channel as u8;
        let pointer = 0x8900 + channel as u16 * 32;
        bytes[entry + 2..entry + 4].copy_from_slice(&pointer.to_le_bytes());
        let at = usize::from(pointer - 0x8000) + 16;
        bytes[at..at + 7].copy_from_slice(&[0, 2, 31, 0, 3, 4, 0xff]);
    }
    bytes
}

pub(super) fn fcg_fixture_witness(start: u16, hash: &str) -> bool {
    static HASHES: std::sync::OnceLock<Vec<(u16, String)>> = std::sync::OnceLock::new();
    HASHES
        .get_or_init(|| {
            let bytes = synthetic_fcg_rom();
            fcg::WITNESSES
                .iter()
                .map(|&(start, end, _)| {
                    let at = fcg::span(start, usize::from(end - start));
                    (
                        start,
                        zeff_firmware::sha256_hex(
                            &bytes[at.effective_offset as usize
                                ..at.effective_offset as usize + at.byte_len as usize],
                        ),
                    )
                })
                .collect()
        })
        .iter()
        .any(|(s, h)| *s == start && h == hash)
}

#[cfg(test)]
fn inventory(bytes: &[u8]) -> Vec<NesToseSong> {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 2_000_000,
    };
    let mut songs = Vec::new();
    scan(bytes, &mut songs, &mut budget, 100).unwrap();
    songs
}

#[test]
fn source_and_selection_are_revalidated() {
    let bytes = synthetic_rom();
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 1);
    assert!(songs[0].tracks.iter().all(|track| track.note_count == 1));
    validate_song(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
    let mut unrelated = bytes.clone();
    unrelated[0x23000] ^= 1;
    assert_eq!(inventory(&unrelated), songs);
    for (bank, start, end, _) in WITNESSES {
        for address in [start, end - 1] {
            let mut changed = bytes.clone();
            changed[span(bank, address, 1).effective_offset as usize] ^= 1;
            assert!(inventory(&changed).is_empty());
        }
    }
    let mut stale = songs[0].clone();
    stale.tracks[0].note_count += 1;
    assert!(validate_song(&bytes, &stale, &AtomicBool::new(false)).is_err());
    assert!(validate_song(&bytes, &songs[0], &AtomicBool::new(true)).is_err());
}

#[test]
fn invalid_channels_pointers_and_envelope_tables_are_rejected() {
    for (address, value) in [
        (0x8040, 0),
        (0x8045, 0),
        (0x8041, 4),
        (0x8043, 0x80),
        (0x8204, 0xca),
    ] {
        let mut bytes = synthetic_rom();
        put(&mut bytes, 3, address, &[value]);
        assert!(inventory(&bytes).is_empty(), "{address:x}");
    }
    for offset in [4, 6, 7, 8] {
        let mut bytes = synthetic_rom();
        bytes[offset] ^= 1;
        assert!(inventory(&bytes).is_empty());
    }
    assert!(inventory(&synthetic_rom()[..0x20010]).is_empty());
}

#[test]
fn loops_require_yield_and_follow_native_repeat_zero_semantics() {
    let mut bytes = synthetic_rom();
    put(&mut bytes, 3, 0x8204, &[0xfd, 0xfe, 0xbf, 0xfe]);
    assert!(inventory(&bytes).is_empty());
    put(&mut bytes, 3, 0x8204, &[0xfd, 0xfe, 3, 1, 0xb0, 0xfe, 0xff]);
    let songs = inventory(&bytes);
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].tracks[0].note_count, 1);
    put(&mut bytes, 3, 0x8204, &[0xfd, 0xfe, 3, 1, 0xb2, 0xfe, 0xff]);
    assert_eq!(inventory(&bytes)[0].tracks[0].note_count, 3);
    put(&mut bytes, 3, 0x8204, &[0xfd, 0xfe, 3, 1, 0xbf, 0xfe]);
    assert_eq!(inventory(&bytes).len(), 1);
}

#[test]
fn native_preserves_every_mapped_byte_and_vectors_in_each_bank() {
    let bytes = synthetic_rom();
    let song = inventory(&bytes).remove(0);
    let prepared = prepare_rom(&bytes, &song, &AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.mapper, 66);
    for span in song.mapped_spans {
        let at = span.effective_offset as usize;
        assert_eq!(
            &bytes[at..at + span.byte_len as usize],
            &prepared.bytes[at..at + span.byte_len as usize]
        );
    }
    for bank in 0..4 {
        let at = 16 + bank * 0x8000;
        assert_eq!(
            &prepared.bytes[at + 0x7ffc..at + 0x7ffe],
            &0xf800_u16.to_le_bytes()
        );
        assert_eq!(
            &prepared.bytes[at + 0x7800..at + 0x7807],
            &[0x78, 0xd8, 0xa9, 0, 0x8d, 3, 0xf8]
        );
    }
}

#[test]
fn cancellation_and_budget_are_observable() {
    let bytes = synthetic_rom();
    for (cancelled, remaining, expected) in [
        (true, 100, ScanStop::Cancelled),
        (false, 0, ScanStop::WorkLimit),
    ] {
        let cancel = AtomicBool::new(cancelled);
        let mut budget = Budget {
            cancel: &cancel,
            remaining,
        };
        let mut songs = Vec::new();
        assert_eq!(scan(&bytes, &mut songs, &mut budget, 1), Err(expected));
    }
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 2_000_000,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(ScanStop::CandidateLimit)
    );
}

#[test]
fn fcg_dialect_has_distinct_mapping_and_repeat_semantics() {
    let mut bytes = synthetic_fcg_rom();
    let song = inventory(&bytes).remove(0);
    assert_eq!(song.profile, fcg::PROFILE);
    let prepared = prepare_rom(&bytes, &song, &AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.mapper, 16);
    for span in &song.mapped_spans {
        let at = span.effective_offset as usize;
        assert_eq!(
            &bytes[at..at + span.byte_len as usize],
            &prepared.bytes[at..at + span.byte_len as usize]
        );
    }
    bytes[0x914..0x91c].copy_from_slice(&[0xfd, 0, 3, 1, 0xb0, 0, 0xff, 0]);
    assert_eq!(inventory(&bytes)[0].tracks[0].note_count, 2);
    bytes[0x918] = 0xb2;
    assert_eq!(inventory(&bytes)[0].tracks[0].note_count, 3);
    bytes[0x914..0x91e].copy_from_slice(&[0xfd, 0, 3, 1, 0xb2, 3, 0xb2, 0, 0xff, 0]);
    assert_eq!(inventory(&bytes)[0].tracks[0].note_count, 9);
    bytes[0x914..0x918].copy_from_slice(&[0xb0, 2, 0xff, 0]);
    assert!(inventory(&bytes).is_empty());
    bytes = synthetic_fcg_rom();
    bytes[0x914] = 0xca;
    assert!(inventory(&bytes).is_empty());
    bytes = synthetic_fcg_rom();
    bytes[0x17] ^= 1;
    assert!(inventory(&bytes).is_empty());
}

#[test]
fn indirect_page_crossing_dummy_reads_are_mapped() {
    let mut bytes = synthetic_rom();
    put(&mut bytes, 3, 0x8042, &0x82ff_u16.to_le_bytes());
    put(&mut bytes, 3, 0x82ff, &[0, 2, 31, 0, 3, 1, 0xff]);
    let song = inventory(&bytes).remove(0);
    let covered = |song: &NesToseSong, offset: u32| {
        song.mapped_spans.iter().any(|span| {
            offset >= span.effective_offset && offset < span.effective_offset + span.byte_len
        })
    };
    for address in 0x8200..0x8203 {
        assert!(covered(&song, span(3, address, 1).effective_offset));
    }
    bytes = synthetic_fcg_rom();
    bytes[0x560..0x562].copy_from_slice(&0x89fa_u16.to_le_bytes());
    bytes[0xa0a..0xa13].copy_from_slice(&[0, 2, 31, 0, 0xa2, 2, 3, 1, 0xff]);
    let song = inventory(&bytes).remove(0);
    assert!(covered(&song, fcg::span(0x8900, 1).effective_offset));
}
