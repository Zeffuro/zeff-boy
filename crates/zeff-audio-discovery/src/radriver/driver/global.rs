use super::*;
use crate::radriver::RadriverLayout;

pub(super) fn inspect(
    bytes: &[u8],
    song: usize,
    budget: &mut Budget<'_>,
) -> ReadResult<RadriverNativeProfile> {
    let play = song + 0x54;
    if !sig::matches(bytes, play, sig::GLOBAL_EFFECT_A)
        && !sig::matches(bytes, play, sig::GLOBAL_EFFECT_B)
    {
        return Err(ReadError::Invalid);
    }
    let state = literal(bytes, song + 4)?;
    if !ram(state) || literal(bytes, play + 6)? != state {
        return Err(ReadError::Invalid);
    }
    let set = find_one(
        bytes,
        song.checked_sub(0xa0).ok_or(ReadError::Invalid)?,
        song - 0x80,
        &[0x4901, 0x6008, 0x4770, 0],
        budget,
    )?;
    if literal(bytes, set)? != state {
        return Err(ReadError::Invalid);
    }
    let mut found = None;
    for call in (14..bytes.len().saturating_sub(15)).step_by(2) {
        budget.charge()?;
        if branch(bytes, call) != Some(set)
            || half(bytes, call - 2).is_none_or(|op| op & 0xff00 != 0x4800)
        {
            continue;
        }
        let Some(init) = branch(bytes, call - 6) else {
            continue;
        };
        if init >= set || set - init > 0x1000 {
            continue;
        }
        let init_adjust = if sig::matches(bytes, init, sig::GLOBAL_INIT_A) {
            0
        } else if sig::matches(bytes, init, sig::GLOBAL_INIT_B) {
            2
        } else if sig::matches(bytes, init, sig::GLOBAL_INIT_C) {
            0
        } else {
            continue;
        };
        let (rate, rate_span) = constant(bytes, call - 8, 0)?;
        let (channels, _) = constant(bytes, call - 6, 1)?;
        if !(8000..=44010).contains(&rate)
            || !(1..=16).contains(&channels)
            || literal(bytes, init + 0x22 - init_adjust)? != state + 0x2c
        {
            return Err(ReadError::Invalid);
        }
        let bank = bank(bytes, literal(bytes, call - 2)?)?;
        let update = find_one(bytes, init + 0x100, set, &sig::GLOBAL_UPDATE[..12], budget)?;
        if literal(bytes, update + 0x0a)? != state + 0x10
            || literal(bytes, update + 0x0e)? != state - 0x10
            || literal(bytes, update + 0x12)? != state + 8
            || literal(bytes, update + 0x1a)? != state + 0x2c
        {
            return Err(ReadError::Invalid);
        }
        let render = update.checked_sub(0x48).ok_or(ReadError::Invalid)?;
        if !sig::matches(bytes, render, sig::GLOBAL_RENDER) {
            return Err(ReadError::Invalid);
        }
        let (irq, irq_len) = irq(bytes, init, render, state, budget)?;
        let handoff = span(bytes, call + 4, 12)?;
        let mut setup = vec![
            span(bytes, init, play + 0x28 - init)?,
            span(bytes, call - 14, 30)?,
            rate_span,
            literal_span(bytes, call - 2)?,
            handoff,
            bank,
        ];
        setup.push(pointer(bytes, literal(bytes, render + 0xa)?, 4)?);
        setup.extend(
            crate::radriver::startup::inspect(bytes, call)
                .map_err(|_| ReadError::UnboundStartup)?,
        );
        setup.sort_unstable();
        setup.dedup();
        let profile = RadriverNativeProfile {
            layout: RadriverLayout::GlobalState,
            init: span(bytes, init, 0x40)?,
            handoff,
            play_effect: span(bytes, play, 0x28)?,
            play_music: None,
            update: span(bytes, update, set - update)?,
            timer_irq: span(bytes, irq, irq_len)?,
            bank,
            music_table: None,
            state,
            sample_rate: rate,
            channels: channels as u8,
            setup_spans: setup,
        };
        if found.is_some() {
            return Err(ReadError::Invalid);
        }
        found = Some(profile);
    }
    found.ok_or(ReadError::Invalid)
}

fn irq(
    bytes: &[u8],
    init: usize,
    render: usize,
    state: u32,
    budget: &mut Budget<'_>,
) -> ReadResult<(usize, usize)> {
    let mut found = None;
    for (pattern, size, offsets) in [
        (sig::GLOBAL_IRQ_A, 0x34, [0, 6, 0xa, 0xe]),
        (sig::GLOBAL_IRQ_D, 0x34, [0, 6, 0xa, 0xe]),
        (sig::GLOBAL_IRQ_B, 0x4c, [2, 8, 0x14, 0x18]),
        (sig::GLOBAL_IRQ_C, 0x4c, [2, 8, 0x14, 0x18]),
    ] {
        for at in (init + 0x100..render).step_by(2) {
            budget.charge()?;
            if !sig::matches(bytes, at, pattern) {
                continue;
            }
            for (relative, value) in
                offsets
                    .into_iter()
                    .zip([0x0400_00c4, 0x0400_00d0, 0x0400_0104, state + 4])
            {
                if literal(bytes, at + relative)? != value {
                    return Err(ReadError::Invalid);
                }
            }
            if let Some(previous) = found
                && previous != (at, size)
            {
                return Err(ReadError::Invalid);
            }
            found = Some((at, size));
        }
    }
    found.ok_or(ReadError::Invalid)
}
