use super::*;
use std::sync::atomic::Ordering;

fn fixture(mask: u16, streams: &[&[u8]]) -> (Vec<u8>, Profile) {
    let mut bytes = vec![0; 0x2000];
    let profile = Profile {
        name: PROFILE,
        sha256: "",
        table: 0x100,
        count: 2,
        channel_config: 0x200,
        silence: Some(1),
        witnesses: &[(0x10, 8)],
    };
    put_pointer(&mut bytes, 0x100, 0x180);
    put_pointer(&mut bytes, 0x104, 0x180);
    bytes[0x180..0x182].copy_from_slice(&mask.to_le_bytes());
    bytes[0x182] = 0xf0;
    for (number, kind) in CHANNEL_KINDS.iter().copied().enumerate() {
        bytes[0x200 + number * 12 + 8] = kind;
    }
    for (index, stream) in streams.iter().enumerate() {
        let start = 0x300 + index * 0x100;
        put_pointer(&mut bytes, 0x184 + index * 4, start);
        bytes[start..start + stream.len()].copy_from_slice(stream);
    }
    (bytes, profile)
}

fn put_pointer(bytes: &mut [u8], offset: usize, target: usize) {
    bytes[offset..offset + 4].copy_from_slice(&(0x0800_0000 + target as u32).to_le_bytes());
}

fn read_song(bytes: &[u8], profile: &Profile) -> Result<NatsumeSong, sequence::ReadError> {
    inspect(
        bytes,
        0,
        profile,
        &mut Budget {
            cancel: &AtomicBool::new(false),
            remaining: MAX_VALIDATION_WORK,
        },
    )
}

fn read_sequence(
    bytes: &[u8],
    kind: u8,
    start: usize,
) -> Result<(NatsumeChannel, Vec<RomSpan>), sequence::ReadError> {
    sequence::inspect(
        bytes,
        0,
        kind,
        start,
        &mut Budget {
            cancel: &AtomicBool::new(false),
            remaining: MAX_VALIDATION_WORK,
        },
    )
}

#[test]
fn little_endian_mask_traverses_all_twelve_channels_in_driver_order() {
    let (bytes, profile) = fixture(0x0fff, &[&[0xff][..]; 12]);
    let song = read_song(&bytes, &profile).unwrap();
    assert_eq!(song.channel_mask, 0x0fff);
    assert_eq!(song.header.byte_len, 52);
    assert_eq!(song.priority, 0xf0);
    assert_eq!(song.kind, NatsumeSongKind::Setup);
    assert_eq!(song.channels.len(), 12);
    for (index, channel) in song.channels.iter().enumerate() {
        assert_eq!(channel.number, index as u8);
        assert_eq!(channel.hardware_kind, CHANNEL_KINDS[index]);
        assert_eq!(channel.entry.effective_offset, 0x300 + index as u32 * 0x100);
    }
    let (bytes, profile) = fixture(0x0801, &[&[60, 0, 0xff], &[61, 1, 0xff]]);
    let song = read_song(&bytes, &profile).unwrap();
    assert_eq!(
        song.channels.iter().map(|c| c.number).collect::<Vec<_>>(),
        [0, 11]
    );
    assert_eq!(song.channels[1].wait_units, 2);
    assert_eq!(song.header.byte_len, 12);
}

