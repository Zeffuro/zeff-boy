use std::collections::BTreeSet;

use crate::Budget;

use super::{GbCarillonSong, GbCarillonTrack, ReadError, span};

const STARTS: [u8; 8] = [0, 128, 64, 192, 32, 96, 160, 224];

fn order(data: &[u8], mut at: u8, budget: &mut Budget<'_>) -> Result<Option<(u8, u8)>, ReadError> {
    let mut seen = [false; 256];
    loop {
        budget.charge()?;
        if seen[usize::from(at)] {
            return Err(ReadError::Invalid);
        }
        seen[usize::from(at)] = true;
        let page = data[0xf00 + usize::from(at)];
        if page != 0 {
            return Ok(Some((at, page)));
        }
        at = data[0xf00 + usize::from(at.wrapping_add(1))];
        if at == 255 {
            return Ok(None);
        }
    }
}

fn instrument(data: &[u8], kind: u8, start: u8, budget: &mut Budget<'_>) -> Result<(), ReadError> {
    let page = [0x800, 0xa00, 0xd00][usize::from(kind)];
    let mut seen = [false; 256];
    let mut at = start;
    while !seen[usize::from(at)] {
        seen[usize::from(at)] = true;
        let mut immediate = 0;
        while data[page + usize::from(at)] == 0 {
            budget.charge()?;
            immediate += 1;
            if immediate > 32 {
                return Err(ReadError::Invalid);
            }
            let target = data[page + 256 + usize::from(at)];
            if target == 255 {
                return Ok(());
            }
            at = (at & 0xf0).wrapping_add(target);
        }
        budget.charge()?;
        at = at.wrapping_add(1);
    }
    Ok(())
}

pub(super) fn song(
    bytes: &[u8],
    bank: u16,
    index: u16,
    budget: &mut Budget<'_>,
) -> Result<GbCarillonSong, ReadError> {
    let start = *STARTS.get(usize::from(index)).ok_or(ReadError::Invalid)?;
    let offset = usize::from(bank) * 0x4000;
    let data = &bytes[offset..offset + 0x4000];
    let initial = order(data, start, budget)?.ok_or(ReadError::Invalid)?;
    let mut aliases = Vec::new();
    for (other, &at) in STARTS.iter().enumerate() {
        match order(data, at, budget) {
            Ok(Some(value)) if value == initial => aliases.push(other as u16),
            Err(ReadError::Stop(stop)) => return Err(ReadError::Stop(stop)),
            _ => (),
        }
    }
    // Zero-order jumps can make otherwise distinct native IDs the same entry.
    if aliases.first() != Some(&index) {
        return Err(ReadError::Invalid);
    }
    aliases.remove(0);
    let mut state = initial;
    let mut row = 0_u8;
    let mut seen = BTreeSet::new();
    let mut pages = BTreeSet::new();
    let mut instruments = BTreeSet::new();
    let mut notes = [0_u32; 4];
    while seen.insert((state, row)) {
        budget.charge()?;
        if !(0x50..0x80).contains(&state.1) {
            return Err(ReadError::Invalid);
        }
        pages.insert(state.1);
        let at = usize::from(state.1 - 0x40) * 256 + usize::from(row) * 8;
        let event = &data[at..at + 8];
        for (channel, pos) in [0, 2, 4, 6].into_iter().enumerate() {
            let note = event[pos];
            if channel == 2 && note == 255 {
                return Err(ReadError::Invalid);
            }
            if note != 0 {
                notes[channel] += 1;
                if channel == 3 {
                    instruments.insert((2, note & 0xfe));
                } else if note & 1 == 0 {
                    instruments.insert((u8::from(channel == 2), event[pos + 1]));
                }
            }
        }
        row = (row + 1) % 32;
        if event[7] >> 4 == 8 {
            row = 0;
        }
        if row == 0 {
            let Some(next) = order(data, state.0.wrapping_add(1), budget)? else {
                break;
            };
            state = next;
        }
    }
    if notes.iter().all(|&n| n == 0) {
        return Err(ReadError::Invalid);
    }
    for (kind, start) in instruments {
        instrument(data, kind, start, budget)?;
    }
    let mut mapped_spans = vec![span(bank, 0x4000, 0x1000)];
    mapped_spans.extend(
        pages
            .into_iter()
            .map(|page| span(bank, u16::from(page) * 256, 256)),
    );
    Ok(GbCarillonSong {
        profile: "carillon-cgb-v1",
        index,
        bank,
        title: format!("Bank {bank:02X} audio selection {index}"),
        order_address: 0x4f00 + u16::from(initial.0),
        aliases,
        table_entry: span(bank, 0x40f2 + index, 1),
        tracks: notes.into_iter().enumerate().map(|(i, note_count)| GbCarillonTrack { number: i as u8 + 1, note_count }).collect(),
        mapped_spans,
        warnings: vec!["Native CGB double-speed driver, called once per VBlank. Role, duration and complete soundtrack membership are not established; sample commands are not supported.".into()],
    })
}
