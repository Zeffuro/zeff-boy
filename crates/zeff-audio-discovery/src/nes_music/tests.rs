use super::*;
use crate::test_support::nes_music::fixture;

fn put(bytes: &mut [u8], address: u16, data: &[u8]) {
    let offset = usize::from(address) - 0x8000 + 16;
    bytes[offset..offset + data.len()].copy_from_slice(data);
}

fn trace(bytes: &[u8], index: u8, loops: u8, max_frames: u32) -> sequence::Program {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    sequence::interpret(bytes, index, loops, max_frames, &mut budget).unwrap()
}

fn layout_witnesses(bytes: &[u8]) -> Vec<Witness> {
    WITNESSES
        .iter()
        .map(|witness| Witness {
            offset: witness.offset,
            len: witness.len,
            sha256: Box::leak(
                const_hex::encode(zeff_firmware::sha256_bytes(
                    &bytes[witness.offset..witness.offset + witness.len],
                ))
                .into_boxed_str(),
            ),
        })
        .collect()
}

fn matches_layout(bytes: &[u8], witnesses: &[Witness]) -> bool {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    recognized_with_witnesses(bytes, &mut budget, witnesses).unwrap()
}

#[test]
fn nrom_layout_requires_header_mapping_and_all_driver_witnesses() {
    let bytes = fixture();
    let witnesses = layout_witnesses(&bytes);
    assert!(matches_layout(&bytes, &witnesses));

    for (offset, mask) in [(4, 1), (5, 1), (6, 4), (6, 0x10), (7, 8), (9, 1)] {
        let mut changed = bytes.clone();
        changed[offset] ^= mask;
        assert!(!matches_layout(&changed, &witnesses));
    }
    for offset in [0x790d, 0x791d, 0x7f00, 0x7f66, 0x7210, 0x7510, 0x800a] {
        let mut changed = bytes.clone();
        changed[offset] ^= 1;
        assert!(
            !matches_layout(&changed, &witnesses),
            "witness at {offset:#x}"
        );
    }
    assert!(!matches_layout(&bytes[..bytes.len() - 1], &witnesses));
}

#[test]
fn qualified_layout_interprets_every_fixed_selector() {
    let bytes = fixture();
    for index in 0..SONG_COUNT as u8 {
        let program = trace(&bytes, index, 1, MAX_FRAMES);
        assert_eq!(program.song.profile, PROFILE);
        assert_eq!(program.song.index, index);
    }
}

#[test]
fn four_channels_follow_distinct_encodings_and_global_termination() {
    let program = trace(&fixture(), 13, 1, MAX_FRAMES);
    assert!(program.song.midi_exportable);
    assert_eq!(program.song.queue, NesQueue::Area);
    assert_eq!(program.song.selector, 0x20);
    assert_eq!(
        program
            .song
            .channels
            .iter()
            .map(|c| (c.number, c.event_count, c.note_count, c.end_frame))
            .collect::<Vec<_>>(),
        [(1, 2, 2, 5), (2, 5, 2, 5), (3, 4, 2, 5), (4, 5, 3, 5)]
    );
    assert_eq!(
        program
            .notes
            .iter()
            .map(|n| (n.channel, n.frame, n.duration))
            .collect::<Vec<_>>(),
        [
            (2, 0, 2),
            (1, 0, 2),
            (3, 0, 2),
            (4, 0, 2),
            (2, 2, 3),
            (1, 2, 3),
            (3, 2, 3),
            (4, 2, 2),
            (4, 4, 1)
        ]
    );
    assert!(
        program
            .song
            .channels
            .iter()
            .all(|c| c.termination == NesTermination::Fine)
    );
    assert!(
        program
            .notes
            .iter()
            .filter(|n| n.channel == 4)
            .all(|n| n.drum == Some(42))
    );
}

#[test]
fn ground_playlist_retains_lead_in_and_all_repeated_sections() {
    let bytes = fixture();
    let first = trace(&bytes, 8, 1, MAX_FRAMES);
    let twice = trace(&bytes, 8, 2, MAX_FRAMES);
    assert_eq!(first.song.sections.len(), 33);
    assert_eq!(twice.song.sections.len(), 65);
    assert_eq!(first.song.channels[0].end_frame, 165);
    assert_eq!(twice.song.channels[0].end_frame, 325);
    assert_eq!(twice.song.channels[0].loop_start_frame, Some(5));
    let offset = pointer(&bytes, TABLE + 16, 1).unwrap() as u32;
    assert_eq!(first.song.table_entry.offset, offset);
    assert_eq!(twice.song.sections[32].table_entry.offset, offset + 32);
    assert_eq!(twice.song.sections[33].table_entry.offset, offset + 1);
    assert_eq!(twice.song.sections[33].start_frame, 165);
}

