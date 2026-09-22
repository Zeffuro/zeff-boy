use super::*;

fn word_at(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn fixture() -> Vec<u8> {
    super::fixture::song()
}

fn inspect(bytes: &[u8]) -> Option<SongStructure> {
    inspect_plain_song(bytes, 0x200, ScanLimits::default(), &AtomicBool::new(false)).unwrap()
}

#[test]
fn order_byte_count_aliases_and_loop_ticks_follow_driver_layout() {
    let bytes = fixture();
    let song = inspect(&bytes).unwrap();
    assert_eq!(
        (song.order_count, song.ticks_per_row, song.loop_ticks),
        (2, 1, 128)
    );
    assert_eq!(song.notes.len(), 16);
    assert_eq!(song.instruments.len(), 4);
    assert_eq!(song.spans.len(), 9);
    assert_eq!(song.instruments[0].source, song.instruments[1].source);
    for (index, note) in song.notes.iter().enumerate() {
        assert_eq!(note.tick, [0, 16, 64, 80][index / 4]);
        assert_eq!(note.channel, (index % 4) as u8);
        assert_eq!(note.pitch, [24, 31, 36, 43][index / 4]);
        assert_eq!(note.instrument, 1);
        assert!(note.reload_instrument);
    }
    assert_eq!(song.instruments[2].wave, Some(span(0x320, 16)));
    assert!(
        song.spans
            .windows(2)
            .all(|pair| pair[0].offset + pair[0].byte_len <= pair[1].offset)
    );
}

#[test]
fn tempo_zero_is_256_ticks_and_later_instrument_zero_reuses_state() {
    let mut bytes = fixture();
    bytes[0x200] = 0;
    bytes[0x1031] = 0;
    let song = inspect(&bytes).unwrap();
    assert_eq!(song.ticks_per_row, 256);
    assert_eq!(song.loop_ticks, 32768);
    assert_eq!(song.notes[4].tick, 4096);
    assert_eq!(song.notes[4].instrument, 1);
    assert!(!song.notes[4].reload_instrument);
    bytes[0x1001] = 0;
    assert!(inspect(&bytes).is_none());
}

#[test]
fn effect_parameters_subpatterns_and_noncanonical_notes_are_held() {
    for (at, value) in [
        (0x1001, 0x11),
        (0x1002, 1),
        (0x1000, 0x98),
        (0x303, 1),
        (0x309, 1),
        (0x30d, 1),
        (0x308, 16),
        (0x211, 1),
    ] {
        let mut bytes = fixture();
        bytes[at] = value;
        assert!(inspect(&bytes).is_none(), "at {at:x}");
    }
}

#[test]
fn invalid_mapping_counts_pointers_and_overlaps_are_held() {
    for count in [0, 1, 3, 255] {
        let mut bytes = fixture();
        bytes[0x220] = count;
        assert!(inspect(&bytes).is_none());
    }
    for (at, pointer) in [
        (0x201, 0xffff),
        (0x203, 0x7fff),
        (0x230, 0x7f80),
        (0x20b, 0x1000),
        (0x20d, 0x300),
        (0x213, 0x7ff1),
        (0x232, 0x1001),
    ] {
        let mut bytes = fixture();
        word_at(&mut bytes, at, pointer);
        assert!(inspect(&bytes).is_none(), "at {at:x} -> {pointer:x}");
    }
    for at in [0x147, 0x148, 0x149] {
        let mut bytes = fixture();
        bytes[at] = 1;
        assert!(inspect(&bytes).is_none());
    }
    assert!(inspect(&fixture()[..0x7fff]).is_none());
}

#[test]
fn unused_rest_instruments_are_not_dereferenced_and_silent_channels_are_held() {
    let mut bytes = fixture();
    bytes[0x1004] = 0xf0;
    assert!(inspect(&bytes).is_some());
    for at in [0x1000, 0x1030, 0x1100, 0x1130] {
        bytes[at] = 90;
    }
    assert!(inspect(&bytes).is_none());
}

#[test]
fn limits_and_cancellation_never_return_partial_structures() {
    for (limits, cancel, expected) in [
        (ScanLimits::default(), true, ScanStop::Cancelled),
        (
            ScanLimits {
                max_work: 200,
                ..Default::default()
            },
            false,
            ScanStop::WorkLimit,
        ),
        (
            ScanLimits {
                max_candidates: 0,
                ..Default::default()
            },
            false,
            ScanStop::CandidateLimit,
        ),
        (
            ScanLimits {
                max_work: crate::MAX_SCAN_WORK + 1,
                ..Default::default()
            },
            false,
            ScanStop::InvalidLimits,
        ),
    ] {
        assert_eq!(
            inspect_plain_song(&fixture(), 0x200, limits, &AtomicBool::new(cancel)),
            Err(expected)
        );
    }
}

#[test]
fn bounded_mutations_never_escape_the_source_image() {
    let source = fixture();
    for at in (0x200..0x1140).step_by(7) {
        let mut bytes = source.clone();
        bytes[at] ^= 0xff;
        if let Some(song) = inspect(&bytes) {
            for span in song.spans {
                assert!(span.offset as usize + span.byte_len as usize <= bytes.len());
            }
            assert!(song.notes.iter().all(|note| note.tick < song.loop_ticks));
        }
    }
}
