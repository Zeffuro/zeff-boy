use super::midi::*;

use crate::audio_tooling::{
    AudioChannelId, AudioSemanticFrame, AudioVoiceClass, AudioVoiceState, GB_TEMPO_US_PER_BEAT,
    NTSC_60_TEMPO_US_PER_BEAT,
};

fn voice_frame(
    frame: u64,
    channel: u16,
    name: &'static str,
    class: AudioVoiceClass,
    active: bool,
    pitch_level: (Option<f64>, f32),
) -> AudioSemanticFrame {
    AudioSemanticFrame {
        frame,
        tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
        voices: vec![AudioVoiceState {
            channel: AudioChannelId(channel),
            name,
            class,
            active,
            pitch_hz: pitch_level.0,
            level: Some(pitch_level.1),
        }],
    }
}

fn mixed_frame(frame: u64, tone_hz: f64, noise_active: bool) -> AudioSemanticFrame {
    AudioSemanticFrame {
        frame,
        tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
        voices: vec![
            AudioVoiceState {
                channel: AudioChannelId(0),
                name: "Sega PSG Tone 0",
                class: AudioVoiceClass::Tone,
                active: true,
                pitch_hz: Some(tone_hz),
                level: Some(1.0),
            },
            AudioVoiceState {
                channel: AudioChannelId(3),
                name: "Sega PSG Noise",
                class: AudioVoiceClass::Noise,
                active: noise_active,
                pitch_hz: None,
                level: Some(1.0),
            },
        ],
    }
}

fn mixed_frame_with_pcm(frame: u64, tone_hz: f64) -> AudioSemanticFrame {
    AudioSemanticFrame {
        frame,
        tempo_us_per_beat: NTSC_60_TEMPO_US_PER_BEAT,
        voices: vec![
            AudioVoiceState {
                channel: AudioChannelId(0),
                name: "GBA PSG 1 (Square + Sweep)",
                class: AudioVoiceClass::Pulse,
                active: true,
                pitch_hz: Some(tone_hz),
                level: Some(1.0),
            },
            AudioVoiceState {
                channel: AudioChannelId(4),
                name: "GBA FIFO A",
                class: AudioVoiceClass::Pcm,
                active: true,
                pitch_hz: None,
                level: Some(0.5),
            },
        ],
    }
}

fn read_vlq(bytes: &[u8], i: &mut usize) -> u32 {
    let mut value = 0u32;
    loop {
        let b = bytes[*i];
        *i += 1;
        value = (value << 7) | u32::from(b & 0x7F);
        if b & 0x80 == 0 {
            break;
        }
    }
    value
}

fn ch0_note_events(track: &[u8]) -> Vec<(u32, u8, u8)> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < track.len() {
        let delta = read_vlq(track, &mut i);
        if i >= track.len() {
            break;
        }
        let status = track[i];
        i += 1;
        match status {
            0x90 | 0x80 => {
                if i + 1 >= track.len() {
                    break;
                }
                let note = track[i];
                let vel = track[i + 1];
                i += 2;
                out.push((delta, status, note));
                let _ = vel;
            }
            0xA0 => {
                i += 2;
            }
            0xC0 => {
                i += 1;
            }
            0xFF => {
                if i >= track.len() {
                    break;
                }
                let _meta = track[i];
                i += 1;
                let len = read_vlq(track, &mut i) as usize;
                i = i.saturating_add(len);
            }
            _ => break,
        }
    }
    out
}

fn midi_header_track_count(data: &[u8]) -> u16 {
    u16::from_be_bytes([data[10], data[11]])
}

fn midi_header_division(data: &[u8]) -> u16 {
    u16::from_be_bytes([data[12], data[13]])
}

fn midi_tempo_us_per_beat(data: &[u8]) -> u32 {
    assert_eq!(&data[14..18], b"MTrk");
    assert_eq!(&data[22..26], &[0x00, 0xFF, 0x51, 0x03]);
    (u32::from(data[26]) << 16) | (u32::from(data[27]) << 8) | u32::from(data[28])
}

