use super::*;

fn songs(
    bytes: &[u8],
    limit: usize,
    work: u64,
    cancel: bool,
) -> (Vec<NesNativeSong>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(cancel);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: work,
    };
    let mut songs = Vec::new();
    let result = scan(bytes, &mut songs, &mut budget, limit);
    (songs, result)
}

#[test]
fn bounded_groups_preserve_mapping_and_only_patch_the_bootstrap() {
    let bytes = fixture_rom();
    let (songs, result) = songs(&bytes, 16, 10_000, false);
    result.unwrap();
    assert_eq!(
        songs.iter().map(|song| song.raw_index).collect::<Vec<_>>(),
        [0x90, 0x93, 0x96]
    );
    for song in songs {
        assert_eq!(song.channels.len(), 3);
        let prepared = prepare_rom(&bytes, &song, &AtomicBool::new(false)).unwrap();
        assert_eq!(prepared.bytes.len(), bytes.len());
        let patch = song.native.bootstrap;
        for (offset, (before, after)) in bytes.iter().zip(&prepared.bytes).enumerate() {
            if before != after {
                assert!(
                    (patch.effective_offset as usize
                        ..(patch.effective_offset + patch.byte_len) as usize)
                        .contains(&offset)
                        || (0x800a..0x8010).contains(&offset)
                );
            }
        }
        for span in &song.mapped_spans {
            let start = span.effective_offset as usize;
            assert_eq!(
                bytes[start..start + span.byte_len as usize],
                prepared.bytes[start..start + span.byte_len as usize]
            );
        }
    }
}

#[test]
fn source_and_selection_revalidation_reject_changed_contracts() {
    let bytes = fixture_rom();
    let mut song = songs(&bytes, 16, 10_000, false).0.remove(0);
    song.raw_index = 0x91;
    assert!(prepare_rom(&bytes, &song, &AtomicBool::new(false)).is_err());
    song.raw_index = 0x90;
    song.native.bootstrap = song.native.driver;
    assert!(prepare_rom(&bytes, &song, &AtomicBool::new(false)).is_err());
    let mut changed = bytes.clone();
    changed[0x700b + 16 * 3] = 4;
    assert!(songs(&changed, 16, 10_000, false).0.is_empty());
    for length in [0, 7, 0x8010, bytes.len() - 1] {
        assert!(songs(&bytes[..length], 16, 10_000, false).0.is_empty());
    }
}

#[test]
fn cancellation_work_and_candidate_limits_stop_before_admission() {
    let bytes = fixture_rom();
    assert_eq!(songs(&bytes, 16, 10_000, true).1, Err(ScanStop::Cancelled));
    assert_eq!(songs(&bytes, 16, 0, false).1, Err(ScanStop::WorkLimit));
    let (retained, result) = songs(&bytes, 1, 10_000, false);
    assert_eq!(result, Err(ScanStop::CandidateLimit));
    assert_eq!(retained.len(), 1);
    assert_eq!(
        songs(&bytes, 0, 10_000, false).1,
        Err(ScanStop::CandidateLimit)
    );
}

#[test]
fn fixed_prg_mapping_checks_the_complete_source_span() {
    let bytes = fixture_rom();
    let media = MediaIdentity {
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
            effective_offset: 15,
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
}

#[test]
fn pc10_retains_its_tail_and_requires_the_matching_length_and_identity() {
    let bytes = fixture_rom_pc10();
    let mut found = songs(&bytes, 16, 10_000, false).0;
    assert_eq!(found.len(), 3);
    let prepared = prepare_rom(&bytes, &found.remove(0), &AtomicBool::new(false)).unwrap();
    assert_eq!(prepared.bytes[0x8010..], bytes[0x8010..]);
    assert!(songs(&bytes[..0x10010], 16, 10_000, false).0.is_empty());
    let media = MediaIdentity {
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
        &MediaIdentity {
            byte_len: 0x10010,
            ..media
        },
        span
    ));
}
