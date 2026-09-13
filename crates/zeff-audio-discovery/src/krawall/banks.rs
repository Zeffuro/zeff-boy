use std::collections::{BTreeMap, BTreeSet};

use super::module::Event;
use super::{Budget, KrawallNativeProfile, ReadError, RomSpan, ScanStop, half, pointer, range};

const MAX_BANK_ITEMS: usize = 4096;

pub(super) struct Banks {
    pub instrument_count: u16,
    pub sample_count: u16,
    pub spans: Vec<RomSpan>,
}

#[derive(Default)]
struct InstrumentUse {
    keys: BTreeSet<u8>,
    following_notes: BTreeSet<u8>,
}

#[derive(Clone, Copy)]
struct Sample {
    relative_note: i8,
}

pub(super) fn inspect(
    bytes: &[u8],
    native: &KrawallNativeProfile,
    instrument_based: bool,
    events: &[Event],
    budget: &mut Budget<'_>,
) -> Result<Banks, ReadError> {
    let mut instruments = BTreeMap::<u16, InstrumentUse>::new();
    let mut channel_instruments: [BTreeSet<u16>; 20] = std::array::from_fn(|_| BTreeSet::new());
    let mut following_notes: [BTreeSet<u8>; 20] = std::array::from_fn(|_| BTreeSet::new());
    let mut samples = BTreeMap::<u16, Sample>::new();
    let mut spans = Vec::new();
    for event in events {
        budget.charge()?;
        if !instrument_based {
            if event.instrument != 0 {
                read_sample(
                    bytes,
                    native.sample_bank as usize,
                    event.instrument - 1,
                    &mut samples,
                    &mut spans,
                    budget,
                )?;
            }
            continue;
        }
        if event.instrument != 0 && event.note != 0 {
            if event.note > 96 {
                return Err(ReadError::Invalid);
            }
            let index = event.instrument - 1;
            instruments
                .entry(index)
                .or_default()
                .keys
                .insert(event.note - 1);
            channel_instruments[usize::from(event.channel)].insert(index);
        } else if event.instrument == 0
            && (1..=96).contains(&event.note)
            && !matches!(event.effect, 19 | 24)
        {
            following_notes[usize::from(event.channel)].insert(event.note - 1);
        }
        if instruments.len() > MAX_BANK_ITEMS {
            return Err(ReadError::Stop(ScanStop::ValidationLimit));
        }
    }
    for (channel, indexes) in channel_instruments.iter().enumerate() {
        for index in indexes {
            budget.charge()?;
            instruments
                .get_mut(index)
                .unwrap()
                .following_notes
                .extend(&following_notes[channel]);
        }
    }
    for (&index, usage) in &instruments {
        budget.charge()?;
        let slot = native.instrument_bank as usize + usize::from(index) * 4;
        let offset = pointer(bytes, slot, 302, 2)?;
        validate_envelope(bytes, offset + 192)?;
        validate_envelope(bytes, offset + 244)?;
        spans.extend([RomSpan::new(slot, 4), RomSpan::new(offset, 302)]);
        let mut pending: Vec<u8> = usage.keys.iter().copied().collect();
        let mut visited = [false; 256];
        let mut relative_notes = BTreeSet::new();
        while let Some(key) = pending.pop() {
            budget.charge()?;
            if visited[usize::from(key)] {
                continue;
            }
            if key >= 96 {
                return Err(ReadError::Invalid);
            }
            visited[usize::from(key)] = true;
            let sample_index =
                half(bytes, offset + usize::from(key) * 2).ok_or(ReadError::Invalid)?;
            let sample = read_sample(
                bytes,
                native.sample_bank as usize,
                sample_index,
                &mut samples,
                &mut spans,
                budget,
            )?;
            if relative_notes.insert(sample.relative_note) {
                // A note without a new instrument indexes through the previous sample's transposition.
                for note in &usage.following_notes {
                    budget.charge()?;
                    let adjusted = note.wrapping_add_signed(sample.relative_note);
                    if !visited[usize::from(adjusted)] {
                        pending.push(adjusted);
                    }
                }
            }
        }
    }
    super::merge_spans(&mut spans);
    Ok(Banks {
        instrument_count: instruments.len() as u16,
        sample_count: samples.len() as u16,
        spans,
    })
}

fn read_sample(
    bytes: &[u8],
    bank: usize,
    index: u16,
    samples: &mut BTreeMap<u16, Sample>,
    spans: &mut Vec<RomSpan>,
    budget: &mut Budget<'_>,
) -> Result<Sample, ReadError> {
    budget.charge()?;
    if let Some(sample) = samples.get(&index) {
        return Ok(*sample);
    }
    if samples.len() >= MAX_BANK_ITEMS {
        return Err(ReadError::Stop(ScanStop::ValidationLimit));
    }
    let slot = bank + usize::from(index) * 4;
    let offset = pointer(bytes, slot, 18, 2)?;
    let loop_length = super::super::word(bytes, offset).ok_or(ReadError::Invalid)? as usize;
    let end_address = super::super::word(bytes, offset + 4).ok_or(ReadError::Invalid)?;
    let end = end_address
        .checked_sub(0x0800_0000)
        .ok_or(ReadError::Invalid)? as usize;
    let frequency = super::super::word(bytes, offset + 8).ok_or(ReadError::Invalid)?;
    if end <= offset + 18
        || end > bytes.len()
        || loop_length > end - offset - 18
        || frequency == 0
        || frequency > 0x000f_ffff
        || bytes[offset + 14] > 64
        || bytes[offset + 16] > 2
        || bytes[offset + 17] > 1
        || bytes[offset + 16] != 0 && loop_length == 0
    {
        return Err(ReadError::Invalid);
    }
    let sample = Sample {
        relative_note: bytes[offset + 13] as i8,
    };
    spans.extend([RomSpan::new(slot, 4), RomSpan::new(offset, end - offset)]);
    samples.insert(index, sample);
    Ok(sample)
}

fn validate_envelope(bytes: &[u8], offset: usize) -> Result<(), ReadError> {
    let header = range(bytes, offset, 52)?;
    let (last, sustain, loop_start, flags) = (header[48], header[49], header[50], header[51]);
    if flags > 7 || flags & 1 != 0 && (last > 11 || sustain > 11 || loop_start > 11) {
        return Err(ReadError::Invalid);
    }
    Ok(())
}