#[test]
fn header_transition_preserves_length_buffers_and_preempts_old_notes() {
    let mut bytes = fixture();
    put(&mut bytes, TABLE + 17, &[0x46; 32]);
    put(&mut bytes, TABLE + 0x40, &[0, 0, 0x82, 0, 0, 0x60]);
    put(&mut bytes, TABLE + 0x46, &[0, 0, 0x83, 0, 0, 0x60]);
    put(&mut bytes, 0x8200, &[0x81, 0x18, 0]);
    put(&mut bytes, 0x8300, &[0x18, 0]);
    put(&mut bytes, 0x8360, &[0x10, 0]);
    let program = trace(&bytes, 8, 1, MAX_FRAMES);
    assert!(program.song.midi_exportable);
    assert_eq!(program.song.channels[1].end_frame, 99);
    assert_eq!(program.song.channels[1].note_count, 33);
    assert_eq!(program.song.channels[2].note_count, 33);
    assert!(
        program
            .notes
            .iter()
            .filter(|n| n.channel == 2 || n.channel == 3)
            .all(|n| n.duration == 3)
    );
}

#[test]
fn triangle_zero_waits_for_counter_wrap_and_offset_zero_is_enabled() {
    let mut bytes = fixture();
    put(&mut bytes, TABLE + 0x40, &[0, 0, 0x82, 0, 0, 0x60]);
    let shared = trace(&bytes, 10, 1, MAX_FRAMES);
    assert_eq!(shared.song.channels[0].event_count, 0);
    assert_eq!(
        shared.song.channels[1].note_count,
        shared.song.channels[2].note_count
    );
    assert_eq!(shared.song.channels[3].event_count, 0);
    put(&mut bytes, TABLE + 0x40, &[0, 0, 0x82, 0x40, 0, 0x60]);
    put(&mut bytes, LENGTHS, &[200]);
    put(&mut bytes, 0x8200, &[0x80, 0x18, 0x18, 0]);
    put(&mut bytes, 0x8240, &[0, 0x81, 0x18, 0]);
    let program = trace(&bytes, 1, 1, MAX_FRAMES);
    let triangle = program
        .notes
        .iter()
        .filter(|n| n.channel == 3)
        .collect::<Vec<_>>();
    assert_eq!(triangle.len(), 1);
    assert_eq!((triangle[0].frame, triangle[0].duration), (256, 3));
    assert_eq!(program.song.channels[1].end_frame, 400);
}

#[test]
fn byte_offsets_wrap_inside_the_music_data_base() {
    let mut bytes = fixture();
    put(&mut bytes, TABLE + 0x40, &[0, 0, 0x82, 0xfe, 0, 0x60]);
    put(&mut bytes, 0x8200, &[0x87, 0x18, 0]);
    put(&mut bytes, 0x82fe, &[0x80, 0x18]);
    let program = trace(&bytes, 1, 1, MAX_FRAMES);
    assert_eq!(
        program
            .notes
            .iter()
            .filter(|n| n.channel == 3)
            .map(|n| (n.frame, n.duration))
            .collect::<Vec<_>>(),
        [(0, 2), (2, 7)]
    );
    assert!(program.song.mapped_spans.contains(&span(0x30e, 2)));
    assert!(
        program
            .song
            .mapped_spans
            .iter()
            .all(|s| !(s.offset <= 0x310 && s.offset + s.byte_len > 0x310))
    );
}

#[test]
fn pulse_length_operand_zero_is_a_pitch_index_and_triangle_zero_is_control() {
    let mut bytes = fixture();
    put(&mut bytes, 0x8200, &[0x80, 0, 0]);
    put(&mut bytes, 0x8240, &[0x80, 0, 0]);
    put(&mut bytes, FREQUENCIES, &[1, 1]);
    let program = trace(&bytes, 1, 1, MAX_FRAMES);
    assert_eq!(program.song.channels[1].note_count, 1);
    assert_eq!(program.song.channels[2].note_count, 0);
    assert_eq!(
        program.notes.iter().find(|n| n.channel == 2).unwrap().timer,
        257
    );
}

#[test]
fn hardware_sweep_and_prior_area_context_gate_export() {
    let mut bytes = fixture();
    put(&mut bytes, 0x8220, &[0, 0x18, 0x9a]);
    let sweep = trace(&bytes, 0, 1, MAX_FRAMES);
    assert!(!sweep.song.midi_exportable);
    assert_eq!(sweep.song.warnings.len(), 1);
    assert!(sweep.song.warnings[0].reason.contains("hardware sweep"));
    let time_out = trace(&fixture(), 6, 1, MAX_FRAMES);
    assert!(!time_out.song.midi_exportable);
    assert_eq!(time_out.song.warnings.len(), 1);
    assert!(time_out.song.warnings[0].reason.contains("prior area"));
}

#[test]
fn finite_aliases_silence_and_duration_clipping_remain_explicit() {
    let mut bytes = fixture();
    let one = trace(&bytes, 1, 1, MAX_FRAMES);
    let alias = trace(&bytes, 4, 8, MAX_FRAMES);
    assert_eq!(one.song.header, alias.song.header);
    assert_ne!(one.song.table_entry, alias.song.table_entry);
    assert_eq!(one.notes.len(), alias.notes.len());
    let clipped = trace(&bytes, 2, 8, 3);
    assert!(clipped.truncated);
    assert_eq!(clipped.song.channels[0].end_frame, 3);
    assert!(clipped.notes.iter().all(|n| n.frame + n.duration <= 3));
    put(&mut bytes, 0x8200, &[0]);
    let silent = trace(&bytes, 7, 1, MAX_FRAMES);
    assert!(silent.song.midi_exportable);
    assert!(silent.notes.is_empty());
    assert!(silent.song.channels.iter().all(|c| c.end_frame == 0));
}

