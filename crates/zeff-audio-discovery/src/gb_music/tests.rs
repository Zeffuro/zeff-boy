use super::*;
use crate::test_support::gb_music::synthetic_song;

fn fixture(streams: &[(u8, &[u8])]) -> Vec<u8> {
    let mut bytes = vec![0; 0x8000];
    for (index, &(number, stream)) in streams.iter().enumerate() {
        let address = 0x4100u16 + index as u16 * 0x100;
        let entry = 0x4000 + index * 3;
        bytes[entry] = (number - 1)
            | if index == 0 {
                (streams.len() as u8 - 1) << 6
            } else {
                0
            };
        bytes[entry + 1..entry + 3].copy_from_slice(&address.to_le_bytes());
        bytes[usize::from(address)..usize::from(address) + stream.len()].copy_from_slice(stream);
    }
    bytes
}

fn trace(bytes: &[u8], loops: u8, max_frames: u32) -> sequence::Program {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let song = header(bytes, 1, 0x100, 1, 0x4000).unwrap();
    sequence::interpret(bytes, song, loops, max_frames, &mut budget).unwrap()
}

#[test]
fn profiles_use_exact_source_identity_and_define_catalog_bounds() {
    assert_eq!(PROFILES.len(), 6);
    for (index, profile) in PROFILES.iter().enumerate() {
        assert!(
            PROFILES[..index]
                .iter()
                .all(|earlier| earlier.sha256 != profile.sha256)
        );
        assert_eq!(profile.name, format!("gb-banked-driver-v1-rev{index}"));
        assert_eq!(profile.table, 0xe906e);
        assert!(matches!(profile.song_count, 93 | 103));
        assert_eq!(profile.sha256.len(), 64);
        assert!(std::ptr::eq(
            profile,
            profile_by_sha(profile.sha256).unwrap()
        ));
    }
    assert!(profile_by_sha("0".repeat(64).as_str()).is_none());
}

#[test]
fn fractional_frames_and_wave_pitch_follow_hardware_units() {
    let sequence = [0xda, 0, 120, 0xd8, 12, 0xf0, 0xd4, 0x13, 0x23, 0xff];
    let wave = [0xd8, 12, 0x10, 0xd4, 0x13, 0x23, 0xff];
    let bytes = fixture(&[(1, &sequence), (3, &wave)]);
    let program = trace(&bytes, 1, MAX_FRAMES);
    assert!(program.song.midi_exportable);
    assert_eq!(
        program
            .song
            .channels
            .iter()
            .map(|c| c.end_frame)
            .collect::<Vec<_>>(),
        [45, 45]
    );
    let notes = program
        .notes
        .iter()
        .map(|note| (note.channel, note.frame, note.duration, note.frequency))
        .collect::<Vec<_>>();
    assert_eq!(
        notes,
        [
            (1, 0, 22, 1797),
            (3, 0, 22, 1797),
            (1, 22, 23, 1811),
            (3, 22, 23, 1811)
        ]
    );
    assert_eq!(program.song.channels[0].termination, GbTermination::Fine);
}

#[test]
fn wave_volume_masks_driver_bits_when_sharing_pulse_commands() {
    let bytes = fixture(&[
        (
            1,
            &[
                0xd8, 1, 0xa7, 0xd4, 0x10, 0xdc, 0xd7, 0x10, 0xdc, 0xe7, 0x10, 0xdc, 0xf7, 0x10,
                0xdc, 0x87, 0x10, 0xff,
            ],
        ),
        (3, &[0xd8, 1, 0x26, 0x00, 0xfc, 0, 0x41]),
    ]);
    let program = trace(&bytes, 1, MAX_FRAMES);
    assert!(program.song.midi_exportable);
    assert!(program.song.warnings.is_empty());
    for (channel, expected, end) in [
        (1, [84, 110, 118, 127, 67], 5),
        (3, [64, 127, 64, 32, 0], 6),
    ] {
        assert_eq!(
            program
                .notes
                .iter()
                .filter(|note| note.channel == channel)
                .map(|note| note.volume)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            program
                .song
                .channels
                .iter()
                .find(|c| c.number == channel)
                .unwrap()
                .end_frame,
            end
        );
    }
}

#[test]
fn global_tempo_changes_do_not_rescale_an_already_running_note() {
    let bytes = fixture(&[
        (
            1,
            &[
                0xda, 0, 128, 0xd8, 1, 0xf0, 0xd4, 0x13, 0xda, 1, 0, 0x13, 0xff,
            ],
        ),
        (2, &[0xd8, 1, 0xf0, 0xd4, 0x17, 0x17, 0xff]),
    ]);
    let program = trace(&bytes, 1, MAX_FRAMES);
    assert_eq!(program.song.channels[0].end_frame, 6);
    assert_eq!(program.song.channels[1].end_frame, 12);
    assert_eq!(
        program
            .notes
            .iter()
            .map(|note| (note.channel, note.frame, note.duration))
            .collect::<Vec<_>>(),
        [(1, 0, 2), (2, 0, 4), (1, 2, 4), (2, 4, 8)]
    );
}

