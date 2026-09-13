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
    read(bytes, pointer, 4, mapped)?;
    let mut position = 2_u8;
    let mut marker = 0_u8;
    let mut repeat = 0_u8;
    let mut seen = BTreeMap::new();
    let mut notes = 0_u32;
    let mut yields = 0_u32;
    let mut commands = 0_u8;
    for _ in 0..65_536 {
        budget.charge()?;
        if let Some(previous) = seen.insert((position, marker, repeat, commands), yields) {
            return if previous < yields {
                Ok(notes)
            } else {
                Err(ReadError::Invalid)
            };
        }
        commands += 1;
        if commands > 16 {
            return Err(ReadError::Invalid);
        }
        let address = pointer
            .checked_add(u16::from(position) * 2)
            .ok_or(ReadError::Invalid)?;
        let command = read(bytes, address, 1, mapped)?[0];
        if command == 0xff {
            return Ok(notes);
        }
        let argument = read(bytes, address, 2, mapped)?[1];
        match command {
            0xb0..=0xbf => {
                let count = command & 15;
                let jump = if count == 15 {
                    true
                } else {
                    repeat = repeat.wrapping_sub(1);
                    let jump = repeat != 0;
                    if repeat & 0x80 != 0 {
                        repeat = count;
                    }
                    jump
                };
                if jump {
                    position = if argument == 0 { 2 } else { marker };
                    continue;
                }
            }
            0xc0..=0xc9 => (),
            0xca..=0xcf => return Err(ReadError::Invalid),
            0xa0..=0xaf | 0xd0..=0xef | 0xfe => (),
            0xfd => marker = position.wrapping_add(1),
            _ => {
                if (channel == 3 && command < 0x10) || (channel != 3 && command & 15 < 12) {
                    notes += 1;
                }
                yields += 1;
                commands = 0;
                if position == 254 {
                    return Ok(notes);
                }
                if position == 255 {
                    return Err(ReadError::Invalid);
                }
            }
        }
        position = position.wrapping_add(1);
    }
    Err(ReadError::Invalid)
}

fn read<'a>(
    bytes: &'a [u8],
    address: u16,
    len: usize,
    mapped: &mut Vec<RomSpan>,
) -> Result<&'a [u8], ReadError> {
    if address < 0x81f4 || usize::from(address) + len > 0x8f68 {
        return Err(ReadError::Invalid);
    }
    let span = super::span(3, address, len);
    mapped.push(span);
    for offset in 1..len {
        let target = address + offset as u16;
        if target & 0xff00 != address & 0xff00 {
            mapped.push(super::span(3, (address & 0xff00) | (target & 255), 1));
        }
    }
    bytes
        .get(span.effective_offset as usize..span.effective_offset as usize + len)
        .ok_or(ReadError::Invalid)
}
