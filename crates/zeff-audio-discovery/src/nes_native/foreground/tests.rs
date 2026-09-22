use super::*;

fn scan_fixture(
    bytes: &[u8],
    work: u64,
    capacity: usize,
) -> (Vec<NesNativeSong>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut songs = Vec::new();
    let result = scan(bytes, &mut songs, &mut budget, capacity);
    (songs, result)
}

#[test]
fn foreground_patch_preserves_original_startup_interrupts_and_mapped_bytes() {
    let bytes = fixture_rom();
    assert_eq!(zeff_firmware::sha256_hex(&bytes), fixture::SOURCE_HASH);
    let (songs, result) = scan_fixture(&bytes, 10_000, 16);
    result.unwrap();
    assert_eq!(
        songs.iter().map(|song| song.raw_index).collect::<Vec<_>>(),
        [1, 2]
    );
    for song in &songs {
        assert_eq!(song.native.tables.canonical_cpu_address, 0xa377);
        assert_eq!(song.native.tables.byte_len, 0xa9ed - 0xa377);
        let selection = crate::catalog::SongRef::NesNative(song);
        assert_eq!(crate::native_rips::supported_format(selection), None);
        assert!(crate::native_rips::encode(&bytes, selection, &AtomicBool::new(false)).is_err());
        let prepared = prepare(&bytes, song, &AtomicBool::new(false)).unwrap();
        assert_eq!(prepared.mapper, 0);
        assert_eq!(prepared.timing, NesNativeTiming::Ntsc);
        assert_eq!(prepared.ready_address, READY);
        assert_eq!(prepared.ack_address, ACK);
        assert_eq!((prepared.wait_start, prepared.wait_end), (0xfd8a, 0xfd91));
        let patch = song.native.bootstrap.effective_offset as usize;
        assert_eq!(&prepared.bytes[..patch], &bytes[..patch]);
        assert_eq!(&prepared.bytes[patch + 19..], &bytes[patch + 19..]);
        assert_eq!(
            &prepared.bytes[patch + 12..patch + 16],
            &[0xa9, song.raw_index, 0x85, 0xd0]
        );
        for span in &song.mapped_spans {
            let start = span.effective_offset as usize;
            let end = start + span.byte_len as usize;
            assert_eq!(&prepared.bytes[start..end], &bytes[start..end]);
        }
    }
}

#[test]
fn foreground_source_and_selection_revalidation_reject_changes() {
    let bytes = fixture_rom();
    let song = scan_fixture(&bytes, 10_000, 16).0.remove(0);
    for offset in [
        4,
        6,
        9,
        0x1c0a,
        0x2419,
        0x7d53,
        0x7d95,
        0x800a,
        bytes.len() - 1,
    ] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(scan_fixture(&changed, 10_000, 16).0.is_empty());
        assert!(prepare(&changed, &song, &AtomicBool::new(false)).is_err());
    }
    for mutate in [
        |song: &mut NesNativeSong| song.raw_index = 17,
        |song: &mut NesNativeSong| song.index = u16::MAX,
        |song: &mut NesNativeSong| song.native.timing = NesNativeTiming::Pal,
        |song: &mut NesNativeSong| song.native.bootstrap.byte_len += 1,
        |song: &mut NesNativeSong| song.channels[0].sequence.byte_len += 1,
        |song: &mut NesNativeSong| {
            song.mapped_spans.pop();
        },
    ] {
        let mut changed = song.clone();
        mutate(&mut changed);
        assert!(prepare(&bytes, &changed, &AtomicBool::new(false)).is_err());
    }
    assert!(prepare(&bytes, &song, &AtomicBool::new(true)).is_err());
    assert!(
        scan_fixture(&bytes[..bytes.len() - 1], 10_000, 16)
            .0
            .is_empty()
    );
    assert_eq!(
        scan_fixture(&bytes, 0, 16),
        (Vec::new(), Err(ScanStop::WorkLimit))
    );
    let (partial, result) = scan_fixture(&bytes, 10_000, 1);
    assert_eq!(partial.len(), 1);
    assert_eq!(result, Err(ScanStop::CandidateLimit));
}

#[test]
fn foreground_asset_spans_require_source_identity_and_exact_mapping() {
    let bytes = fixture_rom();
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
            canonical_cpu_address: Some(0xc000),
            ..span
        }
    ));
    media.sha256 = None;
    assert!(!source_span_matches(&media, span));
}