#[test]
fn finite_loops_and_single_return_calls_expand_exactly() {
    let mut bytes = fixture(&[(
        1,
        &[
            0xd8, 1, 0xf0, 0xd4, 0x10, 0xfd, 3, 4, 0x41, 0xfe, 0x20, 0x41, 0xff,
        ],
    )]);
    bytes[0x4120..0x4122].copy_from_slice(&[0x20, 0xff]);
    let program = trace(&bytes, 1, MAX_FRAMES);
    assert_eq!(program.notes.len(), 4);
    assert_eq!(program.song.channels[0].end_frame, 4);
    assert_eq!(program.song.channels[0].termination, GbTermination::Fine);
    assert!(
        program
            .song
            .mapped_spans
            .iter()
            .any(|range| range.offset == 0x4120 && range.byte_len == 2)
    );
    assert!(program.song.mapped_spans.contains(&span(0x100, 3)));
    assert!(program.song.mapped_spans.contains(&span(0x4000, 3)));
}

#[test]
fn infinite_loops_preserve_intro_and_requested_passes() {
    let bytes = fixture(&[(
        1,
        &[0xd8, 1, 0xf0, 0xd4, 0x10, 0x20, 0x30, 0xfd, 0, 5, 0x41],
    )]);
    let first = trace(&bytes, 1, MAX_FRAMES);
    let three = trace(&bytes, 3, MAX_FRAMES);
    assert_eq!(first.notes.len(), 3);
    assert_eq!(first.song.channels[0].loop_start_frame, Some(1));
    assert_eq!(first.song.channels[0].termination, GbTermination::Loop);
    assert_eq!(three.notes.len(), 7);
    assert_eq!(three.song.channels[0].end_frame, 7);
}

#[test]
fn overlapping_finite_loops_reveal_the_single_counter_cycle() {
    let bytes = fixture(&[(
        1,
        &[
            0xd8, 1, 0xf0, 0xd4, 0x10, 0xfd, 4, 4, 0x41, 0x20, 0xfd, 4, 4, 0x41, 0xfd, 0, 4, 0x41,
        ],
    )]);
    let one = trace(&bytes, 1, MAX_FRAMES);
    let two = trace(&bytes, 2, MAX_FRAMES);
    assert!(one.song.midi_exportable);
    assert_eq!(one.song.channels[0].termination, GbTermination::Loop);
    assert_eq!(one.notes.len(), 5);
    assert_eq!(one.song.channels[0].loop_start_frame, Some(1));
    assert_eq!(two.notes.len(), 9);
    assert!(
        one.song
            .mapped_spans
            .iter()
            .all(|range| range.offset + range.byte_len <= 0x410e)
    );
}

#[test]
fn shorter_channel_loops_fill_the_common_song_duration() {
    let bytes = fixture(&[
        (1, &[0xd8, 1, 0xf0, 0xd4, 0x10, 0xfd, 0, 4, 0x41]),
        (3, &[0xd8, 1, 0x10, 0xd4, 0x13, 0xfd, 0, 4, 0x42]),
    ]);
    let one = trace(&bytes, 1, MAX_FRAMES);
    let two = trace(&bytes, 2, MAX_FRAMES);
    assert_eq!(
        one.song
            .channels
            .iter()
            .map(|c| (c.note_count, c.end_frame))
            .collect::<Vec<_>>(),
        [(4, 4), (1, 4)]
    );
    assert_eq!(
        two.song
            .channels
            .iter()
            .map(|c| (c.note_count, c.end_frame))
            .collect::<Vec<_>>(),
        [(8, 8), (2, 8)]
    );
    assert!(one.notes.iter().all(|note| note.frame + note.duration <= 4));
}

#[test]
fn forward_jump_reaches_shared_body_and_noise_arity_is_stateful() {
    let mut bytes = fixture(&[(4, &[0xe3, 0, 0xd8, 2, 0xfc, 0x20, 0x41])]);
    bytes[0x4120..0x4125].copy_from_slice(&[0x10, 0xe3, 0x00, 0xff, 0]);
    let program = trace(&bytes, 1, MAX_FRAMES);
    assert_eq!(program.notes.len(), 1);
    assert_eq!(program.notes[0].drum, Some((0, 1)));
    assert_eq!(program.song.channels[0].end_frame, 4);
    assert_eq!(program.song.channels[0].termination, GbTermination::Fine);
}

