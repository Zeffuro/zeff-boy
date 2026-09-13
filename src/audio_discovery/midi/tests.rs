use super::*;
use crate::audio_discovery::test_support;
use std::io::Cursor;

#[derive(Debug)]
struct ParsedEvent {
    tick: u32,
    status: u8,
    data: Vec<u8>,
    meta: Option<u8>,
}

fn read_vlq(data: &[u8], cursor: &mut usize) -> u32 {
    let mut result = 0;
    for _ in 0..4 {
        let byte = data[*cursor];
        *cursor += 1;
        result = result * 128 + u32::from(byte & 127);
        if byte & 128 == 0 {
            return result;
        }
    }
    panic!("unterminated standard MIDI VLQ");
}

fn parse(data: &[u8]) -> Vec<Vec<ParsedEvent>> {
    assert_eq!(&data[..10], b"MThd\0\0\0\x06\0\x01");
    assert_eq!(u16::from_be_bytes(data[12..14].try_into().unwrap()), 48);
    let track_count = u16::from_be_bytes(data[10..12].try_into().unwrap());
    let mut offset = 14;
    let mut tracks = Vec::new();
    for _ in 0..track_count {
        assert_eq!(&data[offset..offset + 4], b"MTrk");
        let len = u32::from_be_bytes(data[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let end = offset + 8 + len;
        offset += 8;
        let mut tick = 0;
        let mut events = Vec::new();
        while offset < end {
            tick += read_vlq(data, &mut offset);
            let status = data[offset];
            offset += 1;
            let (meta, len) = if status == 0xFF {
                let kind = data[offset];
                offset += 1;
                (Some(kind), read_vlq(data, &mut offset) as usize)
            } else {
                assert!((0x80..=0xEF).contains(&status));
                (None, if status & 0xF0 == 0xC0 { 1 } else { 2 })
            };
            let bytes = data[offset..offset + len].to_vec();
            if meta.is_none() {
                assert!(bytes.iter().all(|byte| *byte <= 127));
            }
            events.push(ParsedEvent {
                tick,
                status,
                data: bytes,
                meta,
            });
            offset += len;
        }
        assert_eq!(offset, end);
        assert_eq!(events.last().unwrap().meta, Some(0x2F));
        tracks.push(events);
    }
    assert_eq!(offset, data.len());
    tracks
}

fn fixture(sequences: &[&[u8]], custom_sample: bool) -> (Vec<u8>, SongCandidate) {
    let mut bytes = test_support::fixture();
    bytes[0x100] = sequences.len() as u8;
    for (index, sequence) in sequences.iter().enumerate() {
        let offset = 0x400 + index * 0x100;
        test_support::put_word(&mut bytes, 0x108 + 4 * index, 0x0800_0000 + offset as u32);
        bytes[offset..offset + sequence.len()].copy_from_slice(sequence);
    }
    if custom_sample {
        test_support::put_word(&mut bytes, 0x30C, 0);
    }
    let report = crate::audio_discovery::scan(
        zeff_emu_common::system::System::Gba,
        &bytes,
        Default::default(),
        &AtomicBool::new(false),
    );
    let song = report
        .candidates
        .into_iter()
        .find(|song| song.header.effective_offset == 0x100)
        .unwrap();
    (bytes, song)
}

fn export(bytes: &[u8], song: &SongCandidate, max_seconds: u16) -> Result<Vec<u8>> {
    export_with_gain(bytes, song, max_seconds, PlaybackGain::Raw)
}

fn export_with_gain(
    bytes: &[u8],
    song: &SongCandidate,
    max_seconds: u16,
    playback_gain: PlaybackGain,
) -> Result<Vec<u8>> {
    encode(
        song,
        bytes,
        MidiOptions {
            loops: 2,
            max_seconds,
            skip_channel10: true,
            bank_select: BankSelect::Gs,
            playback_gain,
        },
        json!({"media": {"sha256": "fixture-sha256"}, "title": "音"}),
        &AtomicBool::new(false),
        &AtomicU32::new(0),
    )
}

#[test]
fn high_native_volume_keeps_sequence_and_timed_values_in_bounded_midi() -> Result<()> {
    for gain in [PlaybackGain::Raw, PlaybackGain::Mp2kAmplitude] {
        let sequence = [
            0xBD, 0, 0xBE, 0, 0x80, 0xBE, 64, 0x81, 0xBE, 127, 0x81, 0xBE, 128, 0x81, 0xBE, 160,
            0x81, 0xBE, 255, 0xD3, 60, 100, 0x84, 0xB1,
        ];
        let (bytes, song) = fixture(&[&sequence], false);
        let tracks = parse(&export_with_gain(&bytes, &song, 300, gain)?);
        let controls: Vec<_> = tracks[1]
            .iter()
            .filter(|event| event.status == 0xB0 && event.data[0] == 7)
            .map(|event| (event.tick, event.data[1]))
            .collect();
        let middle = match gain {
            PlaybackGain::Raw => 64,
            PlaybackGain::Mp2kAmplitude => 90,
        };
        assert_eq!(
            controls,
            [
                (0, 0),
                (0, 0),
                (0, middle),
                (2, 127),
                (4, 127),
                (6, 127),
                (8, 127)
            ]
        );
        let originals: Vec<_> = tracks[1]
            .iter()
            .filter(|event| event.meta == Some(1))
            .map(|event| (event.tick, String::from_utf8(event.data.clone()).unwrap()))
            .collect();
        assert_eq!(
            originals,
            [
                (4, "MP2k VOL 128 clipped to MIDI full-scale 127".into()),
                (6, "MP2k VOL 160 clipped to MIDI full-scale 127".into()),
                (8, "MP2k VOL 255 clipped to MIDI full-scale 127".into()),
            ]
        );
        let metadata: Value = serde_json::from_slice(&tracks[0][1].data)?;
        assert_eq!(
            metadata["projection_warnings"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|warning| warning.as_str().unwrap().contains("volume above 127"))
                .count(),
            1
        );
        assert!(
            tracks[1]
                .iter()
                .any(|event| event.status == 0x90 && event.data[0] == 60)
        );
        assert!(
            tracks[1]
                .iter()
                .any(|event| event.status == 0x80 && event.data[0] == 60)
        );
    }
    Ok(())
}

#[test]
fn high_non_volume_controls_remain_unrepresentable() {
    for opcode in [0xBF, 0xC0, 0xC1, 0xC8] {
        let (bytes, song) = fixture(&[&[0xBD, 0, opcode, 128, 0xD3, 60, 100, 0x84, 0xB1]], false);
        assert!(export(&bytes, &song, 300).is_err());
    }
}

#[test]
fn mp2k_playback_gain_changes_only_velocity_and_cc7() -> Result<()> {
    let (bytes, song) = fixture(
        &[&[
            0xBD, 0, 0xBE, 32, 0xBF, 32, 0xC0, 32, 0xC1, 32, 0xD3, 60, 32, 0x84, 0xB1,
        ]],
        false,
    );
    let raw = parse(&export_with_gain(&bytes, &song, 300, PlaybackGain::Raw)?);
    let shaped = parse(&export_with_gain(
        &bytes,
        &song,
        300,
        PlaybackGain::Mp2kAmplitude,
    )?);
    let channel_events = |tracks: &[Vec<ParsedEvent>]| {
        tracks[1]
            .iter()
            .filter(|event| event.status & 0x80 != 0)
            .map(|event| (event.tick, event.status, event.data.clone()))
            .collect::<Vec<_>>()
    };
    let raw_events = channel_events(&raw);
    let shaped_events = channel_events(&shaped);
    assert_eq!(raw_events.len(), shaped_events.len());
    for (raw, shaped) in raw_events.iter().zip(&shaped_events) {
        assert_eq!(raw.0, shaped.0);
        assert_eq!(raw.1, shaped.1);
        let expected = match (raw.1 & 0xF0, raw.2.as_slice()) {
            (0x90, [key, velocity]) => vec![*key, PlaybackGain::Mp2kAmplitude.map(*velocity)],
            (0xB0, [7, volume]) => vec![7, PlaybackGain::Mp2kAmplitude.map(*volume)],
            _ => raw.2.clone(),
        };
        assert_eq!(shaped.2, expected);
    }
    assert!(
        raw_events
            .iter()
            .any(|event| event.1 == 0x90 && event.2 == [60, 32])
    );
    assert!(
        shaped_events
            .iter()
            .any(|event| event.1 == 0x90 && event.2 == [60, 64])
    );
    assert!(
        raw_events
            .iter()
            .any(|event| event.1 == 0xB0 && event.2 == [7, 32])
    );
    assert!(
        shaped_events
            .iter()
            .any(|event| event.1 == 0xB0 && event.2 == [7, 64])
    );

    let metadata: Value = serde_json::from_slice(
        &shaped[0]
            .iter()
            .find(|event| event.meta == Some(1))
            .unwrap()
            .data,
    )?;
    assert_eq!(metadata["midi_options"]["playback_gain"], "mp2k");
    Ok(())
}

#[test]
fn sequence_midi_preserves_notes_controllers_tempo_ties_and_provenance() -> Result<()> {
    let (bytes, song) = fixture(
        &[&[
            0xBD, 0, 0xBB, 75, 0xBE, 100, 0xBF, 32, 0xBC, 12, 0xC8, 127, 0xC0, 80, 0xC1, 12, 0xD3,
            60, 96, 0x84, 0xD3, 62, 80, 0x84, 0xCF, 64, 127, 0x84, 0xCE, 64, 0xB1,
        ]],
        false,
    );
    let output = export(&bytes, &song, 300)?;
    assert_eq!(output, export(&bytes, &song, 300)?);
    let tracks = parse(&output);
    assert_eq!(tracks.len(), 2);
    let metadata: Value = serde_json::from_slice(
        &tracks[0]
            .iter()
            .find(|event| event.meta == Some(1))
            .unwrap()
            .data,
    )?;
    assert_eq!(metadata["media"]["sha256"], "fixture-sha256");
    assert_eq!(metadata["title"], "音");
    let notes = tracks[1]
        .iter()
        .filter(|event| matches!(event.status & 0xF0, 0x80 | 0x90))
        .map(|event| (event.tick, event.status, event.data[0], event.data[1]))
        .collect::<Vec<_>>();
    assert_eq!(
        notes,
        [
            (0, 0x90, 60, 96),
            (8, 0x80, 60, 0),
            (8, 0x90, 62, 80),
            (16, 0x80, 62, 0),
            (16, 0x90, 64, 127),
            (24, 0x80, 64, 0)
        ]
    );
    for expected in [[7, 100], [10, 32], [6, 76], [6, 127], [6, 12]] {
        assert!(
            tracks[1]
                .iter()
                .any(|event| event.status == 0xB0 && event.data == expected)
        );
    }
    assert!(
        tracks[1]
            .iter()
            .any(|event| event.status == 0xE0 && event.data == [0, 80])
    );
    let decoded = rustysynth::MidiFile::new(&mut Cursor::new(output))?;
    let expected = 12.0 * 280_896.0 / 16_777_216.0;
    assert!((decoded.get_length() - expected).abs() < 0.00002);
    Ok(())
}

#[test]
fn conductor_resolves_simultaneous_tempo_in_source_track_order() -> Result<()> {
    let (bytes, song) = fixture(
        &[
            &[
                0xBD, 0, 0xBB, 75, 0xBE, 100, 0xD3, 60, 100, 0x84, 0xBB, 60, 0x84, 0xB1,
            ],
            &[
                0xBD, 0, 0xBE, 100, 0xD3, 64, 100, 0x84, 0xBB, 150, 0x84, 0xB1,
            ],
        ],
        false,
    );
    let output = export(&bytes, &song, 300)?;
    let tracks = parse(&output);
    let tempos = tracks[0]
        .iter()
        .filter(|event| event.meta == Some(0x51))
        .map(|event| {
            (
                event.tick,
                u32::from_be_bytes([0, event.data[0], event.data[1], event.data[2]]),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tempos,
        [
            (0, tempo_us(75, 1)?),
            (0, tempo_us(75, 1)?),
            (8, tempo_us(60, 1)?),
            (8, tempo_us(150, 1)?)
        ]
    );
    let decoded = rustysynth::MidiFile::new(&mut Cursor::new(output))?;
    assert!((decoded.get_length() - 6.0 * 280_896.0 / 16_777_216.0).abs() < 0.00002);
    Ok(())
}

#[test]
fn unresolved_custom_timbres_do_not_block_decoded_sequence_midi() -> Result<()> {
    let (bytes, song) = fixture(&[&[0xBD, 0, 0xBE, 100, 0xD3, 60, 100, 0x84, 0xB1]], true);
    assert!(!song.warnings.is_empty());
    let output = export(&bytes, &song, 300)?;
    let parsed = parse(&output);
    assert!(parsed[1].iter().any(|event| event.status == 0x90));
    let metadata: Value = serde_json::from_slice(
        &parsed[0]
            .iter()
            .find(|event| event.meta == Some(1))
            .unwrap()
            .data,
    )?;
    assert!(
        metadata["projection_warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|warning| warning.as_str().unwrap().contains("unresolved"))
    );
    Ok(())
}

#[test]
fn very_slow_tempo_stays_valid_and_duration_limit_releases_tied_notes() -> Result<()> {
    let (bytes, song) = fixture(
        &[&[
            0xBD, 0, 0xBB, 1, 0xBE, 100, 0xCF, 60, 100, 0xB0, 0xCE, 60, 0xB1,
        ]],
        false,
    );
    let output = export(&bytes, &song, 1)?;
    let parsed = parse(&output);
    let note_off = parsed[1].iter().find(|event| event.status == 0x80).unwrap();
    assert!(note_off.tick > 0);
    assert_eq!(note_off.tick, parsed[1].last().unwrap().tick);
    let decoded = rustysynth::MidiFile::new(&mut Cursor::new(output))?;
    assert!((0.5..=1.0).contains(&decoded.get_length()));
    Ok(())
}

#[test]
fn zero_velocity_note_does_not_release_another_tied_note() -> Result<()> {
    let (bytes, song) = fixture(
        &[&[
            0xBD, 0, 0xBE, 100, 0xCF, 60, 100, 0x81, 0xD0, 60, 0, 0x82, 0xCE, 60, 0xB1,
        ]],
        false,
    );
    let parsed = parse(&export(&bytes, &song, 300)?);
    let notes = parsed[1]
        .iter()
        .filter(|event| matches!(event.status & 0xF0, 0x80 | 0x90))
        .map(|event| (event.tick, event.status))
        .collect::<Vec<_>>();
    assert_eq!(notes, [(0, 0x90), (6, 0x80)]);
    Ok(())
}

#[test]
fn eot_releases_newest_matching_voice_even_when_it_has_a_gate() -> Result<()> {
    let (bytes, song) = fixture(
        &[&[
            0xBD, 0, 0xBE, 100, 0xCF, 60, 100, 0x81, 0xD2, 60, 100, 0x81, 0xCE, 60, 0x83, 0xCE, 60,
            0x81, 0xB1,
        ]],
        false,
    );
    let parsed = parse(&export(&bytes, &song, 300)?);
    let notes = parsed[1]
        .iter()
        .filter(|event| matches!(event.status & 0xF0, 0x80 | 0x90))
        .map(|event| (event.tick, event.status))
        .collect::<Vec<_>>();
    assert_eq!(notes, [(0, 0x90), (2, 0x90), (4, 0x80), (10, 0x80)]);
    Ok(())
}

#[test]
fn silent_newest_voice_consumes_eot_without_sending_midi_note_off() -> Result<()> {
    let (bytes, song) = fixture(
        &[&[
            0xBD, 0, 0xBE, 100, 0xCF, 60, 100, 0x81, 0xD2, 60, 0, 0xCE, 60, 0x83, 0xCE, 60, 0x81,
            0xB1,
        ]],
        false,
    );
    let parsed = parse(&export(&bytes, &song, 300)?);
    let notes = parsed[1]
        .iter()
        .filter(|event| matches!(event.status & 0xF0, 0x80 | 0x90))
        .map(|event| (event.tick, event.status))
        .collect::<Vec<_>>();
    assert_eq!(notes, [(0, 0x90), (8, 0x80)]);
    Ok(())
}

#[test]
fn default_channel_routing_skips_ten_and_moves_the_sixteenth_track_to_port_one() -> Result<()> {
    let routes = (0..16)
        .map(|source| channel_route(source, true))
        .collect::<Result<Vec<_>>>()?;
    assert_eq!(
        routes,
        [
            (0, 0),
            (1, 0),
            (2, 0),
            (3, 0),
            (4, 0),
            (5, 0),
            (6, 0),
            (7, 0),
            (8, 0),
            (10, 0),
            (11, 0),
            (12, 0),
            (13, 0),
            (14, 0),
            (15, 0),
            (0, 1),
        ]
    );
    assert_eq!(channel_route(9, false)?, (9, 0));
    assert!(channel_route(16, true).is_err());
    let sixteenth = ChannelTrack::new(15, true, BankSelect::Gs, PlaybackGain::Raw)?;
    assert!(
        sixteenth
            .writer
            .data
            .windows(5)
            .any(|bytes| bytes == [0, 0xFF, 0x21, 1, 1].as_slice())
    );
    Ok(())
}

#[test]
fn bank_select_conventions_emit_the_vgmtrans_controller_layouts() -> Result<()> {
    assert_eq!(bank_select_messages(12, BankSelect::Gs)?, vec![(0, 12)]);
    assert_eq!(
        bank_select_messages(0x123, BankSelect::Mma)?,
        vec![(0, 2), (32, 0x23)]
    );
    assert!(bank_select_messages(128, BankSelect::Gs).is_err());
    assert!(bank_select_messages(0x4000, BankSelect::Mma).is_err());
    Ok(())
}

#[test]
fn requested_large_duration_is_accepted() -> Result<()> {
    let mut sequence = vec![0xBD, 0, 0xBB, 1, 0xBE, 100, 0xCF, 60, 100];
    sequence.extend(std::iter::repeat_n(0xB0, 8));
    sequence.extend([0xCE, 60, 0xB1]);
    let (bytes, song) = fixture(&[&sequence], false);
    let long = export(&bytes, &song, 7200)?;
    let clipped = export(&bytes, &song, 300)?;
    let full = rustysynth::MidiFile::new(&mut Cursor::new(long))?;
    let clipped = rustysynth::MidiFile::new(&mut Cursor::new(clipped))?;
    assert!(full.get_length() > 600.0);
    assert!(clipped.get_length() <= 300.0 && clipped.get_length() > 290.0);
    Ok(())
}

#[test]
fn malformed_controls_and_cancelled_exports_produce_no_midi() {
    let (bytes, song) = fixture(&[&[0xBD, 0, 0xBF, 200, 0xD0, 60, 100, 0x81, 0xB1]], false);
    assert!(export(&bytes, &song, 300).is_err());
    assert!(
        encode(
            &song,
            &bytes,
            MidiOptions {
                loops: 2,
                max_seconds: 300,
                skip_channel10: true,
                bank_select: BankSelect::Gs,
                playback_gain: PlaybackGain::Raw,
            },
            json!({}),
            &AtomicBool::new(true),
            &AtomicU32::new(0)
        )
        .is_err()
    );
    let mut data = Vec::new();
    for value in [0, 127, 128, 16383, 16384, MAX_VLQ] {
        data.clear();
        write_vlq(&mut data, value).unwrap();
        assert!(data.len() <= 4);
        assert_eq!(read_vlq(&data, &mut 0), value);
    }
    assert!(write_vlq(&mut data, MAX_VLQ + 1).is_err());
}