#[test]
fn both_initial_selectors_are_setup_and_the_profile_silence_selector_is_retained() {
    let (mut bytes, mut profile) = fixture(0x800, &[&[0xff]]);
    profile.count = 4;
    profile.silence = Some(3);
    put_pointer(&mut bytes, 0x108, 0x180);
    put_pointer(&mut bytes, 0x10c, 0x180);
    let mut songs = Vec::new();
    scan_profile(
        &bytes,
        &mut songs,
        &mut Budget {
            cancel: &AtomicBool::new(false),
            remaining: 1_000,
        },
        4,
        &profile,
    )
    .unwrap();
    assert_eq!(
        songs.iter().map(|song| song.kind).collect::<Vec<_>>(),
        [
            NatsumeSongKind::Setup,
            NatsumeSongKind::Setup,
            NatsumeSongKind::Music,
            NatsumeSongKind::Silence,
        ]
    );
    assert_eq!(
        songs.iter().map(|song| song.index).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    #[cfg(not(target_arch = "wasm32"))]
    for song in &songs {
        use crate::{
            catalog::SongRef,
            formats::{AudioFormat, SongFormat},
        };
        assert!(SongRef::Natsume(song).supports(SongFormat::MappedAssets));
        for format in [AudioFormat::Wav, AudioFormat::Flac, AudioFormat::Ogg] {
            assert_eq!(
                SongRef::Natsume(song).supports(SongFormat::Audio(format)),
                song.index == 2
            );
        }
    }
}

#[test]
fn fixed_operand_commands_keep_the_following_note_aligned() {
    let commands = [
        0xe0, 3, 0xe1, 9, 0xe2, 1, 0xe3, 2, 0xe4, 0xe5, 0xe6, 0xe7, 0xe8, 0xfe, 0xe9, 0x40, 0xea,
        3, 0xeb, 0xf7, 0, 0xf8, 2, 0xf9, 3, 0x45, 0xfa, 2, 0xfb, 1, 2, 3, 4, 0xfc, 0xfd, 0xfe, 2,
        60, 5, 0xff,
    ];
    let (channel, spans) = read_sequence(&commands, 0, 0).unwrap();
    assert_eq!(channel.wait_units, 522);
    assert_eq!(channel.note_count, 1);
    assert_eq!(channel.event_count, 22);
    assert_eq!(spans, [RomSpan::new(0, commands.len())]);
    assert_eq!(channel.termination, NatsumeTermination::Fine);
}

#[test]
fn rest_aliases_and_nibble_commands_have_driver_operand_lengths() {
    for rest in 0x80..=0x8f {
        let (channel, _) = read_sequence(&[rest, 3, 60, 0, 0xff], 0, 0).unwrap();
        assert_eq!(
            (channel.event_count, channel.note_count, channel.wait_units),
            (3, 1, 5)
        );
    }
    for command in 0x90..=0xdf {
        let (channel, _) = read_sequence(&[command, 60, 0, 0xff], 0, 0).unwrap();
        assert_eq!(
            (channel.event_count, channel.note_count, channel.wait_units),
            (3, 1, 1)
        );
    }
}

#[test]
fn both_counted_loops_are_independent_and_zero_means_256_passes() {
    let (channel, _) = read_sequence(&[0xf0, 2, 0xf2, 3, 60, 0, 0xf3, 0xf1, 0xff], 0, 0).unwrap();
    assert_eq!(
        (channel.note_count, channel.wait_units, channel.event_count),
        (6, 6, 18)
    );
    for (begin, end) in [(0xf0, 0xf1), (0xf2, 0xf3)] {
        let (channel, _) = read_sequence(&[begin, 0, 60, 0, end, 0xff], 0, 0).unwrap();
        assert_eq!(
            (channel.note_count, channel.wait_units, channel.event_count),
            (256, 256, 514)
        );
    }
}

#[test]
fn absolute_call_return_and_jump_preserve_only_visited_source_ranges() {
    let mut bytes = vec![0xec; 0x400];
    bytes[0x300] = 0xf5;
    put_pointer(&mut bytes, 0x301, 0x345);
    bytes[0x305..0x308].copy_from_slice(&[0x80, 0, 0xf4]);
    put_pointer(&mut bytes, 0x308, 0x380);
    bytes[0x345..0x348].copy_from_slice(&[60, 1, 0xf6]);
    bytes[0x380] = 0xff;
    let (channel, spans) = read_sequence(&bytes, 4, 0x300).unwrap();
    assert_eq!(
        (channel.event_count, channel.note_count, channel.wait_units),
        (6, 1, 3)
    );
    assert_eq!(
        spans,
        [
            RomSpan::new(0x300, 12),
            RomSpan::new(0x345, 3),
            RomSpan::new(0x380, 1)
        ]
    );
}

#[test]
fn nested_calls_overwrite_the_single_return_slot() {
    let mut bytes = vec![0xff; 0x380];
    for (source, target) in [(0x300, 0x340), (0x340, 0x360)] {
        bytes[source] = 0xf5;
        put_pointer(&mut bytes, source + 1, target);
    }
    bytes[0x345] = 0xf6;
    bytes[0x360..0x363].copy_from_slice(&[60, 0, 0xf6]);
    assert_eq!(
        read_sequence(&bytes, 0, 0x300),
        Err(sequence::ReadError::Invalid(
            "sequence contains a zero-wait control cycle"
        ))
    );
}

#[test]
fn repeated_states_end_valid_loops_but_reject_zero_wait_cycles() {
    let mut bytes = vec![60, 2, 0xf4, 0, 0, 0, 0];
    put_pointer(&mut bytes, 3, 0);
    let (channel, spans) = read_sequence(&bytes, 0, 0).unwrap();
    assert_eq!(channel.termination, NatsumeTermination::Loop);
    assert_eq!(channel.loop_start_wait_units, Some(0));
    assert_eq!((channel.event_count, channel.wait_units), (2, 3));
    assert_eq!(spans, [RomSpan::new(0, bytes.len())]);
    let mut bytes = vec![0xf4, 0, 0, 0, 0];
    put_pointer(&mut bytes, 1, 0);
    assert!(matches!(
        read_sequence(&bytes, 0, 0),
        Err(sequence::ReadError::Invalid(_))
    ));
}

#[test]
fn note_modes_and_rest_ticks_reset_the_high_length_prefix() {
    for kind in [0, 1, 2, 4, 5] {
        let (channel, _) = read_sequence(
            &[0xfe, 1, 0xe1, 3, 0x23, 0xff, 0xfe, 2, 0x24, 4, 0xff],
            kind,
            0,
        )
        .unwrap();
        assert_eq!(
            (channel.event_count, channel.note_count, channel.wait_units),
            (6, 2, 8)
        );
    }
    let (channel, _) = read_sequence(&[0xfe, 1, 0xe1, 4, 0x23, 0xff], 3, 0).unwrap();
    assert_eq!(
        (channel.event_count, channel.note_count, channel.wait_units),
        (4, 1, 4)
    );
    let (channel, _) = read_sequence(&[0xfa, 2, 0x80, 0, 60, 0, 0xff], 0, 0).unwrap();
    assert_eq!(channel.wait_units, 2);
}

#[test]
fn malformed_headers_and_configuration_are_rejected() {
    let (bytes, profile) = fixture(0x800, &[&[0xff]]);
    for (offset, replacement) in [(0x180, 0x0f), (0x181, 0xff), (0x183, 1), (0x208, 6)] {
        let mut malformed = bytes.clone();
        malformed[offset] = replacement;
        assert!(read_song(&malformed, &profile).is_err());
    }
    for address in [0, 0x0200_0000, 0x0800_0181, 0x09ff_ffff, u32::MAX] {
        let mut malformed = bytes.clone();
        malformed[0x100..0x104].copy_from_slice(&address.to_le_bytes());
        assert!(read_song(&malformed, &profile).is_err());
    }
    assert!(read_song(&bytes[..0x182], &profile).is_err());
}

#[test]
fn malformed_commands_and_uninitialized_branches_fail_closed() {
    for command in [0xec, 0xed, 0xee, 0xef, 0xf1, 0xf3, 0xf6] {
        assert!(read_sequence(&[command, 0xff], 0, 0).is_err());
    }
    for command in [
        0x80, 0xe0, 0xe1, 0xe2, 0xe3, 0xe8, 0xe9, 0xea, 0xf0, 0xf2, 0xf4, 0xf5, 0xf7, 0xf8, 0xf9,
        0xfa, 0xfb, 0xfe,
    ] {
        assert!(read_sequence(&[command], 0, 0).is_err());
    }
    assert!(read_sequence(&[0xfe, 3, 0xff], 0, 0).is_err());
    assert!(read_sequence(&[0xf4, 0, 0, 0, 0], 0, 0).is_err());
    assert!(sequence::range(&[], usize::MAX, 2).is_err());
}

#[test]
fn scan_limits_and_cancellation_preserve_completed_entries() {
    let (bytes, profile) = fixture(0x800, &[&[60, 0, 0xff]]);
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000,
    };
    let mut songs = Vec::new();
    assert_eq!(
        scan_profile(&bytes, &mut songs, &mut budget, 1, &profile),
        Err(ScanStop::CandidateLimit)
    );
    assert_eq!(songs.len(), 1);
    let retained = songs.clone();
    cancel.store(true, Ordering::Relaxed);
    assert_eq!(
        scan_profile(&bytes, &mut songs, &mut budget, 2, &profile),
        Err(ScanStop::Cancelled)
    );
    assert_eq!(songs, retained);
    cancel.store(false, Ordering::Relaxed);
    budget.remaining = 0;
    assert_eq!(
        scan_profile(&bytes, &mut songs, &mut budget, 2, &profile),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(songs, retained);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000,
    };
    let first = inspect(&bytes, 0, &profile, &mut budget).unwrap();
    let one_song_work = 1_000 - budget.remaining;
    budget.remaining = one_song_work + 2;
    let mut songs = Vec::new();
    assert_eq!(
        scan_profile(&bytes, &mut songs, &mut budget, 2, &profile),
        Err(ScanStop::WorkLimit)
    );
    assert_eq!(songs, [first]);
}

