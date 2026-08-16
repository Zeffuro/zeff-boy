use std::path::PathBuf;

use crate::audio_tooling::{AudioChannelId, AudioSemanticFrame, AudioVoiceClass, AudioVoiceState};

pub(super) const MIDI_TICKS_PER_QUARTER: u16 = 960;
pub(super) const MIDI_TICKS_PER_FRAME: u32 = 16;

pub(super) fn hz_to_midi_note(hz: f64) -> u8 {
    if hz <= 0.0 {
        return 0;
    }
    let note = 69.0 + 12.0 * (hz / 440.0).log2();
    (note.round() as i32).clamp(0, 127) as u8
}

pub(super) fn finish_midi(
    path: PathBuf,
    frames: &[AudioSemanticFrame],
) -> std::io::Result<PathBuf> {
    if frames.is_empty() {
        std::fs::write(&path, [])?;
        return Ok(path);
    }

    let tempo_us = frames[0].tempo_us_per_beat;
    let exported_voices = exportable_voices(frames);
    let track_data = exported_voices
        .iter()
        .enumerate()
        .map(|(index, voice)| {
            build_midi_track_for_voice(frames, voice.channel, midi_channel(index))
        })
        .collect::<Vec<_>>();

    let mut smf = Vec::with_capacity(frames.len() * 16);

    smf.extend_from_slice(b"MThd");
    smf.extend_from_slice(&6u32.to_be_bytes());
    smf.extend_from_slice(&1u16.to_be_bytes());
    smf.extend_from_slice(&((track_data.len() as u16) + 1).to_be_bytes());
    smf.extend_from_slice(&MIDI_TICKS_PER_QUARTER.to_be_bytes());

    let tempo_track = build_tempo_track(tempo_us);
    smf.extend_from_slice(b"MTrk");
    smf.extend_from_slice(&(tempo_track.len() as u32).to_be_bytes());
    smf.extend_from_slice(&tempo_track);

    for track in &track_data {
        smf.extend_from_slice(b"MTrk");
        smf.extend_from_slice(&(track.len() as u32).to_be_bytes());
        smf.extend_from_slice(track);
    }

    std::fs::write(&path, &smf)?;
    Ok(path)
}

fn build_tempo_track(tempo_us: u32) -> Vec<u8> {
    let mut data = Vec::new();

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x51);
    data.push(0x03);
    data.push((tempo_us >> 16) as u8);
    data.push((tempo_us >> 8) as u8);
    data.push(tempo_us as u8);

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x2F);
    data.push(0x00);

    data
}

fn exportable_voices(frames: &[AudioSemanticFrame]) -> Vec<AudioVoiceState> {
    let Some(first) = frames.first() else {
        return Vec::new();
    };

    first
        .voices
        .iter()
        .filter(|voice| !matches!(voice.class, AudioVoiceClass::Noise | AudioVoiceClass::Pcm))
        .filter(|voice| {
            frames
                .iter()
                .filter_map(|frame| voice_in_frame(frame, voice.channel))
                .any(|state| state.pitch_hz.is_some())
        })
        .cloned()
        .collect()
}

fn midi_channel(track_index: usize) -> u8 {
    let channel = track_index.min(14) as u8;
    if channel >= 9 { channel + 1 } else { channel }
}

fn midi_program_for_voice(class: AudioVoiceClass) -> Option<u8> {
    match class {
        AudioVoiceClass::Pulse | AudioVoiceClass::Tone => Some(80),
        AudioVoiceClass::Triangle | AudioVoiceClass::Wavetable => Some(81),
        AudioVoiceClass::Noise | AudioVoiceClass::Pcm => None,
    }
}

struct FrameChannelState {
    note: u8,
    velocity: u8,
    enabled: bool,
}

