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
fn exact_source_identities_expose_only_caller_proven_register_presets() {
    for (bytes, mapper, profile) in [
        (fixture_rom(), 0, fixture::PROFILE_NROM),
        (fixture_rom_cnrom(), 3, fixture::PROFILE_CNROM),
    ] {
        let (songs, result) = scan_fixture(&bytes, 16, 10_000);
        result.unwrap();
        assert_eq!(
            songs.iter().map(|song| song.raw_index).collect::<Vec<_>>(),
            RAW
        );
        for song in songs {
            assert_eq!(song.profile, profile);
            assert_eq!(song.native.mapper, mapper);
            assert!(song.channels.is_empty());
            assert_eq!(song.header, song.table_entry);
            assert_eq!(song.table_entry.byte_len, 4);
            assert_eq!(song.native.init.canonical_cpu_address, u32::from(INIT));
            assert_eq!(song.native.tick.canonical_cpu_address, u32::from(TICK));
            assert_eq!(song.native.tables.canonical_cpu_address, u32::from(TABLE));
            assert!(
                song.warnings
                    .iter()
                    .any(|warning| warning.contains("not sequenced"))
            );
        }
    }
}

#[test]
fn prepared_preset_keeps_source_driver_and_table_bytes() {
    for bytes in [fixture_rom(), fixture_rom_cnrom()] {
        let song = scan_fixture(&bytes, 16, 10_000).0.remove(0);
        let prepared = prepare(&bytes, &song, &AtomicBool::new(false)).unwrap();
        assert_eq!(prepared.mapper, song.native.mapper);
        assert_eq!(prepared.timing, NesNativeTiming::Ntsc);
        assert_eq!(prepared.ready_address, READY);
        assert_eq!(prepared.ack_address, ACK);
        assert_eq!(prepared.wait_end - prepared.wait_start, 7);
        assert_eq!(&prepared.bytes[..16], &bytes[..16]);
        for span in &song.mapped_spans {
            let start = span.effective_offset as usize;
            let end = start + span.byte_len as usize;
            assert_eq!(&prepared.bytes[start..end], &bytes[start..end]);
        }
        assert_eq!(
            &prepared.bytes[song.table_entry.effective_offset as usize..][..4],
            &bytes[song.table_entry.effective_offset as usize..][..4]
        );
    }
}

#[test]
fn revalidation_rejects_source_header_metadata_and_unproven_raw_values() {
    let bytes = fixture_rom();
    let song = scan_fixture(&bytes, 16, 10_000).0.remove(0);
    for offset in [
        4,
        6,
        9,
        16,
        0x5956,
        0x69cb,
        0x69ff,
        0x7800,
        0x800a,
        bytes.len() - 1,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(scan_fixture(&changed, 16, 10_000).0.is_empty());
        assert!(prepare(&changed, &song, &AtomicBool::new(false)).is_err());
    }
    for mutate in [
        |song: &mut NesNativeSong| song.raw_index = 4,
        |song: &mut NesNativeSong| song.raw_index = 7,
        |song: &mut NesNativeSong| song.index = u16::MAX,
        |song: &mut NesNativeSong| song.native.mapper = 3,
        |song: &mut NesNativeSong| song.native.timing = NesNativeTiming::Pal,
        |song: &mut NesNativeSong| song.native.tick.byte_len += 1,
        |song: &mut NesNativeSong| song.table_entry.byte_len += 1,
        |song: &mut NesNativeSong| {
            song.mapped_spans.pop();
        },
    ] {
        let mut changed = song.clone();
        mutate(&mut changed);
        assert!(prepare(&bytes, &changed, &AtomicBool::new(false)).is_err());
    }
    assert!(prepare(&bytes, &song, &AtomicBool::new(true)).is_err());
}

#[test]
fn source_spans_are_bound_to_each_exact_source_identity() {
    for bytes in [fixture_rom(), fixture_rom_cnrom()] {
        let mut media = MediaIdentity {
            system: "nes",
            byte_len: bytes.len() as u64,
            sha256: Some(zeff_firmware::sha256_hex(&bytes)),
        };
        let span = SourceSpan {
            effective_offset: 16,
            byte_len: 0x8000,
            canonical_cpu_address: Some(0x8000),
        };
        assert!(source_span_matches(&media, span));
        assert!(!source_span_matches(
            &media,
            SourceSpan {
                byte_len: 0x8001,
                ..span
            }
        ));
        assert!(!source_span_matches(
            &media,
            SourceSpan {
                canonical_cpu_address: Some(0x8001),
                ..span
            }
        ));
        media.sha256 = None;
        assert!(!source_span_matches(&media, span));
    }
}

#[test]
fn work_candidate_and_cancellation_limits_leave_no_partial_admission() {
    let bytes = fixture_rom();
    assert_eq!(scan_fixture(&bytes, 16, 0).1, Err(ScanStop::WorkLimit));
    let (partial, result) = scan_fixture(&bytes, 1, 10_000);
    assert_eq!(partial.len(), 1);
    assert_eq!(result, Err(ScanStop::CandidateLimit));
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 10_000,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 16),
        Err(ScanStop::Cancelled)
    );
}
