use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};

use super::{Budget, GbSong, MAX_FRAMES, sequence};

const APPROXIMATION: &str = "GB banked-driver MIDI projection: frame-quantized note timing and static hardware-frequency offsets; General MIDI pulse/wave and drum approximations. Duty, envelopes, wave samples, vibrato, noise recipes, runtime SFX and mono/stereo game settings are not reproduced. Stereo commands are interpreted as stereo enabled. Mapped structures are not a complete sound bank.";

pub struct GbMidi {
    pub bytes: Vec<u8>,
}

pub fn midi(
    bytes: &[u8],
    song: &GbSong,
    loops: u8,
    max_seconds: u16,
    cancel: &AtomicBool,
) -> Result<GbMidi> {
    ensure!(
        (1..=8).contains(&loops),
        "Game Boy music loop count must be 1 through 8"
    );
    ensure!(
        (1..=7200).contains(&max_seconds),
        "Game Boy music duration must be 1 through 7200 seconds"
    );
    ensure!(
        song.midi_exportable,
        "Game Boy music has unresolved commands"
    );
    super::validate_song(bytes, song, cancel)?;
    encode(bytes, song, loops, max_seconds, cancel)
}

pub(crate) fn encode(
    bytes: &[u8],
    song: &GbSong,
    loops: u8,
    max_seconds: u16,
    cancel: &AtomicBool,
) -> Result<GbMidi> {
    let max_frames = (u64::from(max_seconds) * 4_194_304 / 70_224) as u32;
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let program = sequence::interpret(
        bytes,
        song.clone(),
        loops,
        max_frames.min(MAX_FRAMES),
        &mut budget,
    )
    .map_err(sequence::error)?;
    ensure!(
        program.song.warnings.is_empty(),
        "Game Boy music expansion encountered unresolved commands"
    );
    let end_frame = program
        .song
        .channels
        .iter()
        .map(|channel| channel.end_frame)
        .max()
        .unwrap_or(0);
    let metadata = serde_json::json!({
        "schema": "zeff-gb-music-midi/1", "profile": song.profile, "song_index": song.index,
        "source_sha256": const_hex::encode(zeff_firmware::sha256_bytes(bytes)),
        "source_bank": song.bank, "source_cpu_address": song.cpu_address,
        "source_offset": song.header.offset, "passes": loops, "max_seconds": max_seconds,
        "duration_frames": end_frame, "truncated": program.truncated,
        "source_frame_seconds": "70224/4194304", "limitations": APPROXIMATION,
    });
    let mut conductor = Vec::new();
    meta(&mut conductor, 0, 0x03, song.title.as_bytes());
    meta(&mut conductor, 0, 0x01, metadata.to_string().as_bytes());
    // One MIDI tick is one GB frame, rounded within 1/1024 microsecond.
    meta(&mut conductor, 0, 0x51, &[0x41, 0x66, 0xb5]);
    meta(&mut conductor, end_frame, 0x2f, &[]);
    let mut tracks = vec![conductor];
    for channel in &program.song.channels {
        ensure!(
            !cancel.load(Ordering::Relaxed),
            "Game Boy MIDI export cancelled"
        );
        let midi_channel = if channel.number == 4 {
            9
        } else {
            channel.number - 1
        };
        let mut events = vec![(
            0,
            1,
            vec![
                0xc0 | midi_channel,
                if channel.number == 3 { 81 } else { 80 },
            ],
        )];
        if channel.number != 4 {
            for (control, value) in [(101, 0), (100, 0), (6, 2), (38, 0), (101, 127), (100, 127)] {
                events.push((0, 1, vec![0xb0 | midi_channel, control, value]));
            }
        }
        for note in program
            .notes
            .iter()
            .filter(|note| note.channel == channel.number)
        {
            budget.charge().map_err(sequence::error)?;
            let (key, bend) = if let Some((kit, instrument)) = note.drum {
                let Some(key) = drum_key(kit, instrument) else {
                    continue;
                };
                (key, 8192)
            } else {
                let clock = if channel.number == 3 {
                    65_536.0
                } else {
                    131_072.0
                };
                let frequency = clock / f64::from(2048 - note.frequency);
                let pitch = 69.0 + 12.0 * (frequency / 440.0).log2();
                let key = pitch.round();
                ensure!(
                    (0.0..=127.0).contains(&key),
                    "Game Boy pitch exceeds MIDI note range"
                );
                (
                    key as u8,
                    (8192.0 + (pitch - key) * 4096.0)
                        .round()
                        .clamp(0.0, 16383.0) as u16,
                )
            };
            events.push((note.frame, 1, vec![0xb0 | midi_channel, 10, note.pan]));
            if channel.number != 4 {
                events.push((
                    note.frame,
                    1,
                    vec![0xe0 | midi_channel, (bend & 127) as u8, (bend >> 7) as u8],
                ));
            }
            if note.volume != 0 {
                events.push((note.frame, 2, vec![0x90 | midi_channel, key, note.volume]));
                events.push((
                    note.frame + note.duration,
                    0,
                    vec![0x80 | midi_channel, key, 0],
                ));
            }
        }
        if let Some(frame) = channel.loop_start_frame {
            let mut marker = Vec::new();
            meta(&mut marker, 0, 0x06, b"Source loop start");
            events.push((frame, 1, marker[1..].to_vec()));
        }
        events.sort_by_key(|(frame, priority, _)| (*frame, *priority));
        let mut data = Vec::new();
        meta(
            &mut data,
            0,
            0x03,
            format!("GB channel {}", channel.number).as_bytes(),
        );
        let mut last = 0;
        for (frame, _, event) in events {
            vlq(&mut data, frame - last);
            data.extend(event);
            last = frame;
        }
        meta(&mut data, channel.end_frame.saturating_sub(last), 0x2f, &[]);
        tracks.push(data);
    }
    let mut output = b"MThd\0\0\0\x06\0\x01".to_vec();
    output.extend((tracks.len() as u16).to_be_bytes());
    output.extend(256u16.to_be_bytes());
    for track in tracks {
        output.extend(b"MTrk");
        output.extend((track.len() as u32).to_be_bytes());
        output.extend(track);
    }
    ensure!(
        !cancel.load(Ordering::Relaxed),
        "Game Boy MIDI export cancelled"
    );
    Ok(GbMidi { bytes: output })
}

