use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use serde_json::{Value, json};

use super::SongCandidate;
use super::formats::BankSelect;
use super::mp2k::{self, Event};
use super::render::{MAX_DURATION_SECONDS, MAX_LOOP_PASSES, PlaybackGain};
use super::timeline::{
    GBA_CLOCK_HZ, GBA_CYCLES_PER_FRAME, Scheduled, ScheduledEvent, build_timeline,
};

const PPQN: u16 = 48;
const MAX_MIDI_BYTES: usize = 32 * 1024 * 1024;
const MAX_TRACK_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_VLQ: u32 = 0x0FFF_FFFF;
const MAX_ACTIVE_NOTES: usize = 4096;

#[derive(Clone, Copy, Serialize)]
pub(super) struct MidiOptions {
    pub(super) loops: u8,
    pub(super) max_seconds: u16,
    pub(super) skip_channel10: bool,
    pub(super) bank_select: BankSelect,
    pub(super) playback_gain: PlaybackGain,
}

pub(super) fn encode(
    song: &SongCandidate,
    bytes: &[u8],
    options: MidiOptions,
    mut metadata: Value,
    cancel: &AtomicBool,
    progress: &AtomicU32,
) -> Result<Vec<u8>> {
    ensure!(
        (1..=MAX_LOOP_PASSES).contains(&options.loops),
        "MIDI loop count must be between 1 and {MAX_LOOP_PASSES}"
    );
    ensure!(
        (1..=MAX_DURATION_SECONDS).contains(&options.max_seconds),
        "MIDI duration limit must be between 1 and {MAX_DURATION_SECONDS} seconds"
    );
    ensure!(
        (1..=16).contains(&song.tracks.len()),
        "MIDI export requires between 1 and 16 sequence tracks"
    );
    check_cancel(cancel)?;
    progress.store(0, Ordering::Relaxed);
    let programs = song
        .tracks
        .iter()
        .map(|track| mp2k::program_for_song(bytes, song, track.entry_address, cancel))
        .collect::<Result<Vec<_>>>()?;
    let (timeline, end_tick) = build_timeline(&programs, options.loops)?;
    // A very slow engine tempo exceeds SMF's 24-bit tempo field. Double ticks
    // and halve every tempo in that file to preserve elapsed time.
    let tempo_scale = if timeline.iter().any(|item| {
        matches!(
            item.event,
            ScheduledEvent::Sequence(Event::Control {
                opcode: 0xBB,
                value: 1
            })
        )
    }) {
        2
    } else {
        1
    };
    let ticks_per_engine = 2 * tempo_scale;
    let (end_midi_tick, truncated) =
        duration_limit(&timeline, end_tick, ticks_per_engine, options.max_seconds)?;
    ensure!(
        end_midi_tick > 0,
        "duration limit is shorter than one MIDI clock tick"
    );

    let mut warnings = vec![
        "MIDI is an approximate sequence projection. Load the matching song SoundFont for its instrument/program mapping; General MIDI instruments are unrelated.".to_owned(),
        "Engine envelopes, PSG behavior, priority/channel stealing, reverb and runtime player controls are not represented by MIDI.".to_owned(),
        "Original keys select SoundFont regions; KEYSH and TUNE use channel coarse/fine tuning RPNs, which the MIDI player must support.".to_owned(),
    ];
    if !song.warnings.is_empty() {
        warnings.push("The scan has unresolved structure/instrument warnings. Decoded sequence events are exported independently; this does not establish that all instruments can be rendered.".to_owned());
    }
    if tempo_scale != 1 {
        warnings.push("Very slow tempo requires doubled MIDI ticks and halved tempo values; elapsed timing is preserved, with a different notated beat scale.".to_owned());
    }
    if options.skip_channel10 && song.tracks.len() == 16 {
        warnings.push("The sixteenth source track is emitted on MIDI port 1 because the first 15 tracks skip channel 10. Players must support the standard MIDI-port meta event.".to_owned());
    }
    if truncated {
        warnings.push(format!("The expanded sequence was truncated at the {}-second limit, rounded down to a MIDI tick.", options.max_seconds));
    }

    let mut tracks = (0..song.tracks.len())
        .map(|index| {
            ChannelTrack::new(
                index as u8,
                options.skip_channel10,
                options.bank_select,
                options.playback_gain,
            )
        })
        .collect::<Result<Vec<_>>>()?;
    let mut tempos = vec![(0, tempo_us(75, tempo_scale)?)];
    let mut markers = programs
        .iter()
        .map(|program| {
            program
                .loop_start
                .map(|tick| {
                    tick.checked_mul(ticks_per_engine)
                        .context("MIDI loop marker overflows")
                })
                .transpose()
        })
        .collect::<Result<Vec<_>>>()?;
    for (index, timed) in timeline.iter().enumerate() {
        if index.is_multiple_of(256) {
            check_cancel(cancel)?;
            progress.store(
                (index * 90 / timeline.len().max(1)) as u32,
                Ordering::Relaxed,
            );
        }
        let tick = timed
            .tick
            .checked_mul(ticks_per_engine)
            .context("MIDI event time overflows")?;
        if tick > end_midi_tick {
            break;
        }
        let track_index = usize::from(timed.track);
        let track = &mut tracks[track_index];
        if markers[track_index].is_some_and(|marker| marker <= tick && marker <= end_midi_tick) {
            track.writer.meta(
                markers[track_index].take().unwrap(),
                0x06,
                b"Source loop start (expanded)",
            )?;
        }
        match timed.event {
            ScheduledEvent::Sequence(Event::MemoryWrite { index, value }) => {
                track.writer.meta(
                    tick,
                    0x01,
                    format!("MP2k MEMACC write [{index}] = {value} (not executed)").as_bytes(),
                )?;
                push_warning(
                    &mut warnings,
                    "Sequence memory-write commands are preserved as text events; MIDI export does not execute game/player memory operations.",
                );
            }
            ScheduledEvent::Sequence(Event::ExtendedControl { command, value }) => {
                track.writer.meta(
                    tick,
                    0x01,
                    format!("MP2k XCMD {command} = {value} (pseudo echo; unprojected)").as_bytes(),
                )?;
                push_warning(
                    &mut warnings,
                    "MP2k pseudo-echo commands are preserved as text events; their synthesis effect is not represented by MIDI.",
                );
            }
            ScheduledEvent::Sequence(Event::Control {
                opcode: 0xBB,
                value,
            }) => tempos.push((tick, tempo_us(value, tempo_scale)?)),
            ScheduledEvent::Sequence(Event::Control { opcode, value }) => {
                track.control(tick, opcode, value, &mut warnings)?
            }
            ScheduledEvent::Sequence(Event::Note { key, velocity, .. }) => {
                if tick < end_midi_tick {
                    track.note_on(tick, timed.order, key, velocity, &mut warnings)?;
                }
            }
            ScheduledEvent::GateOff { .. } => track.gate_off(tick, timed.order)?,
            ScheduledEvent::Sequence(Event::EndTie { key }) => track.end_tie(tick, key)?,
            ScheduledEvent::Sequence(Event::Fine) => track.release_all(tick)?,
            ScheduledEvent::Sequence(Event::RuntimeSongReturn) => {
                track.writer.meta(tick, 0x01, b"Engine ends this track and may restore previous runtime music; that continuation is not exported")?;
                track.release_all(tick)?;
                push_warning(
                    &mut warnings,
                    "The engine may restore previously active music after this song. The standalone export ends here and does not follow runtime player state.",
                );
            }
        }
    }
    for (index, track) in tracks.iter_mut().enumerate() {
        if let Some(marker) = markers[index]
            .take()
            .filter(|marker| *marker <= end_midi_tick)
        {
            track
                .writer
                .meta(marker, 0x06, b"Source loop start (expanded)")?;
        }
        track.release_all(end_midi_tick)?;
    }
    metadata["kind"] = json!("approximate_song_midi");
    metadata["midi_options"] = json!(options);
    metadata["midi_ppqn"] = json!(PPQN);
    metadata["midi_ticks_per_engine_tick"] = json!(ticks_per_engine);
    metadata["midi_end_tick"] = json!(end_midi_tick);
    metadata["projection_warnings"] = json!(warnings);
    let metadata = serde_json::to_vec(&metadata)?;
    ensure!(
        metadata.len() <= MAX_TEXT_BYTES,
        "MIDI metadata exceeds the 1 MiB limit"
    );

    let mut conductor = TrackWriter::default();
    conductor.meta(0, 0x03, b"Zeff MP2k sequence (approximate)")?;
    conductor.meta(0, 0x01, &metadata)?;
    for (tick, tempo) in tempos {
        conductor.meta(tick, 0x51, &tempo.to_be_bytes()[1..])?;
    }
    let mut output = Vec::new();
    output.extend_from_slice(b"MThd");
    output.extend_from_slice(&6u32.to_be_bytes());
    output.extend_from_slice(&1u16.to_be_bytes());
    output.extend_from_slice(&(tracks.len() as u16 + 1).to_be_bytes());
    output.extend_from_slice(&PPQN.to_be_bytes());
    append_track(&mut output, conductor.finish(end_midi_tick)?)?;
    for track in tracks {
        check_cancel(cancel)?;
        append_track(&mut output, track.writer.finish(end_midi_tick)?)?;
    }
    check_cancel(cancel)?;
    Ok(output)
}