pub(super) fn build_midi_track_for_voice(
    frames: &[AudioSemanticFrame],
    channel: AudioChannelId,
    midi_ch: u8,
) -> Vec<u8> {
    let mut data = Vec::new();
    let descriptor = frames
        .iter()
        .find_map(|frame| voice_in_frame(frame, channel))
        .cloned();
    let name = descriptor
        .as_ref()
        .map(|voice| voice.name)
        .unwrap_or("Audio Voice");

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x03);
    write_vlq(&mut data, name.len() as u32);
    data.extend_from_slice(name.as_bytes());

    if let Some(program) = descriptor
        .as_ref()
        .and_then(|voice| midi_program_for_voice(voice.class))
    {
        write_vlq(&mut data, 0);
        data.push(0xC0 | midi_ch);
        data.push(program);
    }

    let mut current_note: Option<u8> = None;
    let mut current_velocity: u8 = 0;
    let mut pending_delta: u32 = 0;

    for frame in frames {
        let state = frame_channel_state(frame, channel);
        let should_sound = state.enabled && state.velocity > 0;

        if should_sound {
            if let Some(prev_note) = current_note {
                if prev_note != state.note {
                    write_note_off(&mut data, midi_ch, pending_delta, prev_note);
                    pending_delta = 0;
                    write_note_on(&mut data, midi_ch, 0, state.note, state.velocity);

                    current_note = Some(state.note);
                    current_velocity = state.velocity;
                } else if state.velocity != current_velocity {
                    // Keep MIDI note-only; exact volume changes stay semantic.
                    current_velocity = state.velocity;
                }
            } else {
                write_note_on(
                    &mut data,
                    midi_ch,
                    pending_delta,
                    state.note,
                    state.velocity,
                );
                pending_delta = 0;
                current_note = Some(state.note);
                current_velocity = state.velocity;
            }
        } else if let Some(prev_note) = current_note.take() {
            write_note_off(&mut data, midi_ch, pending_delta, prev_note);
            pending_delta = 0;
            current_velocity = 0;
        }

        pending_delta = pending_delta.saturating_add(MIDI_TICKS_PER_FRAME);
    }

    if let Some(prev_note) = current_note {
        write_note_off(&mut data, midi_ch, pending_delta, prev_note);
    }

    write_vlq(&mut data, 0);
    data.push(0xFF);
    data.push(0x2F);
    data.push(0x00);

    data
}

fn write_note_on(data: &mut Vec<u8>, midi_ch: u8, delta: u32, note: u8, velocity: u8) {
    write_vlq(data, delta);
    data.push(0x90 | midi_ch);
    data.push(note);
    data.push(velocity);
}

fn write_note_off(data: &mut Vec<u8>, midi_ch: u8, delta: u32, note: u8) {
    write_vlq(data, delta);
    data.push(0x80 | midi_ch);
    data.push(note);
    data.push(0);
}

fn frame_channel_state(frame: &AudioSemanticFrame, channel: AudioChannelId) -> FrameChannelState {
    let Some(voice) = voice_in_frame(frame, channel) else {
        return FrameChannelState {
            note: 0,
            velocity: 0,
            enabled: false,
        };
    };

    let Some(pitch_hz) = voice.pitch_hz else {
        return FrameChannelState {
            note: 0,
            velocity: voice.level_velocity(),
            enabled: false,
        };
    };

    FrameChannelState {
        note: hz_to_midi_note(pitch_hz),
        velocity: voice.level_velocity(),
        enabled: voice.active && pitch_hz > 0.0,
    }
}

fn voice_in_frame(frame: &AudioSemanticFrame, channel: AudioChannelId) -> Option<&AudioVoiceState> {
    frame.voices.iter().find(|voice| voice.channel == channel)
}

pub(super) fn write_vlq(buf: &mut Vec<u8>, mut value: u32) {
    if value == 0 {
        buf.push(0);
        return;
    }

    let mut bytes = [0u8; 4];
    let mut count = 0;

    while value > 0 {
        bytes[count] = (value & 0x7F) as u8;
        value >>= 7;
        count += 1;
    }

    for i in (1..count).rev() {
        buf.push(bytes[i] | 0x80);
    }
    buf.push(bytes[0]);
}
