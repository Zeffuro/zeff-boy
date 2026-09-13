use super::{
    Budget, KrawallNativeProfile, NativeEntry, PatternEncoding, RamCopy, RomSpan, ScanStop, half,
    pointer, signatures as sig, startup,
};

pub(super) fn recognize(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Option<KrawallNativeProfile>, ScanStop> {
    let mut found = None;
    for offset in (0..bytes.len().saturating_sub(215)).step_by(4) {
        budget.charge()?;
        let encoding = if exact(bytes, offset, sig::ROW_EXTENDED) {
            PatternEncoding::Extended2004
        } else if exact(bytes, offset, sig::ROW_PACKED) {
            PatternEncoding::Packed2003
        } else {
            continue;
        };
        if found.replace((offset, encoding)).is_some() {
            return Ok(None);
        }
    }
    let Some((row, encoding)) = found else {
        return Ok(None);
    };
    let (instruction, sample_instruction, length) = match encoding {
        PatternEncoding::Packed2003 => (0xbe, 0xcc, sig::ROW_PACKED.len()),
        PatternEncoding::Extended2004 => (0xd2, 0xe0, sig::ROW_EXTENDED.len()),
    };
    let Some(instrument_slot) = literal_slot(bytes, row + instruction) else {
        return Ok(None);
    };
    let Some(sample_slot) = literal_slot(bytes, row + sample_instruction) else {
        return Ok(None);
    };
    let (Ok(instrument_bank), Ok(sample_bank)) = (
        pointer(bytes, instrument_slot, 4, 4),
        pointer(bytes, sample_slot, 4, 4),
    ) else {
        return Ok(None);
    };
    let begin = row.saturating_sub(0x4000);
    let end = bytes.len().min(row.saturating_add(0x8000));
    let init = find_thumb(
        bytes,
        begin,
        end,
        &[
            (sig::INIT_RESET, &[6, 12, 18, 22, 26]),
            (sig::INIT_SHORT, &[6, 12, 16, 20]),
        ],
        budget,
    )?;
    let play = find_thumb(
        bytes,
        begin,
        end,
        match encoding {
            PatternEncoding::Packed2003 => &[(sig::PLAY_PACKED, &[])],
            PatternEncoding::Extended2004 => &[(sig::PLAY_EXTENDED, &[18, 24])],
        },
        budget,
    )?;
    let instrument_update =
        find_thumb(bytes, begin, end, &[(sig::INSTRUMENT_UPDATE, &[6])], budget)?;
    let (Some(init), Some(play), Some(instrument_update)) = (init, play, instrument_update) else {
        return Ok(None);
    };
    let Some((ram_copies, mut setup_spans)) = startup::inspect(bytes, row, budget)? else {
        return Ok(None);
    };
    let mixer = find_ram(bytes, &ram_copies, &[sig::MIXER], budget)?;
    let timer1_irq = find_ram(
        bytes,
        &ram_copies,
        &[sig::IRQ_COUNTER, sig::IRQ_TOGGLE, sig::IRQ_RESTORABLE],
        budget,
    )?;
    let (Some(mixer), Some(timer1_irq)) = (mixer, timer1_irq) else {
        return Ok(None);
    };
    let player_slot = literal_slot(bytes, row + 12);
    let play_player_slot = literal_slot(
        bytes,
        play.source.effective_offset as usize
            + match encoding {
                PatternEncoding::Packed2003 => 28,
                PatternEncoding::Extended2004 => 40,
            },
    );
    let Some(player) = player_slot.and_then(|slot| super::super::word(bytes, slot)) else {
        return Ok(None);
    };
    if play_player_slot.and_then(|slot| super::super::word(bytes, slot)) != Some(player)
        || !((0x0200_0000..0x0204_0000).contains(&player)
            || (0x0300_0000..0x0300_7f00).contains(&player))
    {
        return Ok(None);
    }
    setup_spans.extend([
        RomSpan::new(instrument_slot, 4),
        RomSpan::new(sample_slot, 4),
        RomSpan::new(player_slot.unwrap(), 4),
        RomSpan::new(play_player_slot.unwrap(), 4),
    ]);
    setup_spans.extend([
        init.source,
        play.source,
        instrument_update.source,
        mixer.source,
        timer1_irq.source,
    ]);
    super::merge_spans(&mut setup_spans);
    Ok(Some(KrawallNativeProfile {
        encoding,
        process_row: RomSpan::new(row, length),
        instrument_bank: instrument_bank as u32,
        sample_bank: sample_bank as u32,
        init,
        play,
        instrument_update,
        mixer,
        timer1_irq,
        ram_copies,
        setup_spans,
    }))
}

fn find_thumb(
    bytes: &[u8],
    begin: usize,
    end: usize,
    signatures: &[(&[u8], &[usize])],
    budget: &mut Budget<'_>,
) -> Result<Option<NativeEntry>, ScanStop> {
    let mut result = None;
    for offset in (begin..end).step_by(4) {
        budget.charge()?;
        for &(signature, branches) in signatures {
            if !thumb_matches(bytes, offset, signature, branches) {
                continue;
            }
            let entry = NativeEntry {
                source: RomSpan::new(offset, signature.len()),
                cpu_address: 0x0800_0001 + offset as u32,
            };
            if result.replace(entry).is_some() {
                return Ok(None);
            }
        }
    }
    Ok(result)
}

fn find_ram(
    bytes: &[u8],
    copies: &[RamCopy],
    signatures: &[&[u8]],
    budget: &mut Budget<'_>,
) -> Result<Option<NativeEntry>, ScanStop> {
    let mut result = None;
    for copy in copies {
        if !(0x0300_0000..0x0300_7f00).contains(&copy.destination) {
            continue;
        }
        let start = copy.source.effective_offset as usize;
        let end = start + copy.source.byte_len as usize;
        for offset in (start..end).step_by(4) {
            budget.charge()?;
            for &signature in signatures {
                if offset + signature.len() > end || !exact(bytes, offset, signature) {
                    continue;
                }
                let entry = NativeEntry {
                    source: RomSpan::new(offset, signature.len()),
                    cpu_address: copy.destination + (offset - start) as u32,
                };
                if result
                    .is_some_and(|previous: NativeEntry| previous.cpu_address != entry.cpu_address)
                {
                    return Ok(None);
                }
                result.get_or_insert(entry);
            }
        }
    }
    Ok(result)
}

pub(super) fn exact(bytes: &[u8], offset: usize, signature: &[u8]) -> bool {
    bytes.get(offset..offset.saturating_add(signature.len())) == Some(signature)
}

pub(super) fn thumb_matches(
    bytes: &[u8],
    offset: usize,
    signature: &[u8],
    branches: &[usize],
) -> bool {
    let Some(actual) = bytes.get(offset..offset.saturating_add(signature.len())) else {
        return false;
    };
    let mut cursor = 0;
    while cursor < signature.len() {
        if branches.contains(&cursor) {
            if thumb_branch(bytes, offset + cursor).is_none() {
                return false;
            }
            cursor += 4;
        } else if actual[cursor] == signature[cursor] {
            cursor += 1;
        } else {
            return false;
        }
    }
    true
}

pub(super) fn literal_slot(bytes: &[u8], pc: usize) -> Option<usize> {
    let opcode = half(bytes, pc)?;
    (opcode & 0xf800 == 0x4800).then_some(((pc + 4) & !3) + 4 * usize::from(opcode & 255))
}

pub(super) fn literal(bytes: &[u8], pc: usize) -> Option<u32> {
    super::super::word(bytes, literal_slot(bytes, pc)?)
}

pub(super) fn thumb_branch(bytes: &[u8], pc: usize) -> Option<usize> {
    let first = half(bytes, pc)?;
    let second = half(bytes, pc + 2)?;
    if first & 0xf800 != 0xf000 || second & 0xf800 != 0xf800 {
        return None;
    }
    let encoded = (u32::from(first & 2047) << 12) | (u32::from(second & 2047) << 1);
    let displacement = ((encoded << 9) as i32 >> 9) as i64;
    let target = (pc as i64 + 4 + displacement).try_into().ok()?;
    (target < bytes.len()).then_some(target)
}