fn tempo_us(value: u8, scale: u32) -> Result<u32> {
    ensure!(
        value != 0,
        "sequence contains a zero tempo that stops sequence time"
    );
    let numerator = 75 * 24 * 1_000_000 * GBA_CYCLES_PER_FRAME;
    let denominator = u128::from(value) * GBA_CLOCK_HZ * u128::from(scale);
    let tempo = (numerator + denominator / 2) / denominator;
    ensure!(
        (1..=0xFF_FFFF).contains(&tempo),
        "tempo cannot be represented in a MIDI file"
    );
    Ok(tempo as u32)
}

fn duration_limit(
    timeline: &[Scheduled],
    end_tick: u32,
    ticks_per_engine: u32,
    max_seconds: u16,
) -> Result<(u32, bool)> {
    let end = end_tick
        .checked_mul(ticks_per_engine)
        .context("MIDI song duration overflows")?;
    let max_time = (u128::from(max_seconds) * 1_000_000) << 32;
    let mut time = 0u128;
    let mut previous = 0u32;
    let mut tempo = tempo_us(75, ticks_per_engine / 2)?;
    for event in timeline.iter().map(Some).chain(std::iter::once(None)) {
        let tick = event.map_or(Ok(end), |event| {
            event
                .tick
                .checked_mul(ticks_per_engine)
                .context("MIDI event time overflows")
        })?;
        let delta = tick
            .checked_sub(previous)
            .context("MIDI timeline is not ordered")?;
        let next_time = time + ((u128::from(delta) * u128::from(tempo)) << 32) / u128::from(PPQN);
        if next_time > max_time {
            let remaining = ((max_time - time) * u128::from(PPQN)) / (u128::from(tempo) << 32);
            return Ok((
                previous + u32::try_from(remaining).context("MIDI duration limit overflows")?,
                true,
            ));
        }
        time = next_time;
        previous = tick;
        if let Some(Scheduled {
            event:
                ScheduledEvent::Sequence(Event::Control {
                    opcode: 0xBB,
                    value,
                }),
            ..
        }) = event
        {
            tempo = tempo_us(*value, ticks_per_engine / 2)?;
        }
    }
    Ok((end, false))
}

