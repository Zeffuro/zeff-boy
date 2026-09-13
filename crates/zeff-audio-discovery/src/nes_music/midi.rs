use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};

use super::{Budget, MAX_FRAMES, NesSong, sequence};

const APPROXIMATION: &str = "NES queue-driver MIDI projection: isolated normal-speed NTSC music, frame-quantized note scheduling, static APU timer pitches, and General MIDI pulse/triangle/drum approximations. Pulse envelopes and duty, triangle linear/length counters, noise synthesis, DMC DAC mix bias, runtime SFX, pause and prior area/fast-tempo state are not reproduced. Mapped structures are not a complete sound bank.";

pub struct NesMidi {
    pub bytes: Vec<u8>,
}

pub fn midi(
    bytes: &[u8],
    song: &NesSong,
    loops: u8,
    max_seconds: u16,
    cancel: &AtomicBool,
) -> Result<NesMidi> {
    ensure!(
        (1..=8).contains(&loops),
        "NES music loop count must be 1 through 8"
    );
    ensure!(
        (1..=7200).contains(&max_seconds),
        "NES music duration must be 1 through 7200 seconds"
    );
    ensure!(
        song.midi_exportable,
        "NES music has unresolved synthesis or runtime context"
    );
    super::validate_song(bytes, song, cancel)?;
    encode(bytes, song, loops, max_seconds, cancel)
}

pub(crate) fn encode(
    bytes: &[u8],
    song: &NesSong,
    loops: u8,
    max_seconds: u16,
    cancel: &AtomicBool,
) -> Result<NesMidi> {
    let max_frames = (u64::from(max_seconds) * 39_375_000 / 655_171) as u32;
    let mut budget = Budget {
        cancel,
        remaining: 2_000_000,
    };
    let program = sequence::interpret(
        bytes,
        song.index,
        loops,
        max_frames.min(MAX_FRAMES),
        &mut budget,
    )
    .map_err(sequence::error)?;
    ensure!(
        program.song.warnings.is_empty(),
        "NES music expansion encountered unresolved synthesis or runtime context"
    );
    let end_frame = program
        .song
        .channels
        .iter()
        .map(|channel| channel.end_frame)
        .max()
        .unwrap_or(0);
    let metadata = serde_json::json!({
        "schema": "zeff-nes-music-midi/1", "profile": song.profile, "song_index": song.index,
        "queue": song.queue, "selector": song.selector,
        "source_sha256": const_hex::encode(zeff_firmware::sha256_bytes(bytes)),
        "source_cpu_address": song.cpu_address, "source_offset": song.header.offset,
        "passes": loops, "max_seconds": max_seconds, "duration_frames": end_frame,
        "truncated": program.truncated, "source_frame_seconds": "655171/39375000",
        "source_cpu_hz": "39375000/22", "sections": program.song.sections,
        "limitations": APPROXIMATION,
    });
    let mut conductor = Vec::new();
    meta(&mut conductor, 0, 0x03, song.title.as_bytes());
    meta(&mut conductor, 0, 0x01, metadata.to_string().as_bytes());
    // One tick is an average rendered NTSC frame, rounded within 1/256 microsecond.
    meta(&mut conductor, 0, 0x51, &[0x40, 0xff, 0x43]);
    meta(&mut conductor, end_frame, 0x2f, &[]);
    let mut tracks = vec![conductor];
    for channel in &program.song.channels {
        ensure!(!cancel.load(Ordering::Relaxed), "NES MIDI export cancelled");
        let midi_channel = if channel.number == 4 {
            9
        } else {
            channel.number - 1
        };
        let mut events = Vec::new();
        if channel.number != 4 {
            events.push((
                0,
                1,
                vec![
                    0xc0 | midi_channel,
                    if channel.number == 3 { 81 } else { 80 },
                ],
            ));
            for (control, value) in [(101, 0), (100, 0), (6, 2), (38, 0), (101, 127), (100, 127)] {
                events.push((0, 1, vec![0xb0 | midi_channel, control, value]));
            }
        }
        for note in program
            .notes
            .iter()
            .filter(|note| note.channel == channel.number && note.duration != 0)
        {
            budget.charge().map_err(sequence::error)?;
            let (key, bend) = if let Some(drum) = note.drum {
                (drum, 8192)
            } else {
                let divisor = if channel.number == 3 { 32.0 } else { 16.0 };
                let frequency = (39_375_000.0 / 22.0) / (divisor * f64::from(note.timer + 1));
                let pitch = 69.0 + 12.0 * (frequency / 440.0).log2();
                let key = pitch.round();
                ensure!(
                    (0.0..=127.0).contains(&key),
                    "NES pitch exceeds MIDI note range"
                );
                (
                    key as u8,
                    (8192.0 + (pitch - key) * 4096.0)
                        .round()
                        .clamp(0.0, 16383.0) as u16,
                )
            };
            if channel.number != 4 {
                events.push((
                    note.frame,
                    1,
                    vec![0xe0 | midi_channel, (bend & 127) as u8, (bend >> 7) as u8],
                ));
            }
            events.push((note.frame, 2, vec![0x90 | midi_channel, key, 100]));
            events.push((
                note.frame + note.duration,
                0,
                vec![0x80 | midi_channel, key, 0],
            ));
        }
        if let Some(frame) = channel.loop_start_frame.filter(|&frame| frame <= end_frame) {
            let mut marker = Vec::new();
            meta(&mut marker, 0, 0x06, b"Source loop start");
            events.push((frame, 1, marker[1..].to_vec()));
        }
        events.sort_by_key(|(frame, priority, _)| (*frame, *priority));
        let mut data = Vec::new();
        let name = match channel.number {
            1 => "NES pulse 1",
            2 => "NES pulse 2",
            3 => "NES triangle",
            _ => "NES noise",
        };
        meta(&mut data, 0, 0x03, name.as_bytes());
        let mut last = 0;
        for (frame, _, event) in events {
            vlq(&mut data, frame - last);
            data.extend(event);
            last = frame;
        }
        meta(&mut data, end_frame.saturating_sub(last), 0x2f, &[]);
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
    ensure!(!cancel.load(Ordering::Relaxed), "NES MIDI export cancelled");
    Ok(NesMidi { bytes: output })
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
