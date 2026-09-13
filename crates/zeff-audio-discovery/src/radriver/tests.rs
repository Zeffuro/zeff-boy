use super::*;

fn inventory(bytes: &[u8]) -> (Vec<RadriverSong>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut songs = Vec::new();
    let result = scan(bytes, &mut songs, &mut budget, 128);
    (songs, result)
}

#[test]
fn linear_probe_groups_preserve_work_and_cancellation_limits() {
    let bytes = vec![0; 1024];
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 4,
    };
    assert_eq!(driver::recognize(&bytes, &mut budget), Ok((vec![], false)));
    assert_eq!(budget.remaining, 0);
    budget.remaining = 3;
    assert_eq!(
        driver::recognize(&bytes, &mut budget),
        Err(ScanStop::WorkLimit)
    );
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 4,
    };
    assert_eq!(
        driver::recognize(&bytes, &mut budget),
        Err(ScanStop::Cancelled)
    );
}

#[test]
fn a_signature_hit_is_charged_before_profile_validation() {
    let mut bytes = vec![0; 128];
    for (index, &word) in signatures::GLOBAL_SONG.iter().enumerate() {
        fixture::put16(&mut bytes, index * 2, word);
    }
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1,
    };
    assert_eq!(
        driver::recognize(&bytes, &mut budget),
        Err(ScanStop::WorkLimit)
    );
}

#[test]
fn discovers_separate_effect_and_compressed_selectors() {
    let bytes = fixture::build();
    let (songs, result) = inventory(&bytes);
    assert_eq!(result, Ok(()));
    assert_eq!(songs.len(), 4);
    assert_eq!(
        songs
            .iter()
            .map(|song| (song.kind, song.index))
            .collect::<Vec<_>>(),
        [
            (RadriverSongKind::Effect, 0),
            (RadriverSongKind::Effect, 1),
            (RadriverSongKind::CompressedMusic, 0),
            (RadriverSongKind::CompressedMusic, 1),
        ]
    );
    assert_eq!(songs[0].samples[0].data.byte_len, 16);
    assert_eq!(songs[0].samples[0].padding.unwrap().byte_len, 768);
    assert_eq!(songs[3].samples.len(), 2);
    assert_eq!(songs[3].order.unwrap().byte_len, 6);
}

#[test]
fn global_state_bank_has_native_effect_identity_and_unaligned_handoff() {
    let bytes = fixture::global();
    let (songs, result) = inventory(&bytes);
    assert_eq!(result, Ok(()));
    assert_eq!(songs.len(), 2);
    assert_eq!(songs[0].native.layout, RadriverLayout::GlobalState);
    assert_eq!(songs[0].native.handoff.effective_offset, 0x30e);
    assert_eq!(songs[0].native.channels, 2);
    let prepared = prepare_rom(&bytes, &songs[0], &AtomicBool::new(false)).unwrap();
    assert_eq!(half(&prepared.bytes, 0x30e), Some(0x4b01));
    assert_eq!(word(&prepared.bytes, 0x314), Some(0x0800_5000));
}

#[test]
fn invalid_pcm_padding_preserves_other_valid_selectors() {
    let mut bytes = fixture::build();
    bytes[fixture::SAMPLE + 32] = 1;
    let (songs, result) = inventory(&bytes);
    assert_eq!(result, Err(ScanStop::ValidationLimit));
    assert_eq!(songs.len(), 3);
    assert_eq!(songs[0].index, 1);
}

#[test]
fn compressed_orders_reject_null_negative_and_out_of_range_blocks() {
    for (at, value) in [(fixture::TABLE, 0), (0x4100, 0xfffe), (0x4100, 1024)] {
        let mut bytes = fixture::build();
        if at == fixture::TABLE {
            fixture::put32(&mut bytes, at, value);
        } else {
            fixture::put16(&mut bytes, at, value as u16);
        }
        let (songs, result) = inventory(&bytes);
        assert_eq!(result, Err(ScanStop::ValidationLimit));
        assert_eq!(songs.len(), 3);
    }
}

#[test]
fn preparation_revalidates_identity_and_builds_barrier() {
    let bytes = fixture::build();
    let (songs, _) = inventory(&bytes);
    let cancel = AtomicBool::new(false);
    for song in &songs {
        let prepared = prepare_rom(&bytes, song, &cancel).unwrap();
        assert_eq!(prepared.wait_loop.byte_len, 12);
        assert!(prepared.bytes.len() > bytes.len());
        assert!(
            prepared.bytes[bytes.len()..]
                .windows(4)
                .any(|value| value == crate::gba_bootstrap::READY_VALUE.to_le_bytes())
        );
    }
    let mut wrong = songs[0].clone();
    wrong.index = 1;
    assert!(prepare_rom(&bytes, &wrong, &cancel).is_err());
    let mut changed = bytes.clone();
    fixture::put32(&mut changed, fixture::SAMPLE + 4, u32::MAX);
    assert!(prepare_rom(&changed, &songs[0], &cancel).is_err());
}