#[test]
fn unsupported_pitch_slides_runtime_commands_and_nested_calls_are_gated() {
    for opcode in [
        0xdf, 0xe0, 0xe2, 0xe7, 0xe8, 0xea, 0xeb, 0xee, 0xf0, 0xf9, 0xfa, 0xfb,
    ] {
        let bytes = fixture(&[(1, &[opcode, 0, 0, 0, 0xff])]);
        let program = trace(&bytes, 1, MAX_FRAMES);
        assert!(!program.song.midi_exportable, "opcode {opcode:02x}");
        assert_eq!(program.song.warnings.len(), 1);
        assert_eq!(program.song.warnings[0].offset, 0x4100);
    }
    let mut bytes = fixture(&[(1, &[0xfe, 0x20, 0x41, 0xff])]);
    bytes[0x4120..0x4124].copy_from_slice(&[0xfe, 0x30, 0x41, 0xff]);
    assert!(!trace(&bytes, 1, MAX_FRAMES).song.midi_exportable);
}

#[test]
fn malformed_pointers_headers_and_no_time_cycles_are_bounded() {
    let mut bytes = fixture(&[(1, &[0xfc, 0, 0x41])]);
    assert!(!trace(&bytes, 1, MAX_FRAMES).song.midi_exportable);
    bytes[0x4000] = 4;
    assert!(header(&bytes, 1, 0x100, 1, 0x4000).is_err());
    assert!(pointer(&bytes, 1, 0x3fff, 1).is_err());
    assert!(pointer(&bytes, 1, 0x7fff, 2).is_err());
    assert!(pointer(&bytes, 2, 0x4000, 1).is_err());
    bytes[0x4000..0x4003].copy_from_slice(&[0, 0xff, 0x7f]);
    bytes[0x7fff] = 0xff;
    assert!(trace(&bytes, 1, MAX_FRAMES).song.midi_exportable);
}

#[test]
fn cancellation_work_budget_and_duration_are_independent_limits() {
    let bytes = fixture(&[(1, &[0xd8, 12, 0xf0, 0xd4, 0x1f, 0xfd, 0, 4, 0x41])]);
    let song = header(&bytes, 1, 0x100, 1, 0x4000).unwrap();
    for (cancel, remaining, expected) in [
        (true, 1000, ScanStop::Cancelled),
        (false, 0, ScanStop::WorkLimit),
    ] {
        let cancel = AtomicBool::new(cancel);
        let mut budget = Budget {
            cancel: &cancel,
            remaining,
        };
        assert!(
            matches!(sequence::interpret(&bytes, song.clone(), 1, MAX_FRAMES, &mut budget), Err(sequence::ReadError::Stop(stop)) if stop == expected)
        );
    }
    let short = trace(&bytes, 8, 5);
    assert!(short.truncated);
    assert_eq!(short.notes[0].duration, 5);
    assert_eq!(short.song.channels[0].end_frame, 5);
    assert!(!short.song.midi_exportable);
}

#[test]
fn arbitrary_game_titles_and_changed_source_do_not_pass_the_profile_gate() {
    let mut bytes = vec![0; 2 * 1024 * 1024];
    bytes[0x134..0x140].copy_from_slice(b"TEST_PROFILE");
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1000,
    };
    let mut songs = Vec::new();
    scan(&bytes, &mut songs, &mut budget, 100).unwrap();
    assert!(songs.is_empty());
    let fixture = fixture(&[(1, &[0xff])]);
    let song = synthetic_song(&fixture, 1, 0x4000);
    assert!(validate_song(&fixture, &song, &cancel).is_err());
}

#[test]
fn midi_has_embedded_limits_and_expected_events() {
    let bytes = fixture(&[
        (1, &[0xd8, 12, 0xf0, 0xd4, 0x13, 0xff]),
        (3, &[0xd8, 12, 0x10, 0xd4, 0x13, 0xff]),
    ]);
    let song = synthetic_song(&bytes, 1, 0x4000);
    let result = midi::encode(&bytes, &song, 1, 7200, &AtomicBool::new(false)).unwrap();
    assert!(result.bytes.starts_with(b"MThd\0\0\0\x06\0\x01"));
    assert!(result.bytes.windows(3).any(|data| data == [0x90, 72, 127]));
    assert!(result.bytes.windows(3).any(|data| data == [0x92, 60, 127]));
    assert!(
        result
            .bytes
            .windows(14)
            .any(|data| data == b"source_sha256\"")
    );
    assert!(
        result
            .bytes
            .windows(17)
            .any(|data| data == b"General MIDI puls")
    );
    assert!(midi::midi(&bytes, &song, 1, 30, &AtomicBool::new(false)).is_err());
    assert!(midi::encode(&bytes, &song, 1, 30, &AtomicBool::new(true)).is_err());
}