#[test]
fn hz_to_midi_note_a4() {
    assert_eq!(hz_to_midi_note(440.0), 69);
}

#[test]
fn hz_to_midi_note_c4() {
    assert_eq!(hz_to_midi_note(261.63), 60);
}

#[test]
fn hz_to_midi_note_zero_returns_zero() {
    assert_eq!(hz_to_midi_note(0.0), 0);
}

#[test]
fn write_vlq_zero() {
    let mut buf = Vec::new();
    write_vlq(&mut buf, 0);
    assert_eq!(buf, vec![0]);
}

#[test]
fn write_vlq_small() {
    let mut buf = Vec::new();
    write_vlq(&mut buf, 0x7F);
    assert_eq!(buf, vec![0x7F]);
}

#[test]
fn write_vlq_two_bytes() {
    let mut buf = Vec::new();
    write_vlq(&mut buf, 0x80);
    assert_eq!(buf, vec![0x81, 0x00]);
}

#[test]
fn finish_midi_produces_valid_smf_header() {
    let frames = vec![
        voice_frame(
            0,
            0,
            "GB CH1 (Square 1)",
            AudioVoiceClass::Pulse,
            true,
            (Some(440.0), 1.0),
        ),
        voice_frame(
            1,
            0,
            "GB CH1 (Square 1)",
            AudioVoiceClass::Pulse,
            true,
            (Some(494.0), 0.8),
        ),
    ];

    let dir = std::env::temp_dir();
    let path = dir.join("test_midi_output.mid");
    let result = finish_midi(path.clone(), &frames);
    assert!(result.is_ok());

    let data = std::fs::read(&path).unwrap();
    assert!(data.len() > 14);
    assert_eq!(&data[0..4], b"MThd");
    assert_eq!(midi_header_track_count(&data), 2);
    assert_eq!(midi_header_division(&data), MIDI_TICKS_PER_QUARTER);
    assert_eq!(&data[14..18], b"MTrk");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn finish_empty_midi_produces_valid_single_track_smf() {
    let path = std::env::temp_dir().join("test_empty_midi_output.mid");
    finish_midi(path.clone(), &[]).unwrap();

    let data = std::fs::read(&path).unwrap();
    assert_eq!(&data[0..4], b"MThd");
    assert_eq!(midi_header_track_count(&data), 1);
    assert_eq!(midi_header_division(&data), MIDI_TICKS_PER_QUARTER);
    assert_eq!(midi_tempo_us_per_beat(&data), NTSC_60_TEMPO_US_PER_BEAT);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn finish_midi_uses_semantic_frame_tempo() {
    let mut frames = vec![
        voice_frame(
            0,
            0,
            "GB CH1 (Square 1)",
            AudioVoiceClass::Pulse,
            true,
            (Some(440.0), 1.0),
        ),
        voice_frame(
            1,
            0,
            "GB CH1 (Square 1)",
            AudioVoiceClass::Pulse,
            true,
            (Some(494.0), 1.0),
        ),
    ];
    for frame in &mut frames {
        frame.tempo_us_per_beat = GB_TEMPO_US_PER_BEAT;
    }

    let dir = std::env::temp_dir();
    let path = dir.join("test_midi_timing_output.mid");
    finish_midi(path.clone(), &frames).unwrap();

    let data = std::fs::read(&path).unwrap();
    assert_eq!(midi_header_division(&data), MIDI_TICKS_PER_QUARTER);
    assert_eq!(midi_tempo_us_per_beat(&data), GB_TEMPO_US_PER_BEAT);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn midi_note_change_on_adjacent_frames_advances_time() {
    let frames = vec![
        voice_frame(
            0,
            0,
            "Voice",
            AudioVoiceClass::Pulse,
            true,
            (Some(440.0), 1.0),
        ),
        voice_frame(
            1,
            0,
            "Voice",
            AudioVoiceClass::Pulse,
            true,
            (Some(494.0), 1.0),
        ),
    ];

    let track = build_midi_track_for_voice(&frames, AudioChannelId(0), 0);
    let events = ch0_note_events(&track);
    assert!(events.len() >= 4);

    assert_eq!(events[0].0, 0);
    assert_eq!(events[0].1, 0x90);
    assert_eq!(events[1].0, MIDI_TICKS_PER_FRAME);
    assert_eq!(events[1].1, 0x80);
    assert_eq!(events[2].0, 0);
    assert_eq!(events[2].1, 0x90);
}

#[test]
fn midi_note_off_on_adjacent_frame_advances_time() {
    let frames = vec![
        voice_frame(
            0,
            0,
            "Voice",
            AudioVoiceClass::Pulse,
            true,
            (Some(440.0), 1.0),
        ),
        voice_frame(
            1,
            0,
            "Voice",
            AudioVoiceClass::Pulse,
            false,
            (Some(440.0), 1.0),
        ),
    ];

    let track = build_midi_track_for_voice(&frames, AudioChannelId(0), 0);
    let events = ch0_note_events(&track);
    assert!(events.len() >= 2);

    assert_eq!(events[0], (0, 0x90, events[0].2));
    assert_eq!(events[1], (MIDI_TICKS_PER_FRAME, 0x80, events[1].2));
}

#[test]
fn midi_new_note_after_one_silent_frame_uses_delta_one() {
    let frames = vec![
        voice_frame(
            0,
            0,
            "Voice",
            AudioVoiceClass::Pulse,
            false,
            (Some(440.0), 1.0),
        ),
        voice_frame(
            1,
            0,
            "Voice",
            AudioVoiceClass::Pulse,
            true,
            (Some(440.0), 1.0),
        ),
    ];

    let track = build_midi_track_for_voice(&frames, AudioChannelId(0), 0);
    let events = ch0_note_events(&track);
    assert!(!events.is_empty());

    assert_eq!(events[0].0, MIDI_TICKS_PER_FRAME);
    assert_eq!(events[0].1, 0x90);
}

#[test]
fn midi_omits_noise_tracks_by_default() {
    let frames = vec![mixed_frame(0, 440.0, true), mixed_frame(1, 494.0, true)];

    let dir = std::env::temp_dir();
    let path = dir.join("test_midi_noise_omit.mid");
    finish_midi(path.clone(), &frames).unwrap();

    let data = std::fs::read(&path).unwrap();
    assert_eq!(
        midi_header_track_count(&data),
        2,
        "tempo + one pitched tone; noise semantic voice is not projected to fake drums"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn midi_omits_pcm_tracks_by_default() {
    let frames = vec![
        mixed_frame_with_pcm(0, 440.0),
        mixed_frame_with_pcm(1, 494.0),
    ];

    let dir = std::env::temp_dir();
    let path = dir.join("test_midi_pcm_omit.mid");
    finish_midi(path.clone(), &frames).unwrap();

    let data = std::fs::read(&path).unwrap();
    assert_eq!(
        midi_header_track_count(&data),
        2,
        "tempo + one pitched PSG voice; PCM semantic voice is not projected to fake pitched MIDI"
    );

    let _ = std::fs::remove_file(&path);
}

#[test]
fn midi_omits_same_note_velocity_aftertouch() {
    let frames = vec![
        voice_frame(
            0,
            0,
            "Voice",
            AudioVoiceClass::Pulse,
            true,
            (Some(440.0), 1.0),
        ),
        voice_frame(
            1,
            0,
            "Voice",
            AudioVoiceClass::Pulse,
            true,
            (Some(440.0), 0.5),
        ),
    ];

    let track = build_midi_track_for_voice(&frames, AudioChannelId(0), 0);
    assert!(
        !track.contains(&0xA0),
        "hardware level changes should not be encoded as polyphonic key pressure"
    );
}