struct ChannelTrack {
    channel: u8,
    playback_gain: PlaybackGain,
    voice: Option<u8>,
    writer: TrackWriter,
    active: BTreeMap<u32, (u8, bool)>,
    key_counts: [u16; 128],
}

impl ChannelTrack {
    fn new(
        source_track: u8,
        skip_channel10: bool,
        bank_select: BankSelect,
        playback_gain: PlaybackGain,
    ) -> Result<Self> {
        let (channel, port) = channel_route(source_track, skip_channel10)?;
        let mut track = Self {
            channel,
            playback_gain,
            voice: None,
            writer: TrackWriter::default(),
            active: BTreeMap::new(),
            key_counts: [0; 128],
        };
        track.writer.meta(
            0,
            0x03,
            format!("MP2k track {}", source_track + 1).as_bytes(),
        )?;
        if port != 0 {
            track.writer.meta(0, 0x21, &[port])?;
        }
        track.writer.meta(0, 0x20, &[channel])?;
        for (controller, value) in bank_select_messages(0, bank_select)?.into_iter().chain([
            (7, 0),
            (10, 64),
            (11, 127),
            (64, 0),
        ]) {
            track.cc(0, controller, value)?;
        }
        track.rpn(0, 0, 2, Some(0))?;
        track.rpn(0, 1, 64, Some(0))?;
        track.rpn(0, 2, 64, None)?;
        track.writer.channel(0, 0xE0 | channel, &[0, 64])?;
        Ok(track)
    }

