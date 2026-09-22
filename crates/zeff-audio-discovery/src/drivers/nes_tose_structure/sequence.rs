use std::collections::BTreeMap;

use crate::{Budget, ScanStop, tracker::FileSpan};

use super::{
    super::candidates::{SelectorHoldReason as Hold, StructuralTrack},
    layout::Layout,
};

pub(super) fn walk(
    bank: &[u8],
    offset: usize,
    layout: &Layout,
    pointer: usize,
    channel: u8,
    budget: &mut Budget<'_>,
) -> Result<Result<StructuralTrack, Hold>, ScanStop> {
    let Some(pointer) = pointer.checked_sub(layout.engine.cpu_base) else {
        return Ok(Err(Hold::SequenceOutOfRange));
    };
    let mut reads = vec![false; bank.len()];
    if let Err(reason) = read(bank, layout, pointer, 4, &mut reads) {
        return Ok(Err(reason));
    }
    let mut pos = 2_u8;
    let mut marker = 0_u8;
    let mut repeats = [0_u8; 2];
    let mut seen = BTreeMap::new();
    let mut notes = 0_u32;
    let mut yields = 0_u32;
    let mut commands = 0_u8;
    for _ in 0..65_536 {
        budget.charge()?;
        let state = (pos, marker, repeats);
        if let Some(previous) = seen.insert(state, yields) {
            return Ok(if previous == yields {
                Err(Hold::NonYieldingLoop)
            } else {
                Ok(track(offset, channel, notes, &reads, pointer, budget)?)
            });
        }
        let address = pointer + usize::from(pos) * 2;
        let first = match read(bank, layout, address, 1, &mut reads) {
            Ok(value) => value,
            Err(reason) => return Ok(Err(reason)),
        };
        let command = first[0];
        if command == 0xff {
            return Ok(Ok(track(offset, channel, notes, &reads, pointer, budget)?));
        }
        let pair = match read(bank, layout, address, 2, &mut reads) {
            Ok(value) => value,
            Err(reason) => return Ok(Err(reason)),
        };
        let argument = pair[1];
        pos = pos.wrapping_add(1);
        commands += 1;
        match command {
            0xad => return Ok(Err(Hold::PointerRebase)),
            0xca..=0xcf => return Ok(Err(Hold::UnsupportedCommand)),
            0xb0..=0xbf => {
                let count = command & 15;
                let counter = usize::from(argument != 0);
                let jump = if count == 0 {
                    true
                } else {
                    repeats[counter] = repeats[counter].wrapping_sub(1);
                    let jump = repeats[counter] != 0;
                    if repeats[counter] & 128 != 0 {
                        repeats[counter] = count;
                    }
                    jump
                };
                if jump {
                    pos = if argument == 0 { marker } else { argument };
                }
                commands = 0;
            }
            0xfd => marker = pos,
            0xa0..=0xff => (),
            _ => {
                if (channel == 3 && command < 16) || (channel != 3 && command & 15 < 12) {
                    notes += 1;
                }
                yields += 1;
                commands = 0;
                if pos == 255 {
                    return Ok(Ok(track(offset, channel, notes, &reads, pointer, budget)?));
                }
                if pos == 0 {
                    return Ok(Err(Hold::Restart));
                }
            }
        }
        if commands >= 16 || (pos == 0 && commands != 0) {
            return Ok(Err(Hold::CommandBatchLimit));
        }
    }
    Ok(Err(Hold::WalkLimit))
}

fn read<'a>(
    bank: &'a [u8],
    layout: &Layout,
    at: usize,
    len: usize,
    reads: &mut [bool],
) -> Result<&'a [u8], Hold> {
    let bytes = bank.get(at..at + len).ok_or(Hold::SequenceOutOfRange)?;
    if (at..at + len).any(|address| layout.is_code(address)) {
        return Err(Hold::SourceOverlap);
    }
    if at < layout.table {
        return Err(Hold::SequenceOutOfRange);
    }
    reads[at..at + len].fill(true);
    Ok(bytes)
}

fn track(
    offset: usize,
    channel: u8,
    note_count: u32,
    reads: &[bool],
    pointer: usize,
    budget: &mut Budget<'_>,
) -> Result<StructuralTrack, ScanStop> {
    let mut source_spans = Vec::new();
    let mut start = None;
    let end = (pointer + 512).min(reads.len());
    for (index, used) in reads[pointer..end]
        .iter()
        .copied()
        .chain([false])
        .enumerate()
    {
        let at = pointer + index;
        if index.is_multiple_of(64) {
            budget.charge()?;
        }
        if used {
            start.get_or_insert(at);
        } else if let Some(start) = start.take() {
            if source_spans.len() >= 256 {
                return Err(ScanStop::InventoryLimit);
            }
            source_spans.push(FileSpan {
                offset: (offset + start) as u32,
                byte_len: (at - start) as u32,
            });
        }
    }
    Ok(StructuralTrack {
        channel,
        note_count,
        source_spans,
    })
}
