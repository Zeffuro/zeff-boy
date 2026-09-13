use std::collections::BTreeMap;

use crate::{Budget, RomSpan};

use super::ReadError;

pub(super) fn validate(
    bytes: &[u8],
    pointer: u16,
    channel: u8,
    mapped: &mut Vec<RomSpan>,
    budget: &mut Budget<'_>,
) -> Result<u32, ReadError> {
    read(bytes, pointer, pointer, 4, mapped)?;
    let (mut position, mut marker) = (2_u8, 0_u8);
    let mut repeats = [0_u8; 2];
    let mut seen = BTreeMap::new();
    let (mut notes, mut yields) = (0_u32, 0_u32);
    let mut commands = 0_u16;
    let mut until_yield = 0_u8;
    let mut batch_base = pointer;
    for _ in 0..65_536 {
        budget.charge()?;
        let cursor = if commands == 0 { 0 } else { batch_base };
        if let Some(previous) = seen.insert(
            (position, marker, repeats, cursor, commands, until_yield),
            yields,
        ) {
            return if previous < yields {
                Ok(notes)
            } else {
                Err(ReadError::Invalid)
            };
        }
        until_yield += 1;
        if until_yield > 16 {
            return Err(ReadError::Invalid);
        }
        let address = pointer
            .checked_add(u16::from(position) * 2)
            .ok_or(ReadError::Invalid)?;
        if commands == 0 {
            batch_base = address;
        }
        let command = read(bytes, batch_base, address, 1, mapped)?[0];
        if command == 0xff {
            return Ok(notes);
        }
        let argument = read(bytes, batch_base, address, 2, mapped)?[1];
        position = position.wrapping_add(1);
        commands += 1;
        match command {
            0xb0..=0xbf => {
                let count = command & 15;
                let repeat = &mut repeats[usize::from(argument != 0)];
                let jump = if count == 0 {
                    true
                } else {
                    *repeat = repeat.wrapping_sub(1);
                    let jump = *repeat != 0;
                    if *repeat & 0x80 != 0 {
                        *repeat = count;
                    }
                    jump
                };
                if jump {
                    position = if argument == 0 { marker } else { argument };
                }
                commands = 0;
            }
            0xca..=0xcf => return Err(ReadError::Invalid),
            0xfd => marker = position,
            0xa0..=0xfe => (),
            _ => {
                if (channel == 3 && command < 16) || (channel != 3 && command & 15 < 12) {
                    notes += 1;
                }
                yields += 1;
                until_yield = 0;
                if position == 255 {
                    return Ok(notes);
                }
                if position == 0 {
                    return Err(ReadError::Invalid);
                }
                commands = 0;
            }
        }
        // Command batches retain an eight-bit Y cursor until a note or jump.
        if commands >= 128 || (position == 0 && commands != 0) {
            return Err(ReadError::Invalid);
        }
    }
    Err(ReadError::Invalid)
}

fn read<'a>(
    bytes: &'a [u8],
    base: u16,
    address: u16,
    len: usize,
    mapped: &mut Vec<RomSpan>,
) -> Result<&'a [u8], ReadError> {
    if address < 0x88e2 || usize::from(address) + len > 0xc000 {
        return Err(ReadError::Invalid);
    }
    let span = super::fcg::span(address, len);
    mapped.push(span);
    for offset in 0..len {
        let target = address + offset as u16;
        if target & 0xff00 != base & 0xff00 {
            mapped.push(super::fcg::span((base & 0xff00) | (target & 255), 1));
        }
    }
    bytes
        .get(span.effective_offset as usize..span.effective_offset as usize + len)
        .ok_or(ReadError::Invalid)
}
