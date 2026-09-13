use super::*;
use crate::radriver::RadriverLayout;

pub(super) fn inspect(
    bytes: &[u8],
    song: usize,
    budget: &mut Budget<'_>,
) -> ReadResult<RadriverNativeProfile> {
    let play = song + 0x21c;
    if !sig::matches(bytes, play, sig::CONTEXT_EFFECT)
        || !sig::matches(bytes, song + 0x34, sig::CONTEXT_ALLOCATE)
        || branch(bytes, song + 0x16) != Some(song + 0x34)
    {
        return Err(ReadError::Invalid);
    }
    let table = pointer(
        bytes,
        literal(bytes, song + 8)?,
        (usize::from(half(bytes, song + 4).unwrap() & 255) + 1) * 8,
    )?;
    let init = find_one(
        bytes,
        song.checked_sub(0x400).ok_or(ReadError::Invalid)?,
        song,
        &sig::CONTEXT_INIT[..12],
        budget,
    )?;
    let state = literal(bytes, init + 0x14)?;
    if !ram(state)
        || literal(bytes, play + 6)? != state
        || literal(bytes, song + 0x3a)? != state
        || !sig::matches(bytes, play - 12, &[0x4901, 0x6809, 0x60c8, 0x4770])
        || literal(bytes, play - 12)? != state
    {
        return Err(ReadError::Invalid);
    }
    let update = find_one(bytes, init + 0x100, song, sig::CONTEXT_UPDATE, budget)?;
    if literal(bytes, update + 0xa)? != state
        || literal(bytes, update + 0x1a)? != 0x0400_0208
        || literal(bytes, update + 0x20)? != 0x0400_0104
    {
        return Err(ReadError::Invalid);
    }
    let render = update.checked_sub(0x54).ok_or(ReadError::Invalid)?;
    if !sig::matches(bytes, render, &sig::CONTEXT_RENDER[..26])
        || literal(bytes, render + 0xa)? != state
    {
        return Err(ReadError::Invalid);
    }
    let irq = irq(bytes, init, render, state, budget)?;
    let mut bank_load = None;
    let mut handoff = None;
    for at in (init + 0x80..irq).step_by(2) {
        budget.charge()?;
        if branch(bytes, at) == Some(play - 12)
            && half(bytes, at - 2).is_some_and(|op| op & 0xff00 == 0x4800)
        {
            if bank_load.is_some() {
                return Err(ReadError::Invalid);
            }
            bank_load = Some(at - 2);
        }
        if sig::matches(bytes, at, &[0xb001, 0xbc08, 0x4698, 0xbcf0, 0xbc01, 0x4700]) {
            if handoff.is_some() {
                return Err(ReadError::Invalid);
            }
            handoff = Some(at);
        }
    }
    let load = bank_load.ok_or(ReadError::Invalid)?;
    let bank = bank(bytes, literal(bytes, load)?)?;
    if word(bytes, bank.effective_offset as usize) != Some(0) {
        return Err(ReadError::Invalid);
    }
    let handoff_at = handoff.ok_or(ReadError::Invalid)?;
    if handoff_at <= load
        || half(bytes, handoff_at - 2) != Some(0x6008)
        || literal(bytes, handoff_at - 6)? != 0x0400_0100
    {
        return Err(ReadError::Invalid);
    }
    let timer = literal(bytes, handoff_at - 4)?;
    let sample_rate = match timer {
        0x0080_fa0f => 11025,
        0x0080_f7cf => 8000,
        _ => return Err(ReadError::Invalid),
    };
    let (rate, _) = constant(bytes, init + 0x2a, 0)?;
    if rate != sample_rate {
        return Err(ReadError::Invalid);
    }
    let buffer_store = if half(bytes, init + 0x1a) == Some(0x8398) {
        init + 0x1a
    } else {
        init + 0x1c
    };
    if half(bytes, buffer_store) != Some(0x8398)
        || constant(bytes, buffer_store, 0)?.0 != if rate == 8000 { 208 } else { 288 }
    {
        return Err(ReadError::Invalid);
    }
    let arm0 = literal(bytes, init + 0x2c)?;
    let arm1 = literal(bytes, init + 0x2e)?;
    let arm_start = arm0.min(arm1);
    if arm0.abs_diff(arm1) != 0x2b4 {
        return Err(ReadError::Invalid);
    }
    let mixer = pointer(bytes, arm_start, 840)?;
    let codec_tables = crate::radriver::codec::inspect(bytes, mixer, state, budget)?;
    let mut channels = None;
    let mut calls = Vec::new();
    for at in (2..bytes.len().saturating_sub(3)).step_by(2) {
        budget.charge()?;
        if branch(bytes, at) != Some(init) {
            continue;
        }
        let (count, _) = constant(bytes, at, 2)?;
        if !(1..=16).contains(&count) || channels.is_some_and(|old| old != count) {
            return Err(ReadError::Invalid);
        }
        if calls.len() == 16 {
            return Err(ReadError::Stop(ScanStop::InventoryLimit));
        }
        channels = Some(count);
        calls.push(span(bytes, at - 2, 6)?);
    }
    let channels = channels.ok_or(ReadError::Invalid)? as u8;
    let mut setup = vec![span(bytes, init, play + 0x40 - init)?, mixer, table, bank];
    setup.extend(codec_tables);
    setup.extend(calls);
    setup.sort_unstable();
    setup.dedup();
    Ok(RadriverNativeProfile {
        layout: RadriverLayout::ContextState,
        init: span(bytes, init, irq - init)?,
        handoff: span(bytes, handoff_at, 12)?,
        play_effect: span(bytes, play, 0x40)?,
        play_music: Some(span(bytes, song, 0x34)?),
        update: span(bytes, update, song - update)?,
        timer_irq: span(bytes, irq, render - irq)?,
        bank,
        music_table: Some(table),
        state,
        sample_rate,
        channels,
        setup_spans: setup,
    })
}

fn irq(
    bytes: &[u8],
    init: usize,
    render: usize,
    state: u32,
    budget: &mut Budget<'_>,
) -> ReadResult<usize> {
    let mut found = None;
    for pattern in [sig::CONTEXT_IRQ_A, sig::CONTEXT_IRQ_B] {
        for at in (init + 0x100..render).step_by(2) {
            budget.charge()?;
            if !sig::matches(bytes, at, pattern) {
                continue;
            }
            if literal(bytes, at + 2)? != 0x0400_00c4
                || literal(bytes, at + 8)? != 0x0400_00d0
                || literal(bytes, at + 0xc)? != state
            {
                return Err(ReadError::Invalid);
            }
            if found.is_some_and(|old| old != at) {
                return Err(ReadError::Invalid);
            }
            found = Some(at);
        }
    }
    found.ok_or(ReadError::Invalid)
}
