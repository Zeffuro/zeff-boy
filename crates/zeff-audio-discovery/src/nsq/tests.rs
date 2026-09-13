use super::{fixture::*, *};

fn discover(bytes: &[u8], limit: usize) -> (Vec<NsqSong>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut songs = Vec::new();
    let result = scan(bytes, &mut songs, &mut budget, limit);
    (songs, result)
}

#[test]
fn five_layouts_bind_original_selectors_and_only_patch_bank_entry() {
    for layout in 0..signatures::LAYOUTS.len() {
        let bytes = build(layout);
        let (songs, result) = discover(&bytes, 8);
        assert_eq!(result, Ok(()), "layout {layout}");
        assert_eq!(songs.len(), 2, "layout {layout}");
        assert_eq!((songs[0].index, songs[1].index), (108, 115));
        assert_eq!(songs[0].root, RomSpan::new(TABLE, 24));
        assert_eq!(songs[1].header, RomSpan::new(TABLE + 8, 8));
        assert_eq!(songs[0].sequence, songs[1].sequence);
        assert_eq!(songs[0].notes, 1);
        assert_eq!(songs[0].duration_frames, 25);
        assert_eq!(songs[0].samples, 1);
        let ready = prepare_rom(&bytes, &songs[1], &AtomicBool::new(false)).unwrap();
        assert_eq!(&ready.bytes[..BANK_ENTRY], &bytes[..BANK_ENTRY]);
        assert_eq!(
            &ready.bytes[BANK_ENTRY + 12..bytes.len()],
            &bytes[BANK_ENTRY + 12..]
        );
        assert_eq!(word(&ready.bytes, BANK_ENTRY + 8), Some(0x0800_8000));
        let wait = ready.wait_loop.effective_offset as usize;
        assert_eq!(word(&ready.bytes, wait), Some(0xe590_1004));
        assert_eq!(word(&ready.bytes, wait + 4), Some(0xe351_0001));
    }
}

#[test]
fn signatures_shared_irq_state_caller_and_bound_paths_are_required() {
    let layout = &signatures::LAYOUTS[0];
    let mix = BANK_ENTRY + layout.offsets[4] + layout.mix_state;
    let slot = literal_slot(mix, layout.patterns[4][layout.mix_state / 2]);
    let release_slot = literal_slot(BANK_ENTRY + layout.release_load, layout.release_copy[0]);
    for (at, value) in [
        (BANK_ENTRY, 0),
        (CALL, 0),
        (slot, 0x0300_2000),
        (0x404, 0),
        (FILESYSTEM + 8, 0),
        (release_slot, 0),
        (RELEASE_PREFIX, 0),
    ] {
        let mut bytes = build(0);
        put(&mut bytes, at, value);
        assert!(discover(&bytes, 8).0.is_empty(), "at {at:x}");
    }
}

#[test]
fn malformed_events_banks_and_sample_extents_are_rejected() {
    for (at, value) in [
        (SEQUENCE + 8, 0x007f_0000),
        (INSTRUMENTS + 4, 999),
        (FILESYSTEM + 8 + 2 * 16 + 12, u32::MAX),
        (TABLE + 8, 108),
    ] {
        let mut bytes = build(0);
        put(&mut bytes, at, value);
        assert!(discover(&bytes, 8).0.is_empty(), "at {at:x}");
    }
}

#[test]
fn repeated_callers_keep_one_witness_per_argument_pair() {
    let mut bytes = build(0);
    let expected = discover(&bytes, 8).0;
    bytes.resize(0xc000, 0);
    for at in (0x8000..0xc000).step_by(16) {
        put(&mut bytes, at, 0x4902_4801);
        let relative = (BANK_ENTRY as i32 - at as i32 - 8) as u32;
        let high = 0xf000 | ((relative >> 12) & 2047);
        let low = 0xf800 | ((relative >> 1) & 2047);
        put(&mut bytes, at + 4, high | low << 16);
        put(&mut bytes, at + 8, 0x0800_0000 + PREFIX as u32);
        put(&mut bytes, at + 12, 0x0800_0000 + PATH as u32);
    }
    let (songs, result) = discover(&bytes, 8);
    assert_eq!(result, Ok(()));
    assert_eq!(songs, expected);
}

#[test]
fn candidate_limit_cancellation_stale_metadata_and_placement_are_explicit() {
    let bytes = build(0);
    let (mut songs, result) = discover(&bytes, 1);
    assert_eq!(songs.len(), 1);
    assert_eq!(result, Err(ScanStop::CandidateLimit));
    assert_eq!(discover(&[0; 64], 0).1, Ok(()));
    for (cancelled, work, expected) in [
        (true, 100_000, ScanStop::Cancelled),
        (false, 1, ScanStop::WorkLimit),
    ] {
        let cancel = AtomicBool::new(cancelled);
        let mut budget = Budget {
            cancel: &cancel,
            remaining: work,
        };
        assert_eq!(scan(&bytes, &mut Vec::new(), &mut budget, 8), Err(expected));
    }
    songs[0].index = 107;
    assert!(prepare_rom(&bytes, &songs[0], &AtomicBool::new(false)).is_err());
    assert!(bootstrap::placement(&vec![0; MAX_ROM_BYTES]).is_none());
}
