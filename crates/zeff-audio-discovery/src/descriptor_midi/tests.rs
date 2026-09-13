use super::{fixture::*, *};

fn songs(bytes: &[u8]) -> (Vec<DescriptorMidiSong>, Result<(), ScanStop>) {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut songs = Vec::new();
    let result = scan(bytes, &mut songs, &mut budget, 16);
    (songs, result)
}

#[test]
fn sparse_descriptors_bind_native_players_and_assets() {
    let bytes = rom();
    let (songs, result) = songs(&bytes);
    assert_eq!(result, Ok(()));
    assert_eq!(songs.iter().map(|s| s.index).collect::<Vec<_>>(), [0, 2]);
    assert_eq!(songs[0].root, RomSpan::new(ROOT, 24));
    assert_eq!(
        (songs[0].tracks, songs[0].channels, songs[0].notes),
        (2, 1, 1)
    );
    assert_eq!((songs[0].instruments, songs[0].samples), (1, 1));
    assert_eq!((songs[1].instruments, songs[1].samples), (1, 1));
    assert!(songs[0].mapped_spans.contains(&RomSpan::new(0x6600, 16)));
    assert!(songs[1].mapped_spans.contains(&RomSpan::new(0x6680, 16)));
}

#[test]
fn raw_midi_and_bootstrap_revalidate_the_selected_source() {
    let bytes = rom();
    let (songs, _) = songs(&bytes);
    let cancel = AtomicBool::new(false);
    assert_eq!(
        midi_bytes(&bytes, &songs[0], &cancel).unwrap(),
        midi_file(0, 100)
    );
    let prepared = prepare_rom(&bytes, &songs[0], &cancel).unwrap();
    assert_eq!(&prepared.bytes[..HANDOFF], &bytes[..HANDOFF]);
    assert_eq!(
        &prepared.bytes[HANDOFF + 12..bytes.len()],
        &bytes[HANDOFF + 12..]
    );
    assert!(prepared.wait_loop.effective_offset >= bytes.len() as u32);
    let mut changed = bytes.clone();
    put32(&mut changed, DESCRIPTOR + 16, 2);
    assert!(prepare_rom(&changed, &songs[0], &cancel).is_err());
    assert!(midi_bytes(&changed, &songs[0], &cancel).is_err());
}

#[test]
fn standalone_smf_and_unbound_driver_do_not_admit_songs() {
    assert!(songs(&midi_file(0, 100)).0.is_empty());
    let mut bytes = rom();
    put16(&mut bytes, SELECTOR + 0x798, 0);
    assert!(songs(&bytes).0.is_empty());
}

#[test]
fn invalid_midi_and_sample_bounds_reject_only_affected_selectors() {
    let mut bytes = rom();
    put32(&mut bytes, SAMPLE + 20, 0x09ff_fffc);
    let (retained, result) = songs(&bytes);
    assert_eq!(result, Err(ScanStop::ValidationLimit));
    assert_eq!(retained.iter().map(|s| s.index).collect::<Vec<_>>(), [2]);
    let mut bytes = rom();
    bytes[MIDI + 14 + 4..MIDI + 14 + 8].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(songs(&bytes).0.len(), 1);
}

#[test]
fn quiet_notes_are_retained_with_their_original_velocity() {
    let mut bytes = rom();
    let smf = midi_file(0, 1);
    bytes[MIDI..MIDI + smf.len()].copy_from_slice(&smf);
    let (songs, result) = songs(&bytes);
    assert_eq!(result, Ok(()));
    assert!(
        songs[0]
            .warnings
            .iter()
            .any(|w| w.contains("minimum velocity"))
    );
    assert_eq!(
        midi_bytes(&bytes, &songs[0], &AtomicBool::new(false)).unwrap(),
        smf
    );
}

#[test]
fn candidate_work_and_cancellation_limits_remain_authoritative() {
    let bytes = rom();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut retained = Vec::new();
    assert_eq!(
        scan(&bytes, &mut retained, &mut budget, 1),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(retained.len(), 1);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 0,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 1),
        Err(ScanStop::WorkLimit)
    );
    let cancel = AtomicBool::new(true);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    assert_eq!(
        scan(&bytes, &mut Vec::new(), &mut budget, 1),
        Err(ScanStop::Cancelled)
    );
}

fn pcm_instrument(bytes: &mut [u8], at: usize) {
    bytes[at] = b'A';
    put32(bytes, at + 4, 0x0800_0000 + SAMPLE as u32);
}

fn selected_midi(bytes: &mut [u8], track: Vec<u8>) {
    let midi = midi_tracks(&[track]);
    bytes[MIDI..MIDI + midi.len()].copy_from_slice(&midi);
}

