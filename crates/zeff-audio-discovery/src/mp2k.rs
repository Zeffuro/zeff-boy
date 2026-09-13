use std::collections::{BTreeMap, BTreeSet};

use super::{
    Budget, CandidateEvidence, Confidence, EngineProfile, RomSpan, ScanLimits, ScanStop,
    SongCandidate, TrackInventory, TrackTermination, VoiceKeys, Warning, rom_pointer, word,
};

mod instruments;

const MAX_TRACK_EVENTS: u32 = 8192;
const MAX_TRACK_SPANS: usize = 128;
const MAX_INVENTORY_ENTRIES: usize = 16_384;

pub(crate) fn scan(
    bytes: &[u8],
    candidates: &mut Vec<SongCandidate>,
    limits: ScanLimits,
    budget: &mut Budget<'_>,
    metadata_headers: &BTreeSet<usize>,
) -> Result<(), ScanStop> {
    budget.charge()?;
    let mut inventory_entries = 0;
    for offset in (0..bytes.len().saturating_sub(11)).step_by(4) {
        budget.charge()?;
        let count = bytes[offset];
        if !(1..=24).contains(&count)
            || (bytes[offset + 1] != 0 && !metadata_headers.contains(&offset))
        {
            continue;
        }
        let header_len = 8 + usize::from(count) * 4;
        if bytes.get(offset..offset + header_len).is_none() {
            continue;
        }
        let voicegroup_address = word(bytes, offset + 4).expect("bounded song header");
        let Some(bank) = rom_pointer(bytes, voicegroup_address, 12, 4) else {
            continue;
        };
        let track_addresses = (0..usize::from(count))
            .map(|track| word(bytes, offset + 8 + track * 4).expect("bounded song header"))
            .collect::<Vec<_>>();
        if !track_addresses.iter().all(|address| {
            rom_pointer(bytes, *address, 1, 1).is_some_and(|entry| bytes[entry] >= 0x80)
        }) {
            continue;
        }

        let engine = if metadata_headers.contains(&offset) {
            EngineProfile::Mp2kSongId
        } else {
            EngineProfile::Mp2k
        };
        let mut tracks = Vec::new();
        let mut warnings = Vec::new();
        let mut voices = BTreeSet::new();
        let mut voice_keys: BTreeMap<u8, BTreeSet<u8>> = BTreeMap::new();
        let mut explicit_note_data = false;
        for address in track_addresses {
            let parsed = read_track(bytes, address, engine, budget, &mut |_, _| {})?;
            voices.extend(parsed.inventory.voices.iter().copied());
            for usage in &parsed.inventory.voice_keys {
                voice_keys
                    .entry(usage.voice)
                    .or_default()
                    .extend(usage.keys.iter().copied());
            }
            explicit_note_data |= parsed.explicit_note_data;
            warnings.extend(parsed.warnings);
            tracks.push(parsed.inventory);
        }
        if voices.is_empty() || tracks.iter().map(|track| track.event_count).sum::<u32>() < 2 {
            continue;
        }
        let mut instruments = Vec::new();
        let mut validated_instruments = 0;
        for voice in voices {
            budget.charge()?;
            let keys = voice_keys.get(&voice).cloned().unwrap_or_default();
            let (instrument, instrument_warnings) =
                instruments::read_instrument(bytes, bank, voice, &keys, budget, engine)?;
            if let Some(instrument) = instrument {
                if instrument_warnings.is_empty() {
                    validated_instruments += 1;
                }
                instruments.push(instrument);
            }
            warnings.extend(instrument_warnings);
        }
        if !explicit_note_data {
            warnings.push(Warning::NoExplicitNoteData);
        }
        if count > 16 {
            warnings.push(Warning::UnsupportedTrackCount { count });
        }
        let decoded_tracks = tracks
            .iter()
            .filter(|track| track.termination != TrackTermination::Unresolved)
            .count() as u8;
        let entry_count = 1
            + tracks.len()
            + tracks.iter().map(|track| track.spans.len()).sum::<usize>()
            + instruments.len() * 3
            + instruments
                .iter()
                .map(|instrument| instrument.regions.len() * 4)
                .sum::<usize>()
            + warnings.len();
        if inventory_entries + entry_count > MAX_INVENTORY_ENTRIES {
            return Err(ScanStop::InventoryLimit);
        }
        if candidates.len() >= limits.max_candidates as usize {
            return Err(ScanStop::CandidateLimit);
        }
        inventory_entries += entry_count;
        candidates.push(SongCandidate {
            engine,
            header: RomSpan::new(offset, header_len),
            priority: bytes[offset + 2],
            reverb: bytes[offset + 3],
            voicegroup_address,
            voicegroup_offset: bank as u32,
            confidence: if warnings.is_empty() && validated_instruments > 0 {
                Confidence::Structural
            } else {
                Confidence::Unresolved
            },
            tracks,
            instruments,
            evidence: CandidateEvidence {
                engine_signature_verified: false,
                song_table_verified: false,
                decoded_tracks,
                validated_instruments,
                explicit_note_data,
            },
            warnings,
            table_entries: Vec::new(),
        });
    }
    Ok(())
}

