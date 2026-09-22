use std::sync::atomic::AtomicBool;

use crate::{Budget, ScanStop};

use super::*;

fn songs(bytes: &[u8], work: u64) -> (Vec<GbNativeSong>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut found = Vec::new();
    let result = scan(bytes, &mut found, &mut budget, 8);
    (found, result)
}

#[test]
fn fixture_profiles_scan_banked_four_channel_selectors() {
    for bytes in [fixture::fixture_rom(), fixture::fixture_rom_small()] {
        let (found, result) = songs(&bytes, 20_000);
        result.unwrap();
        assert_eq!(found.len(), 4);
        assert_eq!(
            found.iter().map(|song| song.raw_index).collect::<Vec<_>>(),
            [1, 2, 0x4c, 0x4d]
        );
        for song in found {
            assert_eq!(song.native.timing, GbNativeTiming::CgbDouble);
            assert_eq!(song.native.cartridge_type, 0x1b);
            assert!((1..=4).contains(&song.channels.len()));
            assert!(matches!(song.header.byte_len, 7 | 10 | 13));
            assert_eq!(song.table_entry.byte_len, 3);
            assert!((DRIVER_BANK..=LAST_AUDIO_BANK).contains(&song.bank));
            assert_eq!(song.loop_start_frame, None);
            assert_eq!(song.playback_clocks, CGB_DOUBLE_CLOCKS * PLAYBACK_SECONDS);
            assert!(song.mapped_spans.iter().any(|span| {
                span.effective_offset == u32::from(DRIVER_BANK) * 0x4000 && span.byte_len == 0x4000
            }));
        }
    }
}

#[test]
fn effect_headers_are_variable_and_follow_music_indices() {
    let found = songs(&fixture_rom(), 20_000).0;
    assert_eq!(found[0].title, "Native music selector 01");
    assert_eq!(found[1].title, "Native music selector 02");
    assert_eq!(found[2].index, 2);
    assert_eq!(found[2].header.byte_len, 7);
    assert_eq!(found[2].channels.len(), 2);
    assert_eq!(found[3].index, 3);
    assert_eq!(found[3].header.byte_len, 10);
    assert_eq!(found[3].channels.len(), 3);
    assert_eq!(found[2].bank, DRIVER_BANK + 1);
    assert_eq!(found[3].bank, DRIVER_BANK + 2);
    for (song, numbers) in [(&found[2], &[3, 6][..]), (&found[3], &[4, 8, 6][..])] {
        assert_eq!(
            song.channels
                .iter()
                .map(|channel| channel.number)
                .collect::<Vec<_>>(),
            numbers
        );
    }
}

#[test]
fn bootstrap_uses_isolated_handshake_and_only_allowed_patch_ranges() {
    let bytes = fixture::fixture_rom();
    let song = &songs(&bytes, 20_000).0[1];
    let prepared = prepare(&bytes, song, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        (prepared.ready_address, prepared.ack_address),
        (0xff81, 0xff80)
    );
    assert_eq!((prepared.ready_value, prepared.ack_value), (0xa5, 0x5a));
    assert!(prepared.wait_start < prepared.wait_end);
    for (offset, (&before, &after)) in bytes.iter().zip(&prepared.bytes).enumerate() {
        if before != after {
            assert!((0x100..0x103).contains(&offset) || (0x150..0x250).contains(&offset));
        }
    }
    for span in &song.mapped_spans {
        let start = span.effective_offset as usize;
        let end = start + span.byte_len as usize;
        assert_eq!(prepared.bytes[start..end], bytes[start..end]);
    }
}

#[test]
fn changed_source_and_cancelled_scan_do_not_admit_songs() {
    let mut bytes = fixture::fixture_rom();
    let table = usize::from(DRIVER_BANK) * 0x4000 + 0x4503 - 0x4000;
    bytes[table + 3] = 2;
    let (found, result) = songs(&bytes, 20_000);
    assert_eq!(result, Ok(()));
    assert!(found.is_empty());

    let bytes = fixture::fixture_rom();
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 20_000,
    };
    let mut found = Vec::new();
    assert_eq!(
        scan(&bytes, &mut found, &mut budget, 8),
        Err(ScanStop::Cancelled)
    );
    assert!(found.is_empty());
}

#[test]
fn stale_selection_and_source_spans_are_rejected() {
    let bytes = fixture_rom();
    let song = songs(&bytes, 20_000).0.remove(0);
    for mutate in [
        |song: &mut GbNativeSong| song.raw_index = 0,
        |song: &mut GbNativeSong| song.index += 1,
        |song: &mut GbNativeSong| song.native.timing = GbNativeTiming::Dmg,
        |song: &mut GbNativeSong| song.mapped_spans[0].effective_offset += 1,
        |song: &mut GbNativeSong| song.channels[0].sequence.effective_offset += 1,
    ] {
        let mut changed = song.clone();
        mutate(&mut changed);
        assert!(prepare(&bytes, &changed, &AtomicBool::new(false)).is_err());
    }
    let mut media = MediaIdentity {
        system: "gbc",
        byte_len: bytes.len() as u64,
        sha256: Some(zeff_firmware::sha256_hex(&bytes)),
    };
    let span = SourceSpan {
        effective_offset: song.header.effective_offset,
        byte_len: song.header.byte_len,
        canonical_cpu_address: Some(song.header.canonical_cpu_address),
    };
    assert!(source_span_matches(&media, span));
    media.byte_len -= 1;
    assert!(!source_span_matches(&media, span));
}

#[test]
fn stream_parser_rejects_immediate_cycles_and_wrong_header_banks() {
    let mut bytes = fixture_rom();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 20_000,
    };
    let stream = usize::from(DRIVER_BANK) * 0x4000 + 0x560;
    bytes[stream..stream + 3].copy_from_slice(&[0xfe, 0x60, 0x45]);
    assert_eq!(
        parse_stream(&bytes, DRIVER_BANK, 0, 0x4560, &mut budget),
        Err(ScanStop::ValidationLimit)
    );
    let bytes = fixture_rom();
    let selected = inspect(&bytes, &fixture::PROFILE, 1, 2, &mut budget).unwrap();
    assert_eq!(
        selected.header.effective_offset / 0x4000,
        u32::from(DRIVER_BANK)
    );
    assert_eq!(selected.bank, DRIVER_BANK + 1);
    assert!(selected.channels.iter().all(|channel| channel.sequence.effective_offset / 0x4000 == u32::from(DRIVER_BANK + 1)));
}

#[test]
fn effect_header_rejects_bad_slot_duplicate_and_missing_terminator() {
    let bytes = fixture_rom();
    let header = usize::from(DRIVER_BANK) * 0x4000 + 0x600;
    for replacement in [
        &[0x14, 0x80, 0x46][..],
        &[0x12, 0x80, 0x46, 0x22, 0x90, 0x46][..],
        &[
            0x12, 0x80, 0x46, 0x23, 0x90, 0x46, 0x35, 0xa0, 0x46, 0x47, 0xb0, 0x46, 0,
        ][..],
    ] {
        let mut changed = bytes.clone();
        changed[header..header + replacement.len()].copy_from_slice(replacement);
        let cancel = AtomicBool::new(false);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: 20_000,
        };
        assert_eq!(
            inspect(&changed, &fixture::PROFILE, 2, 0x4c, &mut budget),
            Err(ScanStop::ValidationLimit)
        );
    }
}