#[test]
fn validation_limits_bound_distinct_nonterminating_commands() {
    let bytes = vec![0x90; MAX_EVENTS + 1];
    assert_eq!(
        read_sequence(&bytes, 0, 0),
        Err(sequence::ReadError::Stop(ScanStop::ValidationLimit))
    );
}

#[test]
fn mapped_ranges_are_canonical_sorted_and_within_source() {
    let (bytes, profile) = fixture(0x800, &[&[60, 0, 0xff]]);
    let song = read_song(&bytes, &profile).unwrap();
    assert_eq!(
        song.mapped_spans,
        [
            RomSpan::new(0x10, 8),
            RomSpan::new(0x100, 4),
            RomSpan::new(0x180, 8),
            RomSpan::new(0x200, 156),
            RomSpan::new(0x300, 3)
        ]
    );
    for span in song.mapped_spans {
        assert_eq!(
            span.canonical_cpu_address,
            0x0800_0000 + span.effective_offset
        );
        assert!(
            bytes
                .get(
                    span.effective_offset as usize
                        ..(span.effective_offset + span.byte_len) as usize
                )
                .is_some()
        );
    }
}

#[test]
fn synthetic_inventory_cannot_authenticate_as_a_retail_profile() {
    let (bytes, profile) = fixture(0x800, &[&[0xff]]);
    let song = read_song(&bytes, &profile).unwrap();
    let cancel = AtomicBool::new(false);
    assert!(
        validate_song(&bytes, &song, &cancel)
            .unwrap_err()
            .to_string()
            .contains("unrecognized")
    );
    let mut songs = Vec::new();
    assert_eq!(
        scan(
            &bytes,
            &mut songs,
            &mut Budget {
                cancel: &cancel,
                remaining: 1_000
            },
            2
        ),
        Ok(())
    );
    assert!(songs.is_empty());
}