    fn cc(&mut self, tick: u32, controller: u8, value: u8) -> Result<()> {
        self.writer
            .channel(tick, 0xB0 | self.channel, &[controller, value])
    }

    fn rpn(&mut self, tick: u32, parameter: u8, msb: u8, lsb: Option<u8>) -> Result<()> {
        self.cc(tick, 101, 0)?;
        self.cc(tick, 100, parameter)?;
        self.cc(tick, 6, msb)?;
        if let Some(lsb) = lsb {
            self.cc(tick, 38, lsb)?;
        }
        self.cc(tick, 101, 127)?;
        self.cc(tick, 100, 127)
    }

    fn control(
        &mut self,
        tick: u32,
        opcode: u8,
        value: u8,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        match opcode {
            0xBA => Ok(()),
            0xBC => {
                let shift = value as i8;
                ensure!(
                    (-64..=63).contains(&shift),
                    "key shift {shift} exceeds MIDI channel coarse tuning"
                );
                self.rpn(tick, 2, (i16::from(shift) + 64) as u8, None)
            }
            0xBD => {
                ensure!(
                    value <= 127,
                    "voice {value} exceeds the supported MIDI program range"
                );
                self.voice = Some(value);
                self.writer.channel(tick, 0xC0 | self.channel, &[value])
            }
            0xBE => {
                if value > 127 {
                    self.writer.meta(
                        tick,
                        0x01,
                        format!("MP2k VOL {value} clipped to MIDI full-scale 127").as_bytes(),
                    )?;
                    push_warning(
                        warnings,
                        "MP2k volume above 127 is clipped to MIDI full-scale. Native volume headroom is not preserved; loudness and channel balance may differ.",
                    );
                }
                self.cc(tick, 7, self.playback_gain.map(value.min(127)))
            }
            0xBF => self.cc(tick, 10, value),
            0xC0 => self.writer.channel(tick, 0xE0 | self.channel, &[0, value]),
            0xC1 => self.rpn(tick, 0, value, Some(0)),
            0xC2..=0xC5 => {
                if value != 0 {
                    push_warning(
                        warnings,
                        "MP2k LFO/modulation commands are present and are not projected into MIDI.",
                    );
                }
                Ok(())
            }
            0xC8 => self.rpn(tick, 1, value, Some(0)),
            _ => bail!("unsupported decoded MIDI control opcode {opcode:#04x}"),
        }
    }

    fn note_on(
        &mut self,
        tick: u32,
        id: u32,
        key: u8,
        velocity: u8,
        warnings: &mut Vec<String>,
    ) -> Result<()> {
        ensure!(
            self.voice.is_some(),
            "track {} starts a note before selecting an instrument",
            self.channel + 1
        );
        ensure!(key <= 127 && velocity <= 127, "invalid MIDI note data");
        ensure!(
            self.active.len() < MAX_ACTIVE_NOTES,
            "too many simultaneous MIDI notes"
        );
        self.active.insert(id, (key, velocity != 0));
        if velocity == 0 {
            return Ok(());
        }
        if self.key_counts[usize::from(key)] != 0 {
            push_warning(
                warnings,
                "Overlapping notes with the same channel/key use the MIDI player's note-off semantics; MP2k voice identity is not representable.",
            );
        }
        self.key_counts[usize::from(key)] += 1;
        self.writer.channel(
            tick,
            0x90 | self.channel,
            &[key, self.playback_gain.map(velocity)],
        )
    }

    fn gate_off(&mut self, tick: u32, id: u32) -> Result<()> {
        if let Some((key, true)) = self.active.remove(&id) {
            self.key_counts[usize::from(key)] -= 1;
            self.writer.channel(tick, 0x80 | self.channel, &[key, 0])?;
        }
        Ok(())
    }

    fn end_tie(&mut self, tick: u32, key: u8) -> Result<()> {
        let id = self
            .active
            .iter()
            .rev()
            .find_map(|(id, (active_key, _))| (*active_key == key).then_some(*id));
        if let Some(id) = id {
            self.gate_off(tick, id)?;
        }
        Ok(())
    }

