use std::collections::BTreeSet;

use super::{Budget, MAX_SPANS, ReadError, ReadResult, RomSpan, ScanStop, pointer, span, word};

pub(super) struct Assets {
    pub spans: BTreeSet<RomSpan>,
    pub instruments: BTreeSet<usize>,
    pub samples: BTreeSet<usize>,
}

impl Assets {
    pub(super) fn new() -> Self {
        Self {
            spans: BTreeSet::new(),
            instruments: BTreeSet::new(),
            samples: BTreeSet::new(),
        }
    }

    pub(super) fn add(&mut self, bytes: &[u8], at: usize, length: usize) -> ReadResult<()> {
        if self.spans.len() == MAX_SPANS {
            return Err(ReadError::Stop(ScanStop::InventoryLimit));
        }
        self.spans.insert(span(bytes, at, length)?);
        Ok(())
    }

    pub(super) fn voice(
        &mut self,
        bytes: &[u8],
        bank: usize,
        program: u8,
        key: u8,
        budget: &mut Budget<'_>,
    ) -> ReadResult<()> {
        budget.charge()?;
        let slot = bank + usize::from(program) * 4;
        self.add(bytes, slot, 4)?;
        if word(bytes, slot) == Some(0) {
            return Ok(());
        }
        let mut instrument = pointer(bytes, slot, 4, 4)?;
        match bytes[instrument] {
            b'R' | b'S' => {
                let first_key = word(bytes, instrument).ok_or(ReadError::Invalid)? >> 8;
                let relative = i64::from(key) - i64::from(first_key);
                let indirect = if bytes[instrument] == b'R' {
                    self.add(bytes, instrument, 8)?;
                    indexed_pointer(bytes, instrument + 4, relative * 4, 4, 4)?
                } else {
                    self.add(bytes, instrument, 12)?;
                    let map = indexed_pointer(bytes, instrument + 4, relative, 1, 1)?;
                    self.add(bytes, map, 1)?;
                    let index = usize::from(bytes[map]);
                    let table = pointer(bytes, instrument + 8, (index + 1) * 4, 4)?;
                    table + index * 4
                };
                self.add(bytes, indirect, 4)?;
                if word(bytes, indirect) == Some(0) {
                    return Ok(());
                }
                instrument = pointer(bytes, indirect, 32, 4)?;
            }
            _ => {}
        }
        budget.charge()?;
        match bytes[instrument] {
            b'A' | b'F' => {
                self.add(bytes, instrument, 32)?;
                let sample = pointer(bytes, instrument + 4, 24, 4)?;
                let length = word(bytes, sample).ok_or(ReadError::Invalid)? as usize;
                let rate = word(bytes, sample + 4).ok_or(ReadError::Invalid)?;
                let root_key = word(bytes, sample + 8).ok_or(ReadError::Invalid)?;
                let loop_start = word(bytes, sample + 12).ok_or(ReadError::Invalid)? as usize;
                let loop_end = word(bytes, sample + 16).ok_or(ReadError::Invalid)? as usize;
                if length == 0
                    || !(1..=192_000).contains(&rate)
                    || root_key > 127
                    || ((loop_start != 0 || loop_end != 0)
                        && !(loop_start < loop_end && loop_end <= length))
                {
                    return Err(ReadError::Invalid);
                }
                let padded = length.checked_add(3).ok_or(ReadError::Invalid)? & !3;
                let pcm = pointer(bytes, sample + 20, padded, 4)?;
                self.add(bytes, sample, 24)?;
                self.add(bytes, pcm, padded)?;
                self.samples.insert(sample);
            }
            b'P' | b'Q' => {
                self.add(bytes, instrument, 36)?;
                if bytes[instrument + 32] & 3 == 2 {
                    let wave = pointer(bytes, instrument + 4, 16, 4)?;
                    self.add(bytes, wave, 16)?;
                    self.samples.insert(wave);
                }
            }
            _ => return Err(ReadError::Invalid),
        }
        self.instruments.insert(instrument);
        Ok(())
    }
}

fn indexed_pointer(
    bytes: &[u8],
    at: usize,
    relative: i64,
    length: usize,
    align: usize,
) -> ReadResult<usize> {
    let base = word(bytes, at).ok_or(ReadError::Invalid)?;
    // A native key table can point inside a shared pointer array.
    let address = u32::try_from(i64::from(base) + relative).map_err(|_| ReadError::Invalid)?;
    super::rom_pointer(bytes, address, length, align).ok_or(ReadError::Invalid)
}
