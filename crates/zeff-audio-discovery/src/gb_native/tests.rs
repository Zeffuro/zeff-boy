use super::*;

fn songs(
    bytes: &[u8],
    limit: usize,
    work: u64,
    cancelled: bool,
) -> (Vec<GbNativeSong>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(cancelled);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut songs = Vec::new();
    let result = scan(bytes, &mut songs, &mut budget, limit);
    (songs, result)
}

#[test]
fn packed_groups_exclude_continuations_effects_and_unqualified_music() {
    let bytes = fixture_rom();
    let (found, result) = songs(&bytes, 32, 10_000, false);
    result.unwrap();
    assert_eq!(
        found.iter().map(|song| song.raw_index).collect::<Vec<_>>(),
        [
            0xba, 0xc3, 0xca, 0xd0, 0xdb, 0xe1, 0xe5, 0xe8, 0xeb, 0xf3, 0xf7
        ]
    );
    assert_eq!(found[0].channels.len(), 3);
    assert_eq!(found[1].channels.len(), 4);
    assert_eq!(found[7].playback_frames, 136);
    assert_eq!(found[7].playback_clocks, 9_550_892);
    assert_eq!(found[7].loop_start_frame, None);
    assert_eq!(found[0].loop_start_frame, Some(1081));
    assert_eq!(found[0].playback_frames, 6841);
}

#[test]
fn expanded_groups_append_selectors_without_changing_the_original_fixture() {
    let original = fixture_rom();
    assert_eq!(
        zeff_firmware::sha256_hex(&original),
        "77117c737c249f1a793cc7acb194dd69d2f25e310ccb7e0166b5264946da51c8"
    );
    let bytes = expanded_fixture_rom();
    let (found, result) = songs(&bytes, 32, 10_000, false);
    result.unwrap();
    assert_eq!(found.len(), 20);
    let original_songs = songs(&original, 32, 10_000, false).0;
    for (new, old) in found.iter().zip(original_songs) {
        assert_eq!((new.index, new.raw_index), (old.index, old.raw_index));
        assert_eq!(new.channels, old.channels);
    }
    assert_eq!(
        found[11..]
            .iter()
            .map(|song| song.raw_index)
            .collect::<Vec<_>>(),
        [0xbd, 0xc0, 0xc7, 0xcd, 0xd4, 0xd8, 0xde, 0xef, 0xfb]
    );
    for song in &found[11..] {
        assert_eq!(song.loop_start_frame, Some(1));
        assert_eq!(song.playback_frames, 257);
        assert_eq!(song.playback_clocks, 18_047_940);
        let prepared = prepare_rom(&bytes, song, &AtomicBool::new(false)).unwrap();
        for span in &song.mapped_spans {
            let start = span.effective_offset as usize;
            let end = start + span.byte_len as usize;
            assert_eq!(bytes[start..end], prepared.bytes[start..end]);
        }
    }
}

#[test]
fn appended_groups_obey_the_same_candidate_and_source_limits() {
    let bytes = expanded_fixture_rom();
    let (found, result) = songs(&bytes, 12, 10_000, false);
    assert_eq!(result, Err(ScanStop::CandidateLimit));
    assert_eq!(found.len(), 12);
    assert_eq!((found[11].index, found[11].raw_index), (11, 0xbd));
    let mut changed = found[11].clone();
    changed.playback_clocks += 1;
    assert!(prepare_rom(&bytes, &changed, &AtomicBool::new(false)).is_err());
    assert!(prepare_rom(&fixture_rom(), &found[11], &AtomicBool::new(false)).is_err());
}

#[test]
fn native_image_retains_original_startup_and_all_mapped_bytes() {
    let bytes = fixture_rom();
    for song in songs(&bytes, 32, 10_000, false).0 {
        let prepared = prepare_rom(&bytes, &song, &AtomicBool::new(false)).unwrap();
        assert_eq!(prepared.bytes.len(), bytes.len());
        assert_eq!(prepared.timing, GbNativeTiming::Dmg);
        assert_eq!(prepared.playback_frames, song.playback_frames);
        assert_eq!(prepared.playback_clocks, song.playback_clocks);
        assert_eq!(prepared.ready_address, 0xfffc);
        assert_eq!(prepared.ack_address, 0xfffb);
        assert_eq!((prepared.wait_start, prepared.wait_end), (0x3fbb, 0x3fc1));
        for (offset, (before, after)) in bytes.iter().zip(&prepared.bytes).enumerate() {
            if before != after {
                assert!(
                    (0x40..0x43).contains(&offset)
                        || (0x1fd3..0x1fd6).contains(&offset)
                        || (0x3fb0..0x4000).contains(&offset)
                );
            }
        }
        for span in song.mapped_spans {
            let offset = span.effective_offset as usize;
            let end = offset + span.byte_len as usize;
            assert_eq!(bytes[offset..end], prepared.bytes[offset..end]);
        }
    }
}

