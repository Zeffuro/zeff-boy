use std::collections::{BTreeMap, BTreeSet};

use crate::{Budget, RomSpan};

use super::{GbSoundSystemSong, GbSoundSystemTrack, ReadError, profiles::Driver, span, word};

struct Reader<'a, 'b, 'c> {
    bytes: &'a [u8],
    driver: &'a Driver,
    budget: &'b mut Budget<'c>,
    ranges: Vec<(u16, u16)>,
}

impl Reader<'_, '_, '_> {
    fn get(&mut self, address: u16, count: usize) -> Result<&[u8], ReadError> {
        self.budget.charge()?;
        let start = usize::from(address);
        if start < 0x4000 + self.driver.profile.end || start + count > 0x8000 {
            return Err(ReadError::Invalid);
        }
        self.ranges.push((address, (start + count) as u16));
        let offset = usize::from(self.driver.bank) * 0x4000 + start - 0x4000;
        Ok(&self.bytes[offset..offset + count])
    }

    fn pointer(&mut self, address: u16) -> Result<u16, ReadError> {
        Ok(word(self.get(address, 2)?, 0))
    }

    fn instrument(&mut self, mut address: u16, channel: u8) -> Result<(), ReadError> {
        let mut burst = 0;
        for _ in 0..2048 {
            let command = self.get(address, 1)?[0];
            address += 1;
            if command >= 12 {
                return Err(ReadError::Invalid);
            }
            let count = match command {
                2 | 7..=9 => 0,
                4 => 2,
                6 => 3,
                10 if channel == 2 => 17,
                _ => 1,
            };
            self.get(address, count)?;
            address += count as u16;
            if command == 2 {
                return Ok(());
            }
            burst = if command == 0 { 0 } else { burst + 1 };
            if burst > 128 {
                return Err(ReadError::Invalid);
            }
        }
        Err(ReadError::Invalid)
    }
}

pub(super) fn song(
    bytes: &[u8],
    driver: &Driver,
    index: u16,
    budget: &mut Budget<'_>,
) -> Result<GbSoundSystemSong, ReadError> {
    if index >= driver.count {
        return Err(ReadError::Invalid);
    }
    let mut reader = Reader {
        bytes,
        driver,
        budget,
        ranges: Vec::new(),
    };
    let entry = driver.table + index * 4;
    let instrument_address = reader.pointer(entry)?;
    let order_address = reader.pointer(entry + 2)?;
    let mut command_address = reader.pointer(order_address)?;
    let mut order = order_address.checked_add(4).ok_or(ReadError::Invalid)?;
    let mut notes = [0; 4];
    let mut instruments = BTreeSet::new();
    let mut seen = BTreeMap::new();
    let mut yields = 0;
    let mut burst = 0;
    loop {
        if let Some(previous_yields) = seen.insert((command_address, order), yields) {
            if previous_yields == yields {
                return Err(ReadError::Invalid);
            }
            break;
        }
        if seen.len() > 16384 || burst > 128 {
            return Err(ReadError::Invalid);
        }
        let command = reader.get(command_address, 1)?[0];
        command_address += 1;
        let count = match command {
            1 | 8 | 17 => 2,
            7 | 9 | 11..=14 => 0,
            0 | 2..=6 | 10 | 15 | 16 | 18 => 1,
            _ => return Err(ReadError::Invalid),
        };
        let mut args = [0; 2];
        args[..count].copy_from_slice(reader.get(command_address, count)?);
        command_address += count as u16;
        match command {
            1 | 2 => {
                if command == 1 && args[0] >= 72 {
                    return Err(ReadError::Invalid);
                }
                let packed = args[usize::from(command == 1)];
                let channel = packed & 3;
                let pointer = instrument_address
                    .checked_add(u16::from(packed >> 2) * 2)
                    .ok_or(ReadError::Invalid)?;
                instruments.insert((reader.pointer(pointer)?, channel));
                notes[usize::from(channel)] += 1;
            }
            5 if args[0] > 3 => return Err(ReadError::Invalid),
            7 => {
                command_address = reader.pointer(order)?;
                order = order.checked_add(4).ok_or(ReadError::Invalid)?;
            }
            8 => {
                // The native operand moves the next-order cursor, not the command cursor.
                order = order.wrapping_add(u16::from_le_bytes(args));
                reader.get(order, 2)?;
            }
            9 => break,
            _ => (),
        }
        if matches!(command, 0 | 7 | 11..=14) {
            yields += 1;
            burst = 0;
        } else {
            burst += 1;
        }
    }
    if notes.iter().sum::<u32>() == 0 {
        return Err(ReadError::Invalid);
    }
    for (address, channel) in instruments {
        reader.instrument(address, channel)?;
    }
    reader.ranges.extend([
        (0x4000, 0x4000 + driver.profile.end as u16),
        (driver.dispatch, driver.dispatch + 0x148),
    ]);
    reader.ranges.sort_unstable();
    let mut merged: Vec<(u16, u16)> = Vec::new();
    for (start, end) in reader.ranges {
        if let Some(last) = merged.last_mut()
            && start <= last.1
        {
            last.1 = last.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    let mut mapped_spans: Vec<_> = merged
        .into_iter()
        .map(|(start, end)| span(driver.bank, start, usize::from(end - start)))
        .collect();
    if driver.normal_host {
        mapped_spans.insert(
            0,
            RomSpan {
                effective_offset: 0x150,
                byte_len: 0x34,
                canonical_cpu_address: 0x150,
            },
        );
    }
    Ok(GbSoundSystemSong {
        profile: driver.profile.name,
        index,
        title: format!("GB Sound System bank {} song {}", driver.bank, index),
        bank: driver.bank,
        order_address,
        instrument_address,
        hardware: driver.hardware,
        table_entry: span(driver.bank, entry, 4),
        tracks: notes
            .into_iter()
            .enumerate()
            .map(|(channel, note_count)| GbSoundSystemTrack {
                number: channel as u8 + 1,
                note_count,
            })
            .collect(),
        mapped_spans,
        warnings: Vec::new(),
    })
}