#[test]
fn malformed_tables_cycles_unknown_identity_and_cancellation_are_bounded() {
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    let bytes = fixture();
    let mut songs = Vec::new();
    scan(&bytes, &mut songs, &mut budget, 16).unwrap();
    assert!(songs.is_empty());
    for (address, values) in [
        (0x8200, &[0x80, 0x19, 0][..]),
        (0x8200, &[0x18, 0][..]),
        (TABLE + 0x40, &[255][..]),
        (TABLE + 0x41, &[255, 255][..]),
        (0x8260, &[0][..]),
    ] {
        let mut broken = fixture();
        put(&mut broken, address, values);
        assert!(sequence::interpret(&broken, 13, 1, MAX_FRAMES, &mut budget).is_err());
    }
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1,
    };
    assert!(matches!(
        sequence::interpret(&bytes, 1, 1, MAX_FRAMES, &mut budget),
        Err(sequence::ReadError::Stop(ScanStop::WorkLimit))
    ));
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 1_000_000,
    };
    assert!(matches!(
        sequence::interpret(&bytes, 1, 1, MAX_FRAMES, &mut budget),
        Err(sequence::ReadError::Stop(ScanStop::Cancelled))
    ));
    assert!(validate_song(&bytes, &trace(&bytes, 1, 1, MAX_FRAMES).song, &cancel).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn independent_midi_reader_checks_tempo_pitch_drums_and_note_offs() {
    let bytes = fixture();
    let song = trace(&bytes, 13, 1, MAX_FRAMES).song;
    let cancel = AtomicBool::new(false);
    let output = midi::encode(&bytes, &song, 1, 10, &cancel).unwrap().bytes;
    assert_eq!(&output[..14], b"MThd\0\0\0\x06\0\x01\0\x05\x01\0");
    let mut cursor = 14;
    let mut starts = Vec::new();
    let mut ends = Vec::new();
    let mut tempo = None;
    for track_index in 0..5 {
        assert_eq!(&output[cursor..cursor + 4], b"MTrk");
        let len = u32::from_be_bytes(output[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
        cursor += 8;
        let end = cursor + len;
        let mut tick = 0;
        let mut active = std::collections::BTreeSet::new();
        while cursor < end {
            tick += read_vlq(&output, &mut cursor);
            let status = output[cursor];
            cursor += 1;
            if status == 0xff {
                let kind = output[cursor];
                cursor += 1;
                let size = read_vlq(&output, &mut cursor) as usize;
                let value = &output[cursor..cursor + size];
                cursor += size;
                if kind == 0x51 {
                    tempo = Some(u32::from_be_bytes([0, value[0], value[1], value[2]]));
                }
                if kind == 0x2f {
                    assert_eq!(tick, 5);
                    assert!(active.is_empty());
                }
            } else {
                let a = output[cursor];
                cursor += 1;
                let b = if status & 0xf0 == 0xc0 {
                    0
                } else {
                    let v = output[cursor];
                    cursor += 1;
                    v
                };
                let channel = status & 15;
                match status & 0xf0 {
                    0x90 if b != 0 => {
                        assert!(active.insert((channel, a)));
                        starts.push((track_index, tick, channel, a));
                    }
                    0x80 => {
                        assert!(active.remove(&(channel, a)));
                        ends.push((track_index, tick, channel, a));
                    }
                    0xb0 | 0xc0 | 0xe0 => (),
                    _ => panic!("unexpected MIDI event"),
                }
            }
        }
        assert_eq!(cursor, end);
    }
    assert_eq!(cursor, output.len());
    assert_eq!(tempo, Some(4_259_651));
    assert_eq!(starts.len(), 9);
    assert_eq!(ends.len(), 9);
    assert_eq!(
        starts
            .iter()
            .filter(|e| e.2 == 1)
            .map(|e| (e.1, e.3))
            .collect::<Vec<_>>(),
        [(0, 69), (2, 71)]
    );
    assert_eq!(
        starts
            .iter()
            .filter(|e| e.2 == 2)
            .map(|e| (e.1, e.3))
            .collect::<Vec<_>>(),
        [(0, 57), (2, 59)]
    );
    assert_eq!(
        starts
            .iter()
            .filter(|e| e.2 == 9)
            .map(|e| (e.1, e.3))
            .collect::<Vec<_>>(),
        [(0, 42), (2, 42), (4, 42)]
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn read_vlq(bytes: &[u8], cursor: &mut usize) -> u32 {
    let mut value = 0;
    for _ in 0..4 {
        let byte = bytes[*cursor];
        *cursor += 1;
        value = value * 128 + u32::from(byte & 127);
        if byte < 128 {
            return value;
        }
    }
    panic!("overlong MIDI VLQ")
}
