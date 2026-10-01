use std::collections::BTreeSet;

use crate::Budget;

use super::{ReadError, Reader};

fn sequence(
    reader: &mut Reader<'_>,
    arpeggio: bool,
    offset: u8,
    wave: bool,
    budget: &mut Budget<'_>,
) -> Result<BTreeSet<u8>, ReadError> {
    let p = reader.profile;
    let (base, end) = if arpeggio {
        (p.arp, p.duty)
    } else {
        (p.duty, p.wave)
    };
    let mut index = 0u8;
    let mut seen = BTreeSet::new();
    let mut values = BTreeSet::new();
    while seen.insert(index) {
        let mut immediate = BTreeSet::new();
        loop {
            budget.charge()?;
            if !immediate.insert(index) {
                return Err(ReadError::Invalid);
            }
            let address = base + u16::from(offset) + u16::from(index);
            let value = reader.owned(address, base, end)?;
            index = index.wrapping_add(1);
            match value {
                255 => index = 0,
                254 => return Ok(values),
                253 if arpeggio => return Ok(values),
                253 => {
                    // The wave handler does not consume FD's following byte.
                    if wave {
                        return Err(ReadError::Invalid);
                    }
                    index = reader.owned(address + 1, base, end)?;
                }
                _ => {
                    values.insert(value);
                    break;
                }
            }
        }
    }
    Ok(values)
}

pub(super) fn validate(
    reader: &mut Reader<'_>,
    channel: usize,
    instrument: u8,
    notes: &BTreeSet<u8>,
    extra_arps: &BTreeSet<u8>,
    budget: &mut Budget<'_>,
) -> Result<(), ReadError> {
    let p = reader.profile;
    let address = p.instruments + u16::from(instrument & 31) * 8;
    if address + 8 > p.instruments_end {
        return Err(ReadError::Invalid);
    }
    let mut record = [0; 7];
    for (i, byte) in record.iter_mut().enumerate() {
        *byte = reader.owned(address + i as u16, p.instruments, p.instruments_end)?;
    }
    if channel < 3 {
        if channel == 2 && record[1] & 128 != 0 {
            return Err(ReadError::Invalid);
        }
        let values = sequence(reader, false, record[1], channel == 2, budget)?;
        if channel == 2 {
            for value in values {
                let start = p.wave + u16::from(value & 15) * 16;
                for i in 0..16 {
                    reader.owned(start + i, p.wave, p.wave_end)?;
                }
            }
        }
    }
    let mut offsets = extra_arps.clone();
    if record[4] & 8 != 0 {
        offsets.insert(record[5]);
    }
    for offset in offsets {
        let values = sequence(reader, true, offset, false, budget)?;
        if channel < 3 {
            for value in values {
                for &note in notes {
                    budget.charge()?;
                    let pitch = if value & 128 != 0 && value != 128 {
                        value & 127
                    } else {
                        note.wrapping_add(value) & 127
                    };
                    if pitch >= 72 {
                        return Err(ReadError::Invalid);
                    }
                    reader.word(p.tone + 2 * u16::from(pitch))?;
                }
            }
        }
    }
    Ok(())
}