    fn release_all(&mut self, tick: u32) -> Result<()> {
        for (_, (key, emitted)) in std::mem::take(&mut self.active) {
            if emitted {
                self.writer.channel(tick, 0x80 | self.channel, &[key, 0])?;
            }
        }
        self.key_counts.fill(0);
        self.cc(tick, 123, 0)
    }
}

fn channel_route(source_track: u8, skip_channel10: bool) -> Result<(u8, u8)> {
    ensure!(
        source_track < 16,
        "MIDI export supports at most 16 source tracks"
    );
    if !skip_channel10 {
        return Ok((source_track, 0));
    }
    match source_track {
        0..=8 => Ok((source_track, 0)),
        9..=14 => Ok((source_track + 1, 0)),
        15 => Ok((0, 1)),
        _ => unreachable!(),
    }
}

fn bank_select_messages(bank: u16, convention: BankSelect) -> Result<Vec<(u8, u8)>> {
    match convention {
        BankSelect::Gs => {
            ensure!(bank <= 127, "GS MIDI bank select must fit CC 0");
            Ok(vec![(0, bank as u8)])
        }
        BankSelect::Mma => {
            ensure!(bank <= 0x3FFF, "MMA MIDI bank select exceeds 14 bits");
            Ok(vec![(0, (bank >> 7) as u8), (32, (bank & 0x7F) as u8)])
        }
    }
}

#[derive(Default)]
struct TrackWriter {
    data: Vec<u8>,
    previous_tick: u32,
}

impl TrackWriter {
    fn start_event(&mut self, tick: u32, bytes: usize) -> Result<()> {
        ensure!(
            self.data
                .len()
                .checked_add(bytes + 4)
                .is_some_and(|size| size <= MAX_TRACK_BYTES),
            "MIDI track exceeds its size limit"
        );
        let delta = tick
            .checked_sub(self.previous_tick)
            .context("MIDI track events moved backwards")?;
        write_vlq(&mut self.data, delta)?;
        self.previous_tick = tick;
        Ok(())
    }

    fn channel(&mut self, tick: u32, status: u8, bytes: &[u8]) -> Result<()> {
        let length = if status & 0xF0 == 0xC0 { 1 } else { 2 };
        ensure!(
            (0x80..=0xEF).contains(&status)
                && bytes.len() == length
                && bytes.iter().all(|value| *value <= 127),
            "invalid MIDI channel message"
        );
        self.start_event(tick, bytes.len() + 1)?;
        self.data.push(status);
        self.data.extend_from_slice(bytes);
        Ok(())
    }

    fn meta(&mut self, tick: u32, kind: u8, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() <= MAX_TEXT_BYTES,
            "MIDI meta event exceeds its size limit"
        );
        self.start_event(tick, bytes.len() + 6)?;
        self.data.extend_from_slice(&[0xFF, kind]);
        write_vlq(&mut self.data, bytes.len() as u32)?;
        self.data.extend_from_slice(bytes);
        Ok(())
    }

    fn finish(mut self, tick: u32) -> Result<Vec<u8>> {
        self.meta(tick, 0x2F, &[])?;
        Ok(self.data)
    }
}

fn append_track(output: &mut Vec<u8>, track: Vec<u8>) -> Result<()> {
    ensure!(
        output
            .len()
            .checked_add(track.len() + 8)
            .is_some_and(|size| size <= MAX_MIDI_BYTES),
        "MIDI export exceeds the 32 MiB limit"
    );
    output.extend_from_slice(b"MTrk");
    output.extend_from_slice(&(track.len() as u32).to_be_bytes());
    output.extend_from_slice(&track);
    Ok(())
}

fn write_vlq(output: &mut Vec<u8>, mut value: u32) -> Result<()> {
    ensure!(
        value <= MAX_VLQ,
        "MIDI delta time exceeds four-byte VLQ range"
    );
    let mut bytes = [0u8; 4];
    let mut start = 3;
    bytes[start] = (value & 0x7F) as u8;
    value >>= 7;
    while value != 0 {
        start -= 1;
        bytes[start] = (value & 0x7F) as u8 | 0x80;
        value >>= 7;
    }
    output.extend_from_slice(&bytes[start..]);
    Ok(())
}

fn push_warning(warnings: &mut Vec<String>, text: &str) {
    if !warnings.iter().any(|warning| warning == text) {
        warnings.push(text.to_owned());
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "MIDI export cancelled");
    Ok(())
}

#[cfg(test)]
#[path = "midi/tests.rs"]
mod tests;