fn drum_key(kit: u8, instrument: u8) -> Option<u8> {
    const KITS: [[u8; 12]; 6] = [
        [38, 38, 38, 38, 36, 76, 77, 42, 38, 38, 38, 46],
        [42, 38, 38, 38, 46, 42, 38, 76, 77, 38, 38, 38],
        [38, 38, 38, 38, 36, 76, 77, 42, 38, 38, 38, 46],
        [38, 38, 38, 36, 76, 36, 42, 42, 42, 0, 36, 49],
        [36, 38, 38, 36, 42, 76, 42, 42, 42, 42, 36, 49],
        [38, 38, 38, 42, 42, 42, 36, 76, 49, 38, 38, 36],
    ];
    let key = KITS[usize::from(kit)][usize::from(instrument - 1)];
    (key != 0).then_some(key)
}

fn meta(data: &mut Vec<u8>, delta: u32, kind: u8, value: &[u8]) {
    vlq(data, delta);
    data.extend([0xff, kind]);
    vlq(data, value.len() as u32);
    data.extend(value);
}

fn vlq(data: &mut Vec<u8>, mut value: u32) {
    let mut bytes = [0; 4];
    let mut index = 3;
    bytes[index] = (value & 127) as u8;
    while {
        value >>= 7;
        value != 0
    } {
        index -= 1;
        bytes[index] = (value & 127) as u8 | 128;
    }
    data.extend(&bytes[index..]);
}