struct ParsedTrack {
    inventory: TrackInventory,
    warnings: Vec<Warning>,
    explicit_note_data: bool,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    ticks: u32,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    loop_start: Option<u32>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    loop_event_start: Option<usize>,
}

const CLOCKS: [u16; 49] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 28,
    30, 32, 36, 40, 42, 44, 48, 52, 54, 56, 60, 64, 66, 68, 72, 76, 78, 80, 84, 88, 90, 92, 96,
];

#[derive(Clone, Debug)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub enum Event {
    Control { opcode: u8, value: u8 },
    ExtendedControl { command: u8, value: u8 },
    MemoryWrite { index: u8, value: u8 },
    Note { key: u8, velocity: u8, gate: u16 },
    EndTie { key: u8 },
    Fine,
    RuntimeSongReturn,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
pub struct TimedEvent {
    pub tick: u32,
    pub event: Event,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone, Debug)]
pub struct Program {
    pub events: Vec<TimedEvent>,
    pub ticks: u32,
    pub loop_start: Option<u32>,
    pub loop_event_start: Option<usize>,
}

#[cfg(not(target_arch = "wasm32"))]
#[cfg(test)]
pub(crate) fn program(
    bytes: &[u8],
    entry_address: u32,
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<Program> {
    program_with_engine(bytes, entry_address, EngineProfile::Mp2k, cancel)
}

#[cfg(not(target_arch = "wasm32"))]
pub fn program_for_song(
    bytes: &[u8],
    song: &SongCandidate,
    entry_address: u32,
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<Program> {
    program_with_engine(bytes, entry_address, song.engine, cancel)
}

#[cfg(not(target_arch = "wasm32"))]
fn program_with_engine(
    bytes: &[u8],
    entry_address: u32,
    engine: EngineProfile,
    cancel: &std::sync::atomic::AtomicBool,
) -> anyhow::Result<Program> {
    anyhow::ensure!(
        rom_pointer(bytes, entry_address, 1, 1).is_some(),
        "invalid sequence entry"
    );
    let mut events = Vec::new();
    let mut budget = Budget {
        remaining: 100_000,
        cancel,
    };
    let parsed = read_track(
        bytes,
        entry_address,
        engine,
        &mut budget,
        &mut |tick, event| {
            events.push(TimedEvent { tick, event });
        },
    )
    .map_err(|stop| anyhow::anyhow!("sequence decoding stopped: {stop:?}"))?;
    anyhow::ensure!(
        parsed
            .warnings
            .iter()
            .all(|warning| matches!(warning, Warning::NoVoiceSelection)
                && parsed.inventory.note_count == 0)
            && parsed.inventory.termination != TrackTermination::Unresolved,
        "sequence cannot be rendered: {:?}",
        parsed.warnings
    );
    anyhow::ensure!(
        parsed.loop_start.is_none_or(|start| parsed.ticks > start),
        "sequence has a zero-time loop"
    );
    Ok(Program {
        events,
        ticks: parsed.ticks,
        loop_start: parsed.loop_start,
        loop_event_start: parsed.loop_event_start,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TrackState {
    pc: usize,
    running: Option<u8>,
    returns: Vec<usize>,
    repeat: u8,
    voice: Option<u8>,
    key: Option<u8>,
    velocity: Option<u8>,
}

fn read_track(
    bytes: &[u8],
    entry_address: u32,
    engine: EngineProfile,
    budget: &mut Budget<'_>,
    emit: &mut impl FnMut(u32, Event),
) -> Result<ParsedTrack, ScanStop> {
    let start = rom_pointer(bytes, entry_address, 1, 1).expect("validated track pointer");
    let mut state = TrackState {
        pc: start,
        running: None,
        returns: Vec::new(),
        repeat: 0,
        voice: None,
        key: None,
        velocity: None,
    };
    let mut visited = BTreeMap::new();
    let mut instructions = BTreeMap::new();
    let mut voices = BTreeSet::new();
    let mut voice_keys: BTreeMap<u8, BTreeSet<u8>> = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut event_count = 0;
    let mut note_count = 0;
    let mut explicit_note_data = false;
    let mut termination = TrackTermination::Unresolved;
    let mut ticks = 0;
    let mut loop_start = None;
    let mut loop_event_start = None;
    let mut emitted = 0;
    loop {
        budget.charge()?;
        if event_count >= MAX_TRACK_EVENTS {
            warnings.push(Warning::TrackLimit {
                offset: state.pc as u32,
            });
            break;
        }
        if let Some(&(tick, event_index)) = visited.get(&state) {
            loop_start = Some(tick);
            loop_event_start = Some(event_index);
            termination = TrackTermination::Loop;
            break;
        }
        visited.insert(state.clone(), (ticks, emitted));
        let instruction_start = state.pc;
        if instructions
            .range(..=instruction_start)
            .next_back()
            .is_some_and(|(&start, &end)| start < instruction_start && instruction_start < end)
        {
            warnings.push(Warning::InvalidTrack {
                offset: instruction_start as u32,
            });
            break;
        }
        let Some(&first) = bytes.get(state.pc) else {
            warnings.push(Warning::InvalidTrack {
                offset: instruction_start as u32,
            });
            break;
        };
        let opcode = if first >= 0x80 {
            state.pc += 1;
            if first >= 0xBD {
                state.running = Some(first);
            }
            first
        } else if let Some(running) = state.running {
            running
        } else {
            warnings.push(Warning::InvalidTrack {
                offset: instruction_start as u32,
            });
            break;
        };
        let mut next_pc = None;
        let mut done = None;
        let mut valid = true;
        let mut event = None;
        match opcode {
            0x80..=0xB0 => ticks += u32::from(CLOCKS[usize::from(opcode - 0x80)]),
            0xB1 => {
                done = Some(TrackTermination::Fine);
                event = Some(Event::Fine);
            }
            0xB6 if engine == EngineProfile::Mp2kSongId => {
                done = Some(TrackTermination::RuntimeSongReturn);
                event = Some(Event::RuntimeSongReturn);
            }
            0xB2 | 0xB3 => {
                let target = word(bytes, state.pc).and_then(|ptr| rom_pointer(bytes, ptr, 1, 1));
                if let Some(target) = target {
                    state.pc += 4;
                    if opcode == 0xB3 {
                        if state.returns.len() == 3 {
                            valid = false;
                        } else {
                            state.returns.push(state.pc);
                        }
                    }
                    next_pc = Some(target);
                } else {
                    valid = false;
                }
            }
            0xB4 => next_pc = state.returns.pop(),
            0xB5 => {
                let repeat = bytes.get(state.pc).copied();
                let target =
                    word(bytes, state.pc + 1).and_then(|ptr| rom_pointer(bytes, ptr, 1, 1));
                if let (Some(count), Some(target)) = (repeat, target) {
                    state.pc += 5;
                    if count == 0 {
                        next_pc = Some(target);
                    } else {
                        state.repeat = state.repeat.wrapping_add(1);
                        if state.repeat < count {
                            next_pc = Some(target);
                        } else {
                            state.repeat = 0;
                        }
                    }
                } else {
                    valid = false;
                }
            }
            0xB9 => {
                if bytes.get(state.pc) == Some(&0) {
                    if let Some(operands) = bytes.get(state.pc + 1..state.pc + 3) {
                        event = Some(Event::MemoryWrite {
                            index: operands[0],
                            value: operands[1],
                        });
                        state.pc += 3;
                    } else {
                        valid = false;
                    }
                } else {
                    warnings.push(Warning::UnsupportedCommand {
                        offset: instruction_start as u32,
                        opcode,
                    });
                    break;
                }
            }
            0xBA..=0xC5 | 0xC8 => {
                if let Some(&value) = bytes.get(state.pc) {
                    state.pc += 1;
                    event = Some(Event::Control { opcode, value });
                    if opcode == 0xBD {
                        state.voice = Some(value);
                        voices.insert(value);
                    }
                } else {
                    valid = false;
                }
            }
            0xCE => {
                let mut key = state.key.unwrap_or(0);
                if bytes.get(state.pc).is_some_and(|value| *value < 0x80) {
                    key = bytes[state.pc];
                    state.key = Some(key);
                    state.pc += 1;
                }
                event = Some(Event::EndTie { key });
            }
            0xCD => {
                if let Some(&command @ (8 | 9)) = bytes.get(state.pc) {
                    if let Some(&value) = bytes.get(state.pc + 1) {
                        state.pc += 2;
                        event = Some(Event::ExtendedControl { command, value });
                    } else {
                        valid = false;
                    }
                } else {
                    warnings.push(Warning::UnsupportedCommand {
                        offset: instruction_start as u32,
                        opcode,
                    });
                    break;
                }
            }
            0xCF..=0xFF => {
                let mut gate = CLOCKS[usize::from(opcode - 0xCF)];
                if bytes.get(state.pc).is_some_and(|value| *value < 0x80) {
                    state.key = Some(bytes[state.pc]);
                    state.pc += 1;
                    if bytes.get(state.pc).is_some_and(|value| *value < 0x80) {
                        state.velocity = Some(bytes[state.pc]);
                        state.pc += 1;
                        if bytes.get(state.pc).is_some_and(|value| *value < 0x80) {
                            gate += u16::from(bytes[state.pc]);
                            state.pc += 1;
                        }
                    }
                }
                note_count += 1;
                explicit_note_data |=
                    state.voice.is_some() && state.key.is_some() && state.velocity.is_some();
                if let (Some(key), Some(velocity)) = (state.key, state.velocity) {
                    event = Some(Event::Note {
                        key,
                        velocity,
                        gate,
                    });
                }
                if let (Some(voice), Some(key)) = (state.voice, state.key) {
                    voice_keys.entry(voice).or_default().insert(key);
                }
            }
            _ => {
                warnings.push(Warning::UnsupportedCommand {
                    offset: instruction_start as u32,
                    opcode,
                });
                break;
            }
        }
        if !valid
            || state.pc <= instruction_start
            || instructions
                .get(&instruction_start)
                .is_some_and(|end| *end != state.pc)
            || instructions
                .range((instruction_start + 1)..state.pc)
                .next()
                .is_some()
        {
            warnings.push(Warning::InvalidTrack {
                offset: instruction_start as u32,
            });
            break;
        }
        instructions.insert(instruction_start, state.pc);
        if let Some(event) = event {
            emit(ticks, event);
            emitted += 1;
        }
        event_count += 1;
        if let Some(ending) = done {
            termination = ending;
            break;
        }
        state.pc = next_pc.unwrap_or(state.pc);
    }
    if voices.is_empty() {
        warnings.push(Warning::NoVoiceSelection);
    }
    let mut spans: Vec<RomSpan> = Vec::new();
    for (start, end) in instructions {
        if let Some(last) = spans.last_mut()
            && last.effective_offset as usize + last.byte_len as usize == start
        {
            last.byte_len += (end - start) as u32;
        } else if spans.len() < MAX_TRACK_SPANS {
            spans.push(RomSpan::new(start, end - start));
        } else {
            warnings.push(Warning::TrackLimit {
                offset: start as u32,
            });
            termination = TrackTermination::Unresolved;
            break;
        }
    }
    Ok(ParsedTrack {
        inventory: TrackInventory {
            entry_address,
            spans,
            event_count,
            note_count,
            voices: voices.into_iter().collect(),
            voice_keys: voice_keys
                .into_iter()
                .map(|(voice, keys)| VoiceKeys {
                    voice,
                    keys: keys.into_iter().collect(),
                })
                .collect(),
            termination,
        },
        warnings,
        explicit_note_data,
        ticks,
        loop_start,
        loop_event_start,
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod event_tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    #[test]
    fn song_id_runtime_return_has_no_operands_and_ends_pending_patterns() {
        let cancel = AtomicBool::new(false);
        for bytes in [
            &[0xBD, 0, 0xCF, 60, 100, 0x84, 0xB6][..],
            &[0xBD, 0, 0xCF, 60, 100, 0x84, 0xB6, 0xB7, 0xFF][..],
        ] {
            let result =
                program_with_engine(bytes, 0x0800_0000, EngineProfile::Mp2kSongId, &cancel)
                    .unwrap();
            assert_eq!(result.ticks, 4);
            assert_eq!(result.events.len(), 3);
            assert_eq!(result.loop_start, None);
            assert_eq!(result.loop_event_start, None);
            assert!(matches!(
                result.events[2],
                TimedEvent {
                    tick: 4,
                    event: Event::RuntimeSongReturn,
                }
            ));
            assert!(program(bytes, 0x0800_0000, &cancel).is_err());
        }
        let bytes = [0xBD, 0, 0xB3, 8, 0, 0, 8, 0xB7, 0xCF, 60, 100, 0x84, 0xB6];
        let result =
            program_with_engine(&bytes, 0x0800_0000, EngineProfile::Mp2kSongId, &cancel).unwrap();
        assert_eq!(result.ticks, 4);
        assert_eq!(result.events.len(), 3);
        assert!(matches!(result.events[2].event, Event::RuntimeSongReturn));
    }

    #[test]
    fn memory_immediate_writes_are_retained_without_executing_player_memory() {
        let bytes = [0xBD, 0, 0xB9, 0, 255, 200, 0xD0, 60, 100, 0x81, 0xB1];
        let before = bytes;
        let result = program(&bytes, 0x0800_0000, &AtomicBool::new(false)).unwrap();
        assert!(matches!(
            result.events[1].event,
            Event::MemoryWrite {
                index: 255,
                value: 200
            }
        ));
        assert_eq!(bytes, before);
        let mut conditional = bytes;
        conditional[3] = 6;
        assert!(program(&conditional, 0x0800_0000, &AtomicBool::new(false)).is_err());
    }

    #[test]
    fn long_pattern_intro_reaches_its_loop_with_a_bounded_event_budget() {
        use crate::test_support::put_word;
        let mut bytes = vec![0; 0x4000];
        bytes[0..2].copy_from_slice(&[0xBD, 0]);
        for call in 0..8 {
            bytes[2 + call * 5] = 0xB3;
            put_word(&mut bytes, 3 + call * 5, 0x0800_1000);
        }
        bytes[42..51].copy_from_slice(&[0xD0, 60, 100, 0x81, 0xB2, 42, 0, 0, 8]);
        for note in 0..300 {
            bytes[0x1000 + note * 4..0x1004 + note * 4].copy_from_slice(&[0xD0, 60, 100, 0x81]);
        }
        bytes[0x1000 + 300 * 4] = 0xB4;
        let result = program(&bytes, 0x0800_0000, &AtomicBool::new(false)).unwrap();
        assert!(result.loop_start.is_some_and(|start| start >= 2400));
        assert!(result.ticks > result.loop_start.unwrap());
        assert!(result.events.len() > 2400);
    }

    #[test]
    fn echo_extensions_preserve_running_status_and_consume_full_byte_values() {
        let bytes = [0xBD, 0, 0xCD, 8, 255, 9, 128, 0xD0, 60, 100, 0x81, 0xB1];
        let program = program(&bytes, 0x0800_0000, &AtomicBool::new(false)).unwrap();
        assert!(matches!(
            program.events[1].event,
            Event::ExtendedControl {
                command: 8,
                value: 255
            }
        ));
        assert!(matches!(
            program.events[2].event,
            Event::ExtendedControl {
                command: 9,
                value: 128
            }
        ));
        assert!(matches!(
            program.events[3].event,
            Event::Note { key: 60, .. }
        ));
        assert_eq!(program.ticks, 1);
        for suffix in [&[0xCD, 8][..], &[0xCD, 7, 0][..]] {
            let mut truncated = vec![0xBD, 0];
            truncated.extend_from_slice(suffix);
            assert!(super::program(&truncated, 0x0800_0000, &AtomicBool::new(false)).is_err());
        }
    }

    #[test]
    fn gate_extension_and_wait_use_the_same_clock_table() {
        let bytes = [0xBD, 0, 0xD0, 60, 100, 3, 0x84, 0xB1];
        let program = program(&bytes, 0x0800_0000, &AtomicBool::new(false)).unwrap();
        assert!(matches!(
            program.events[1],
            TimedEvent {
                tick: 0,
                event: Event::Note {
                    key: 60,
                    velocity: 100,
                    gate: 4
                }
            }
        ));
        assert!(matches!(
            program.events[2],
            TimedEvent {
                tick: 4,
                event: Event::Fine
            }
        ));
        assert_eq!(program.ticks, 4);
    }

    #[test]
    fn end_tie_explicit_key_is_remembered_for_subsequent_notes() {
        let bytes = [0xBD, 0, 0xCF, 60, 100, 0x81, 0xCE, 62, 0xD0, 0x81, 0xB1];
        let program = program(&bytes, 0x0800_0000, &AtomicBool::new(false)).unwrap();
        assert!(matches!(program.events[2].event, Event::EndTie { key: 62 }));
        assert!(matches!(
            program.events[3],
            TimedEvent {
                tick: 1,
                event: Event::Note {
                    key: 62,
                    velocity: 100,
                    gate: 1
                }
            }
        ));
    }

    #[test]
    fn repeated_state_after_a_note_retains_the_exact_loop_event_boundary() {
        let bytes = [0xBD, 0, 0xD0, 60, 100, 0x98, 0xB2, 2, 0, 0, 8];
        let program = program(&bytes, 0x0800_0000, &AtomicBool::new(false)).unwrap();
        assert_eq!(program.ticks, 24);
        assert_eq!(program.loop_start, Some(0));
        assert_eq!(program.loop_event_start, Some(2));
        assert!(matches!(
            program.events[2],
            TimedEvent {
                tick: 24,
                event: Event::Note { .. }
            }
        ));
    }
}
