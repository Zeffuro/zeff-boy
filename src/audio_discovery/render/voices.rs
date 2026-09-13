use anyhow::{Context, Result, bail, ensure};

use super::super::mp2k::{Event, Program};
use super::super::timeline::ScheduledEvent;
use super::super::{SongCandidate, ToneInventory};
use super::{FrameEvent, PlaybackGain, push_warning};

#[derive(Clone, Debug)]
pub(super) struct TrackControls {
    voice: Option<u8>,
    key_shift: i8,
    tune: i8,
    bend: i8,
    bend_range: u8,
    active_keys: [u8; 128],
}

impl Default for TrackControls {
    fn default() -> Self {
        Self {
            voice: None,
            key_shift: 0,
            tune: 0,
            bend: 0,
            bend_range: 2,
            active_keys: [0; 128],
        }
    }
}

pub(super) fn validate_programs(song: &SongCandidate, programs: &[Program]) -> Result<()> {
    for (track_index, program) in programs.iter().enumerate() {
        let mut voice = None;
        for timed in &program.events {
            match timed.event {
                Event::Control {
                    opcode: 0xBD,
                    value,
                } => voice = Some(value),
                Event::Control {
                    opcode: 0xBB,
                    value: 0,
                } => {
                    bail!(
                        "track {} contains a zero tempo that stops sequence time",
                        track_index + 1
                    )
                }
                Event::Control {
                    opcode: 0xBC,
                    value,
                } => {
                    let shift = value as i8;
                    ensure!(
                        (-64..=63).contains(&shift),
                        "track {} key shift {shift} exceeds RustySynth's coarse-tuning range",
                        track_index + 1
                    );
                }
                Event::Control {
                    opcode: 0xBE..=0xC1 | 0xC8,
                    value,
                } => {
                    ensure!(
                        value <= 127,
                        "track {} controller value {value} cannot be represented by this renderer",
                        track_index + 1
                    );
                }
                Event::Note { key, .. } => {
                    let selected = voice.with_context(|| {
                        format!(
                            "track {} starts note {key} before selecting an instrument",
                            track_index + 1
                        )
                    })?;
                    ensure!(
                        supports_note(song, selected, key),
                        "track {} uses unresolved instrument {selected} at key {key}",
                        track_index + 1
                    );
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn supports_note(song: &SongCandidate, voice: u8, key: u8) -> bool {
    let Some(instrument) = song
        .instruments
        .iter()
        .find(|instrument| instrument.voice == voice)
    else {
        return false;
    };
    if instrument.regions.is_empty() {
        return supports_tone(&instrument.tone);
    }
    instrument.regions.iter().any(|region| {
        region.key_start <= key
            && key <= region.key_end
            && region.warning.is_none()
            && region.tone.as_ref().is_some_and(supports_tone)
    })
}

fn supports_tone(tone: &ToneInventory) -> bool {
    match tone.kind {
        0x00 | 0x08 => tone.sample.is_some() || tone.synthesis.is_some(),
        0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => tone.sample.is_some(),
        0x01 | 0x02 | 0x04 | 0x09 | 0x0A | 0x0C => true,
        0x03 | 0x0B => tone.waveform.is_some(),
        _ => false,
    }
}

pub(super) fn validate_soundfont_presets(
    song: &SongCandidate,
    programs: &[Program],
    sound_font: &rustysynth::SoundFont,
) -> Result<()> {
    for (track, program) in programs.iter().enumerate() {
        let bank = if track == rustysynth::Synthesizer::PERCUSSION_CHANNEL {
            128
        } else {
            0
        };
        let mut selected_voice = None;
        for timed in &program.events {
            if let Event::Control {
                opcode: 0xBD,
                value,
            } = timed.event
            {
                selected_voice = Some(value);
            }
            if !matches!(timed.event, Event::Note { .. }) {
                continue;
            }
            let voice = selected_voice.context("note has no selected program")?;
            ensure!(
                sound_font.get_presets().iter().any(|preset| {
                    preset.get_bank_number() == bank
                        && preset.get_patch_number() == i32::from(voice)
                }),
                "generated SoundFont is missing bank {bank}, program {voice} required by track {}",
                track + 1
            );
        }
    }
    ensure!(
        !song.instruments.is_empty(),
        "song has no renderable instruments"
    );
    Ok(())
}

pub(super) fn dispatch(
    synth: &mut rustysynth::Synthesizer,
    song: &SongCandidate,
    timed: &FrameEvent,
    controls: &mut [TrackControls],
    playback_gain: PlaybackGain,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let channel = usize::from(timed.track);
    let state = &mut controls[channel];
    match timed.event {
        ScheduledEvent::Sequence(Event::MemoryWrite { .. }) => {
            push_warning(
                warnings,
                "Sequence memory-write commands are retained but do not modify game/player memory during offline rendering.",
            );
        }
        ScheduledEvent::Sequence(Event::ExtendedControl { .. }) => {
            push_warning(
                warnings,
                "MP2k pseudo-echo volume/length commands are present and are not reproduced by SoundFont rendering.",
            );
        }
        ScheduledEvent::GateOff { key } | ScheduledEvent::Sequence(Event::EndTie { key }) => {
            if state.active_keys[usize::from(key)] > 1 {
                push_warning(
                    warnings,
                    "Overlapping notes with the same key share RustySynth note-off timing; MP2k releases only one matching voice.",
                );
            }
            synth.note_off(channel as i32, i32::from(key));
            state.active_keys[usize::from(key)] = 0;
        }
        ScheduledEvent::Sequence(Event::Note { key, velocity, .. }) => {
            let voice = state
                .voice
                .context("note reached the renderer without an instrument")?;
            ensure!(
                supports_note(song, voice, key),
                "instrument {voice} at key {key} became unresolved"
            );
            if state.active_keys[usize::from(key)] != 0 {
                push_warning(
                    warnings,
                    "Overlapping notes with the same key share RustySynth note-off timing; MP2k releases only one matching voice.",
                );
            }
            state.active_keys[usize::from(key)] =
                state.active_keys[usize::from(key)].saturating_add(1);
            synth.note_on(
                channel as i32,
                i32::from(key),
                i32::from(playback_gain.map(velocity)),
            );
        }
        ScheduledEvent::Sequence(Event::Fine) => {
            send_cc(synth, channel, 123, 0);
            state.active_keys.fill(0);
        }
        ScheduledEvent::Sequence(Event::RuntimeSongReturn) => {
            send_cc(synth, channel, 123, 0);
            state.active_keys.fill(0);
            push_warning(
                warnings,
                "The engine may restore previously active music after this song. The standalone export ends here and does not follow runtime player state.",
            );
        }
        ScheduledEvent::Sequence(Event::Control { opcode, value }) => match opcode {
            0xBA | 0xBB => {}
            0xBC => {
                state.key_shift = value as i8;
                set_key_shift(synth, channel, state.key_shift)?;
            }
            0xBD => {
                state.voice = Some(value);
                synth.process_midi_message(channel as i32, 0xC0, i32::from(value), 0);
            }
            0xBE => send_cc(synth, channel, 7, playback_gain.map(value)),
            0xBF => send_cc(synth, channel, 10, value),
            0xC0 => {
                state.bend = (i16::from(value) - 64) as i8;
                set_bend(synth, channel, state.bend);
            }
            0xC1 => {
                state.bend_range = value;
                set_bend_range(synth, channel, value);
            }
            0xC2..=0xC5 => {}
            0xC8 => {
                state.tune = (i16::from(value) - 64) as i8;
                set_tune(synth, channel, state.tune);
            }
            _ => bail!("unsupported decoded control opcode {opcode:#04x}"),
        },
    }
    Ok(())
}

pub(super) fn send_cc(
    synth: &mut rustysynth::Synthesizer,
    channel: usize,
    controller: u8,
    value: u8,
) {
    synth.process_midi_message(
        channel as i32,
        0xB0,
        i32::from(controller),
        i32::from(value),
    );
}

fn select_rpn(synth: &mut rustysynth::Synthesizer, channel: usize, parameter: u8) {
    send_cc(synth, channel, 101, 0);
    send_cc(synth, channel, 100, parameter);
}

pub(super) fn set_key_shift(
    synth: &mut rustysynth::Synthesizer,
    channel: usize,
    shift: i8,
) -> Result<()> {
    ensure!(
        (-64..=63).contains(&shift),
        "key shift {shift} exceeds RustySynth's coarse-tuning range"
    );
    select_rpn(synth, channel, 2);
    send_cc(synth, channel, 6, (i16::from(shift) + 64) as u8);
    Ok(())
}

pub(super) fn set_tune(synth: &mut rustysynth::Synthesizer, channel: usize, tune: i8) {
    let value = 8192i32 + i32::from(tune) * 128;
    select_rpn(synth, channel, 1);
    send_cc(synth, channel, 6, (value >> 7) as u8);
    send_cc(synth, channel, 38, (value & 0x7F) as u8);
}

pub(super) fn set_bend_range(synth: &mut rustysynth::Synthesizer, channel: usize, semitones: u8) {
    select_rpn(synth, channel, 0);
    send_cc(synth, channel, 6, semitones);
    send_cc(synth, channel, 38, 0);
}

pub(super) fn set_bend(synth: &mut rustysynth::Synthesizer, channel: usize, bend: i8) {
    let value = 8192i32 + i32::from(bend) * 128;
    synth.process_midi_message(channel as i32, 0xE0, value & 0x7F, value >> 7);
}
