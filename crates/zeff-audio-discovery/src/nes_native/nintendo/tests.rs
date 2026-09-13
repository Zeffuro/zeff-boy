use super::*;

fn scan_fixture(
    bytes: &[u8],
    maximum: usize,
    work: u64,
) -> (Vec<NesNativeSong>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut songs = Vec::new();
    let result = scan(bytes, &mut songs, &mut budget, maximum);
    (songs, result)
}

#[test]
fn source_identity_and_native_selection_are_bound_to_the_authored_cartridge() {
    let bytes = fixture_rom();
    assert_eq!(zeff_firmware::sha256_hex(&bytes), fixture::SOURCE_HASH);
    let (songs, result) = scan_fixture(&bytes, 16, 10_000);
    result.unwrap();
    assert_eq!(
        songs.iter().map(|song| song.raw_index).collect::<Vec<_>>(),
        [1, 2]
    );
    for song in songs {
        assert_eq!(song.channels.len(), 1);
        assert_eq!(song.channels[0].sequence.byte_len, 6);
        assert_eq!(song.native.mapper, 1);
        let prepared = prepare(&bytes, &song, &AtomicBool::new(false)).unwrap();
        assert_eq!(prepared.bytes.len(), bytes.len());
        assert_eq!(&prepared.bytes[0x10010..], &bytes[0x10010..]);
        for span in &song.mapped_spans {
            let start = span.effective_offset as usize;
            let end = start + span.byte_len as usize;
            assert_eq!(&prepared.bytes[start..end], &bytes[start..end]);
        }
        for (offset, (old, new)) in bytes.iter().zip(&prepared.bytes).enumerate() {
            if old != new {
                assert!((0xb6..0xb9).contains(&offset) || (0x7f50..0x7fe0).contains(&offset));
            }
        }
    }
}

#[test]
fn revalidation_rejects_wrong_calls_banks_headers_and_selectors() {
    let bytes = fixture_rom();
    let song = scan_fixture(&bytes, 16, 10_000).0.remove(0);
    for mutate in [
        |song: &mut NesNativeSong| song.native.mapper = 0,
        |song: &mut NesNativeSong| song.native.init.canonical_cpu_address += 1,
        |song: &mut NesNativeSong| song.table_entry.byte_len += 1,
        |song: &mut NesNativeSong| song.raw_index = 6,
        |song: &mut NesNativeSong| song.index = u16::MAX,
        |song: &mut NesNativeSong| song.mapped_spans.pop().map(drop).unwrap(),
    ] {
        let mut changed = song.clone();
        mutate(&mut changed);
        assert!(prepare(&bytes, &changed, &AtomicBool::new(false)).is_err());
    }
    for offset in [4, 6, 9, 0xb6, 0x5270, 0x6284, 0x1000c, bytes.len() - 1] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(scan_fixture(&changed, 16, 10_000).0.is_empty());
        assert!(prepare(&changed, &song, &AtomicBool::new(false)).is_err());
    }
    assert!(prepare(&bytes, &song, &AtomicBool::new(true)).is_err());
    assert!(
        scan_fixture(&bytes[..bytes.len() - 1], 16, 10_000)
            .0
            .is_empty()
    );
}

#[test]
fn startup_mapping_distinguishes_reset_bank_from_active_bank() {
    let bytes = fixture_rom();
    let media = MediaIdentity {
        system: "nes",
        byte_len: bytes.len() as u64,
        sha256: Some(zeff_firmware::sha256_hex(&bytes)),
    };
    let active = SourceSpan {
        effective_offset: 16,
        byte_len: 0x8000,
        canonical_cpu_address: Some(0x8000),
    };
    let reset = SourceSpan {
        effective_offset: 0xff10,
        byte_len: 0x30,
        canonical_cpu_address: Some(0xff00),
    };
    assert!(source_span_matches(&media, active));
    assert!(source_span_matches(&media, reset));
    let vectors = SourceSpan {
        effective_offset: 0x1000a,
        byte_len: 6,
        canonical_cpu_address: Some(0xfffa),
    };
    assert!(source_span_matches(&media, vectors));
    assert!(!source_span_matches(
        &media,
        SourceSpan {
            byte_len: 7,
            ..vectors
        }
    ));
    assert!(!source_span_matches(
        &media,
        SourceSpan {
            byte_len: 0x8001,
            ..active
        }
    ));
    assert!(!source_span_matches(
        &media,
        SourceSpan {
            effective_offset: 0x8010,
            ..active
        }
    ));
    assert!(!source_span_matches(
        &media,
        SourceSpan {
            byte_len: 0x31,
            ..reset
        }
    ));
    assert!(!source_span_matches(
        &media,
        SourceSpan {
            canonical_cpu_address: Some(0x7f00),
            ..reset
        }
    ));
    assert!(!source_span_matches(
        &MediaIdentity {
            byte_len: media.byte_len + (1_u64 << 32),
            ..media.clone()
        },
        reset
    ));
    assert!(!source_span_matches(
        &MediaIdentity {
            byte_len: media.byte_len - 1,
            ..media
        },
        reset
    ));
}

#[test]
fn bounded_channel_tables_reject_missing_terminators_and_invalid_targets() {
    let bytes = fixture_rom();
    assert!(channel_table(&bytes, 0xe320, None).is_some());
    assert!(channel_table(&bytes, 0xffff, None).is_none());
    let mut changed = bytes.clone();
    changed[0x6320 + 16..0x6420 + 16].fill(0xe3);
    assert!(channel_table(&changed, 0xe320, None).is_none());
    assert_eq!(
        channel_table(&changed, 0xe320, Some(2)).unwrap().byte_len,
        4
    );
    assert!(channel_table(&changed, 0xe320, Some(0)).is_none());
    assert!(channel_table(&changed, 0xe320, Some(129)).is_none());
    changed[0x6320 + 16..0x6326 + 16].copy_from_slice(&[0x80, 0xe3, 0, 0xff, 0, 0x80]);
    assert!(channel_table(&changed, 0xe320, None).is_none());
}

#[test]
fn candidate_work_and_cancellation_limits_are_respected() {
    let bytes = fixture_rom();
    let (retained, result) = scan_fixture(&bytes, 1, 10_000);
    assert_eq!(retained.len(), 1);
    assert_eq!(result, Err(ScanStop::CandidateLimit));
    assert_eq!(scan_fixture(&bytes, 16, 0).1, Err(ScanStop::WorkLimit));
    let mut budget = Budget {
        cancel: &AtomicBool::new(true),
        remaining: 10_000,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 16),
        Err(ScanStop::Cancelled)
    );
}