#[test]
fn full_inventory_precedes_search_and_preserves_existing_songs() {
    let bytes = fixture::build();
    let (mut songs, _) = inventory(&bytes);
    let old = songs.clone();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 0,
    };
    let count = songs.len();
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, count),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(songs, old);
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 0),
        Err(ScanStop::CandidateLimit)
    );
}

#[test]
fn unbound_global_startup_is_partial_without_playable_entries() {
    for (at, value) in [
        (0x27c, 0x0800_4000),
        (0x27c, 0x0800_1001),
        (0x278, 0x0a00_0000),
        (0x270, 0x0300_7f00),
    ] {
        let mut bytes = fixture::global();
        fixture::put32(&mut bytes, at, value);
        let (songs, result) = inventory(&bytes);
        assert_eq!(result, Err(ScanStop::ValidationLimit));
        assert!(songs.is_empty());
    }
}

#[test]
fn candidate_limit_preserves_raw_effect_identity() {
    let bytes = fixture::build();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut songs = Vec::new();
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, 1),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(songs.len(), 1);
    assert_eq!(songs[0].kind, RadriverSongKind::Effect);
    assert_eq!(songs[0].index, 0);
}

#[test]
fn cancellation_and_retained_limit_precede_driver_search() {
    let bytes = fixture::build();
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 0,
    };
    let mut songs = Vec::new();
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, 128),
        Err(ScanStop::Cancelled)
    );
    assert!(songs.is_empty());
    let (mut songs, _) = inventory(&bytes);
    songs[0].title = "x".repeat(MAX_RETAINED_BYTES);
    let count = songs.len();
    assert_eq!(
        scan(&bytes, &mut songs, &mut budget, 128),
        Err(ScanStop::InventoryLimit)
    );
    assert_eq!(songs.len(), count);
    assert_eq!(songs[0].title.len(), MAX_RETAINED_BYTES);
}

#[test]
fn unterminated_compressed_order_retains_prior_effects() {
    let mut bytes = fixture::build();
    bytes.resize(0x10000, 0);
    bytes[0x4100..0x4100 + MAX_ORDERS * 2].fill(0);
    let (songs, result) = inventory(&bytes);
    assert_eq!(result, Err(ScanStop::ValidationLimit));
    assert_eq!(songs.len(), 2);
    assert!(
        songs
            .iter()
            .all(|song| song.kind == RadriverSongKind::Effect)
    );
}

#[test]
fn exact_pcm_loop_tail_is_required_and_revalidated() {
    let mut bytes = fixture::build();
    fixture::put32(&mut bytes, fixture::SAMPLE + 8, 8);
    for index in 0..768 {
        bytes[fixture::SAMPLE + 32 + index] = bytes[fixture::SAMPLE + 24 + index % 8];
    }
    let (songs, result) = inventory(&bytes);
    assert_eq!(result, Ok(()));
    assert_eq!(songs[0].samples[0].loop_start, Some(8));
    assert!(prepare_rom(&bytes, &songs[0], &AtomicBool::new(false)).is_ok());
    bytes[fixture::SAMPLE + 32 + 767] ^= 1;
    assert!(prepare_rom(&bytes, &songs[0], &AtomicBool::new(false)).is_err());
}

#[test]
fn malformed_decoder_and_state_table_reject_native_profile() {
    let bytes = fixture::build();
    let (songs, _) = inventory(&bytes);
    for (at, value) in [(0x2128, 0), (0x2a20, 49), (0x22a0, 0x0a00_0000)] {
        let mut changed = bytes.clone();
        fixture::put32(&mut changed, at, value);
        assert!(inventory(&changed).0.is_empty());
        assert!(prepare_rom(&changed, &songs[2], &AtomicBool::new(false)).is_err());
    }
}

#[test]
fn compressed_blocks_must_cover_a_native_transition_batch() {
    let mut bytes = fixture::build();
    fixture::put32(&mut bytes, 0x4004, 143);
    let (songs, result) = inventory(&bytes);
    assert_eq!(result, Err(ScanStop::ValidationLimit));
    assert_eq!(songs.len(), 2);
    assert!(
        songs
            .iter()
            .all(|song| song.kind == RadriverSongKind::Effect)
    );
}
