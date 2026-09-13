use std::collections::BTreeSet;

use crate::{
    Budget, EngineProfile, InstrumentInventory, InstrumentRegion, RomSpan, ScanStop, ToneInventory,
    Warning,
};

use crate::sample::{SampleError, read_sample_inventory};

use super::{rom_pointer, word};

pub(crate) fn read_instrument(
    bytes: &[u8],
    bank: usize,
    voice: u8,
    keys: &BTreeSet<u8>,
    budget: &mut Budget<'_>,
    engine: EngineProfile,
) -> Result<(Option<InstrumentInventory>, Vec<Warning>), ScanStop> {
    let offset = bank + usize::from(voice) * 12;
    let Some(tone) = descriptor(bytes, offset) else {
        return Ok((None, vec![Warning::InvalidInstrument { voice }]));
    };
    let mut instrument = InstrumentInventory {
        voice,
        tone,
        key_map: None,
        regions: Vec::new(),
    };
    if !matches!(instrument.kind, 0x40 | 0x80) {
        let warning = read_leaf(bytes, &mut instrument.tone, voice, engine);
        return Ok((Some(instrument), warning.into_iter().collect()));
    }
    let Some(base) = rom_pointer(bytes, instrument.data_word, 12, 4) else {
        return Ok((Some(instrument), vec![Warning::InvalidInstrument { voice }]));
    };
    let key_map = if instrument.kind == 0x40 {
        let Some(map) = word(bytes, offset + 8).and_then(|ptr| rom_pointer(bytes, ptr, 128, 1))
        else {
            return Ok((Some(instrument), vec![Warning::InvalidInstrument { voice }]));
        };
        instrument.key_map = Some(RomSpan::new(map, 128));
        Some(map)
    } else {
        None
    };
    let mut warnings = Vec::new();
    if keys.is_empty() {
        warnings.push(Warning::UnresolvedInstrumentKeys { voice });
    }
    // The driver selects one child tone per note and rejects a second split/rhythm level.
    for &key in keys {
        budget.charge()?;
        let index = key_map.map_or(key, |map| bytes[map + usize::from(key)]);
        if let Some(last) = instrument.regions.last_mut()
            && last.descriptor_index == index
            && last.key_end.checked_add(1) == Some(key)
        {
            last.key_end = key;
            continue;
        }
        let child_offset = base + usize::from(index) * 12;
        let mut tone = descriptor(bytes, child_offset);
        let warning = match &mut tone {
            Some(tone) => read_leaf(bytes, tone, voice, engine).map(|warning| match warning {
                Warning::EmptySample { .. } => warning,
                Warning::UnsupportedInstrument { kind, .. } => {
                    Warning::UnsupportedInstrumentRegion {
                        voice,
                        key,
                        kind,
                        offset: child_offset as u32,
                    }
                }
                _ => Warning::InvalidInstrumentRegion {
                    voice,
                    key,
                    offset: child_offset as u32,
                },
            }),
            None => Some(Warning::InvalidInstrumentRegion {
                voice,
                key,
                offset: child_offset as u32,
            }),
        };
        if let Some(warning) = warning {
            warnings.push(warning);
        }
        instrument.regions.push(InstrumentRegion {
            key_start: key,
            key_end: key,
            descriptor_index: index,
            tone,
            warning,
        });
    }
    Ok((Some(instrument), warnings))
}

fn descriptor(bytes: &[u8], offset: usize) -> Option<ToneInventory> {
    let data = bytes.get(offset..offset.checked_add(12)?)?;
    Some(ToneInventory {
        descriptor: RomSpan::new(offset, 12),
        kind: data[0],
        data_word: word(bytes, offset + 4)?,
        key: data[1],
        length: data[2],
        pan_sweep: data[3],
        adsr: (!matches!(data[0], 0x40 | 0x80))
            .then(|| data[8..12].try_into().expect("four descriptor bytes")),
        // Bit 3 fixes PCM rate; PSG uses it to round the note-derived hardware frequency.
        fixed_pitch: data[0] & 7 == 0 && data[0] & 8 != 0,
        sample_header: None,
        sample: None,
        waveform: None,
        synthesis: None,
    })
}

fn read_leaf(
    bytes: &[u8],
    tone: &mut ToneInventory,
    voice: u8,
    engine: EngineProfile,
) -> Option<Warning> {
    let invalid = Some(Warning::InvalidInstrument { voice });
    match tone.kind {
        0x00 | 0x08 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 => {
            let Some(sample) = rom_pointer(bytes, tone.data_word, 16, 4) else {
                return invalid;
            };
            tone.sample_header = Some(RomSpan::new(sample, 16));
            match read_sample_inventory(bytes, sample, tone.kind, engine) {
                Ok(inventory) => tone.sample = Some(inventory),
                Err(SampleError::Empty) => {
                    return Some(Warning::EmptySample {
                        voice,
                        offset: sample as u32,
                    });
                }
                Err(SampleError::Unsupported) => {
                    return Some(Warning::UnsupportedInstrument {
                        voice,
                        kind: tone.kind,
                    });
                }
                Err(SampleError::Invalid) => return invalid,
            }
        }
        0x01 | 0x02 | 0x09 | 0x0A => {
            if tone.data_word > 3 {
                return invalid;
            }
        }
        0x03 | 0x0B => {
            let Some(wave) = rom_pointer(bytes, tone.data_word, 16, 4) else {
                return invalid;
            };
            tone.waveform = Some(RomSpan::new(wave, 16));
        }
        0x04 | 0x0C => {
            if tone.data_word > 1 {
                return invalid;
            }
        }
        0x40 | 0x80 => {
            return Some(Warning::UnsupportedInstrument {
                voice,
                kind: tone.kind,
            });
        }
        _ => return invalid,
    }
    None
}

#[cfg(test)]
mod tests;
