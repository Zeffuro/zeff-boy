use std::collections::BTreeMap;

use crate::Budget;

use super::ReadError;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct State {
    index: u16,
    repeat: u16,
    counters: [u8; 2],
}

pub(super) fn validate(
    bytes: &[u8],
    bank: u8,
    pointer: u16,
    channel: u8,
    mapped: &mut Vec<(usize, usize)>,
    budget: &mut Budget<'_>,
) -> Result<u32, ReadError> {
    if !(0x4000..=0x7ffb).contains(&pointer) {
        return Err(ReadError::Invalid);
    }
    let base = usize::from(bank) * 0x4000 + usize::from(pointer - 0x4000);
    let bank_end = (usize::from(bank) + 1) * 0x4000;
    mapped.push((base, base + 4));
    let mut state = State {
        index: 2,
        repeat: 0,
        counters: [0; 2],
    };
    let mut visited = BTreeMap::new();
    let mut notes = 0;
    let mut yields = 0u32;
    let mut immediate = 0;
    for _ in 0..65536 {
        budget.charge()?;
        if let Some(previous_yields) = visited.insert(state, yields) {
            return if previous_yields < yields {
                Ok(notes)
            } else {
                Err(ReadError::Invalid)
            };
        }
        let at = base + usize::from(state.index) * 2;
        if state.index < 2 || at >= bank_end {
            return Err(ReadError::Invalid);
        }
        let op = *bytes.get(at).ok_or(ReadError::Invalid)?;
        mapped.push((at, at + 1));
        if op == 0xff {
            return Ok(notes);
        }
        if at + 1 >= bank_end {
            return Err(ReadError::Invalid);
        }
        let argument = *bytes.get(at + 1).ok_or(ReadError::Invalid)?;
        mapped.push((at + 1, at + 2));
        state.index = state.index.checked_add(1).ok_or(ReadError::Invalid)?;
        immediate += 1;
        if immediate > 256 {
            return Err(ReadError::Invalid);
        }
        match op {
            0x00..=0x9f | 0xa7 => {
                if op < 0xa0
                    && if channel == 3 {
                        op != 0x1f
                    } else {
                        op & 15 < 12
                    }
                {
                    notes += 1;
                }
                yields += 1;
                immediate = 0;
            }
            0xb0..=0xbf => {
                let count = op & 15;
                if count != 0 {
                    let counter = &mut state.counters[usize::from(argument != 0)];
                    *counter = counter.wrapping_sub(1);
                    if *counter == 0 {
                        continue;
                    }
                    if *counter & 0x80 != 0 {
                        *counter = count;
                    }
                }
                state.index = if argument == 0 {
                    state.repeat
                } else {
                    u16::from(argument)
                };
            }
            0xfd => state.repeat = state.index,
            _ => (),
        }
    }
    Err(ReadError::Invalid)
}