#[test]
fn authenticated_inventory_validation_reparses_and_rejects_forged_fields() {
    let (bytes, profile) = fixture(0x800, &[&[60, 0, 0xff]]);
    let song = read_song(&bytes, &profile).unwrap();
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 10_000,
    };
    validate_profile_song(&bytes, &song, &profile, &mut budget).unwrap();
    for field in 0..6 {
        let mut forged = song.clone();
        match field {
            0 => forged.profile = "forged-profile",
            1 => forged.index = profile.count,
            2 => forged.mapped_spans.push(RomSpan::new(0x500, 1)),
            3 => forged.channels[0].entry.canonical_cpu_address += 1,
            4 => forged.channels[0].wait_units += 1,
            _ => forged.warnings.clear(),
        }
        assert!(validate_profile_song(&bytes, &forged, &profile, &mut budget).is_err());
    }
    cancel.store(true, Ordering::Relaxed);
    assert!(validate_profile_song(&bytes, &song, &profile, &mut budget).is_err());
}

#[test]
fn fuzz_seam_reaches_synthetic_headers_and_direct_sequences_with_limits() {
    let (bytes, _) = fixture(0x0fff, &[&[60, 0, 0xff][..]; 12]);
    for max_candidates in [0, 1, 2] {
        for max_work in [0, 1, 1_000] {
            for cancelled in [false, true] {
                fuzzing::fuzz_parse(
                    &bytes,
                    super::super::ScanLimits {
                        max_candidates,
                        max_work,
                    },
                    &AtomicBool::new(cancelled),
                );
                fuzzing::fuzz_parse(
                    &[0, 0xfa, 1, 60, 0, 0xff],
                    super::super::ScanLimits {
                        max_candidates,
                        max_work,
                    },
                    &AtomicBool::new(cancelled),
                );
            }
        }
    }
}