#[test]
fn key_tables_validate_signed_prefix_entries_and_mapped_children() {
    for mapped in [false, true] {
        let mut bytes = rom();
        pcm_instrument(&mut bytes, 0x64a0);
        put32(
            &mut bytes,
            INSTRUMENT,
            (61 << 8) | u32::from(if mapped { b'S' } else { b'R' }),
        );
        if mapped {
            put32(&mut bytes, INSTRUMENT + 4, 0x0800_63d1);
            bytes[0x63d0] = 3;
            put32(&mut bytes, INSTRUMENT + 8, 0x0800_63e0);
        } else {
            put32(&mut bytes, INSTRUMENT + 4, 0x0800_63f0);
        }
        put32(&mut bytes, 0x63ec, 0x0800_64a0);
        let (retained, result) = songs(&bytes);
        assert_eq!(result, Ok(()));
        assert_eq!(retained.len(), 2);
        assert!(retained[0].mapped_spans.contains(&RomSpan::new(0x63ec, 4)));
        if mapped {
            assert!(retained[0].mapped_spans.contains(&RomSpan::new(0x63d0, 1)));
        }
        put32(&mut bytes, 0x63ec, 0x09ff_fffc);
        assert_eq!(
            songs(&bytes).0.iter().map(|s| s.index).collect::<Vec<_>>(),
            [2]
        );
    }
}

#[test]
fn native_loop_programs_close_over_replayed_notes() {
    let mut bytes = rom();
    pcm_instrument(&mut bytes, 0x64a0);
    put32(&mut bytes, 0x6440, (60 << 8) | u32::from(b'R'));
    put32(&mut bytes, 0x6444, 0x0800_63c0);
    put32(&mut bytes, 0x63c0, 0x0800_64a0);
    put32(&mut bytes, 0x63c4, 0x0800_64a0);
    selected_midi(
        &mut bytes,
        vec![
            0, 0xc0, 0, 24, 0xff, 6, 1, b'[', 0, 0x90, 60, 100, 24, 0xc0, 1, 0, 0x90, 61, 100, 48,
            0xff, 6, 1, b']', 0, 0xff, 47, 0,
        ],
    );
    assert_eq!(songs(&bytes).0.len(), 2);
    // Program 1 remains selected when the native loop replays its first key 60.
    put32(&mut bytes, 0x63c0, 0x09ff_fffc);
    assert!(songs(&bytes).0.is_empty());
}

#[test]
fn loop_closure_keeps_programs_before_the_intro_separate() {
    let mut bytes = rom();
    pcm_instrument(&mut bytes, 0x64a0);
    for (instrument, table, first) in [(INSTRUMENT, 0x63c0, 60), (0x6440, 0x63d0, 61)] {
        put32(&mut bytes, instrument, (first << 8) | u32::from(b'R'));
        put32(&mut bytes, instrument + 4, 0x0800_0000 + table as u32);
        put32(&mut bytes, table, 0x0800_64a0);
    }
    put32(&mut bytes, 0x63c4, 0x09ff_fffc);
    put32(&mut bytes, 0x63cc, 0x09ff_fffc);
    selected_midi(
        &mut bytes,
        vec![
            0, 0xc0, 0, 0, 0x90, 60, 100, 24, 0xc0, 1, 96, 0xff, 6, 1, b'[', 0, 0x90, 61, 100, 120,
            0xff, 6, 1, b']', 0, 0xff, 47, 0,
        ],
    );
    let (retained, _) = songs(&bytes);
    assert_eq!(retained.iter().map(|s| s.index).collect::<Vec<_>>(), [0]);
    assert_eq!(retained[0].instruments, 1);
}

#[test]
fn malformed_timing_and_event_payloads_do_not_admit_selectors() {
    for track in [
        vec![0, 0x90, 60, 100, 0x81, 0x80, 0x80, 0x80, 0, 0xff, 47, 0],
        vec![0, 0x90, 60, 100, 0xff, 0xff, 0xff, 0x7f, 0xff, 47, 0],
        vec![0, 0x90, 60, 100, 0, 0xff, 47, 1, 0],
        vec![0, 0x90, 60, 100, 0, 0xff, 81, 3, 0, 0, 0, 0, 0xff, 47, 0],
        vec![0, 0x90, 60, 100, 0, 0xf0, 1, 0, 0, 0xff, 47, 0],
        vec![0, 0x90, 60, 100, 24, 0xff, 6, 1, b']', 0, 0xff, 47, 0],
    ] {
        let mut bytes = rom();
        selected_midi(&mut bytes, track);
        let (retained, result) = songs(&bytes);
        assert_eq!(result, Err(ScanStop::ValidationLimit));
        assert_eq!(retained.iter().map(|s| s.index).collect::<Vec<_>>(), [2]);
    }
}

#[test]
fn driver_state_aliases_and_unplayed_tracks_are_not_retained() {
    let mut bytes = rom();
    put32(&mut bytes, 0x6048 + 8, 0x0300_1000);
    assert!(songs(&bytes).0.is_empty());
    let mut bytes = rom();
    put16(&mut bytes, 0x6048, 1 << 5);
    let (retained, result) = songs(&bytes);
    assert_eq!(result, Err(ScanStop::ValidationLimit));
    assert_eq!(retained.iter().map(|s| s.index).collect::<Vec<_>>(), [2]);
}

#[test]
fn candidate_limit_counts_only_valid_native_selections() {
    let mut bytes = rom();
    put32(&mut bytes, SAMPLE + 20, 0x09ff_fffc);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: MAX_VALIDATION_WORK,
    };
    let mut retained = Vec::new();
    assert_eq!(
        scan(&bytes, &mut retained, &mut budget, 1),
        Err(ScanStop::ValidationLimit)
    );
    assert_eq!(retained.iter().map(|s| s.index).collect::<Vec<_>>(), [2]);
}