#[test]
fn source_identity_and_song_metadata_are_revalidated() {
    let bytes = fixture_rom();
    let original = songs(&bytes, 32, 10_000, false).0.remove(0);
    let mut changed = original.clone();
    changed.raw_index = 0xbb;
    assert!(prepare_rom(&bytes, &changed, &AtomicBool::new(false)).is_err());
    changed = original.clone();
    changed.bank = 8;
    assert!(prepare_rom(&bytes, &changed, &AtomicBool::new(false)).is_err());
    changed = original.clone();
    changed.channels[0].sequence.byte_len += 1;
    assert!(prepare_rom(&bytes, &changed, &AtomicBool::new(false)).is_err());
    let mut changed = original.clone();
    changed.playback_frames += 1;
    assert!(prepare_rom(&bytes, &changed, &AtomicBool::new(false)).is_err());
    changed = original.clone();
    changed.playback_clocks += 1;
    assert!(prepare_rom(&bytes, &changed, &AtomicBool::new(false)).is_err());
    changed = original.clone();
    changed.native.bootstrap = original.native.driver;
    assert!(prepare_rom(&bytes, &changed, &AtomicBool::new(false)).is_err());
    let mut altered_rom = bytes.clone();
    altered_rom[0x147] = 0x1b;
    assert!(songs(&altered_rom, 32, 10_000, false).0.is_empty());
    altered_rom = bytes.clone();
    altered_rom[0x3fb0] = 1;
    assert!(songs(&altered_rom, 32, 10_000, false).0.is_empty());
    for len in [0, 0x14f, 0x8000, 0xfffff] {
        assert!(songs(&bytes[..len], 32, 10_000, false).0.is_empty());
    }
}

#[test]
fn budget_cancellation_and_candidate_limits_remain_bounded() {
    let bytes = fixture_rom();
    assert_eq!(songs(&bytes, 32, 10_000, true).1, Err(ScanStop::Cancelled));
    assert_eq!(songs(&bytes, 32, 0, false).1, Err(ScanStop::WorkLimit));
    assert_eq!(songs(&bytes, 32, 4_000, false).1, Err(ScanStop::WorkLimit));
    let (found, result) = songs(&bytes, 1, 10_000, false);
    assert_eq!(result, Err(ScanStop::CandidateLimit));
    assert_eq!(found.len(), 1);
    assert_eq!(
        songs(&bytes, 0, 10_000, false).1,
        Err(ScanStop::CandidateLimit)
    );
}

#[test]
fn source_spans_require_exact_identity_and_a_single_correctly_mapped_bank() {
    let bytes = fixture_rom();
    let media = MediaIdentity {
        system: "gb",
        byte_len: bytes.len() as u64,
        sha256: Some(zeff_firmware::sha256_hex(&bytes)),
    };
    let valid = SourceSpan {
        effective_offset: 0x8000,
        byte_len: 0x4000,
        canonical_cpu_address: Some(0x4000),
    };
    assert!(source_span_matches(&media, valid));
    for span in [
        SourceSpan {
            byte_len: 0x4001,
            ..valid
        },
        SourceSpan {
            byte_len: 0,
            ..valid
        },
        SourceSpan {
            effective_offset: 0x8001,
            ..valid
        },
        SourceSpan {
            effective_offset: 0x10_0000,
            ..valid
        },
        SourceSpan {
            canonical_cpu_address: Some(0x8000),
            ..valid
        },
    ] {
        assert!(!source_span_matches(&media, span));
    }
    assert!(!source_span_matches(
        &MediaIdentity {
            system: "gba",
            ..media
        },
        valid
    ));
}
