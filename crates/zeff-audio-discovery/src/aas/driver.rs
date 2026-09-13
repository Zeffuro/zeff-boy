use super::{
    AasNativeProfile, AasTables, Budget, INSTRUMENTS_PER_SONG, MAX_ORDERS, MAX_ROM_BYTES,
    PATTERN_BYTES, ReadError, ReadResult, RomSpan, SAMPLE_HEADER_BYTES, ScanStop, half,
    signatures as sig, span, word,
};

const MAX_PROFILES: usize = 16;
const ROM_BASE: u32 = 0x0800_0000;

struct Layout {
    max_channels: u8,
    config: &'static [u16],
    play: &'static [u16],
    stop: &'static [u16],
    update: &'static [u16],
    irq: &'static [u16],
    module: &'static [u16],
    do_config: &'static [u16],
    config_len: usize,
    play_len: usize,
    stop_len: usize,
    irq_len: usize,
    config_call: usize,
    config_state: usize,
    update_call: usize,
    pending: usize,
    irq_call: usize,
    irq_pending: usize,
    play_state: usize,
    stop_state: usize,
    sequence: usize,
    patterns: usize,
    sample_data: usize,
    restart: usize,
}

const OLD: Layout = Layout {
    max_channels: 8,
    config: sig::CONFIG_8,
    play: sig::PLAY_8,
    stop: sig::STOP_8,
    update: sig::UPDATE_8,
    irq: sig::IRQ_8,
    module: sig::MOD_INTERRUPT_8,
    do_config: sig::DO_CONFIG_8,
    config_len: 0x130,
    play_len: 0x184,
    stop_len: 0xf0,
    irq_len: 0xd8,
    config_call: 0xca,
    config_state: 0xce,
    update_call: 0x22,
    pending: 6,
    irq_call: 0xa6,
    irq_pending: 0xa0,
    play_state: 0x56,
    stop_state: 0xc,
    sequence: 0x64,
    patterns: 0x6e,
    sample_data: 0xce,
    restart: 0xab0,
};
const CURRENT: Layout = Layout {
    max_channels: 16,
    config: sig::CONFIG_16,
    play: sig::PLAY_16,
    stop: sig::STOP_16,
    update: sig::UPDATE_16,
    irq: sig::IRQ_16,
    module: sig::MOD_INTERRUPT_16,
    do_config: sig::DO_CONFIG_16,
    config_len: 0xfc,
    play_len: 0x18c,
    stop_len: 0xdc,
    irq_len: 0xfc,
    config_call: 0x7a,
    config_state: 0x7e,
    update_call: 0x26,
    pending: 8,
    irq_call: 0xb6,
    irq_pending: 0xb0,
    play_state: 0x13e,
    stop_state: 0xa,
    sequence: 0x7a,
    patterns: 0x88,
    sample_data: 0xcc,
    restart: 0xaa0,
};

pub(super) fn recognize(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Vec<AasNativeProfile>, ScanStop> {
    if bytes.len() > MAX_ROM_BYTES {
        return Ok(Vec::new());
    }
    let mut profiles = Vec::new();
    for config in (0..bytes.len().saturating_sub(63)).step_by(2) {
        budget.charge()?;
        if config % 4 != 0 || half(bytes, config) != Some(0xb5f0) {
            continue;
        }
        for layout in [&OLD, &CURRENT] {
            if !sig::matches(bytes, config, layout.config) {
                continue;
            }
            match inspect(bytes, config, layout, budget) {
                Ok(profile) => {
                    if profiles.len() == MAX_PROFILES {
                        return Err(ScanStop::ValidationLimit);
                    }
                    profiles.push(profile);
                }
                Err(ReadError::Invalid) => {}
                Err(ReadError::Stop(stop)) => return Err(stop),
            }
        }
    }
    Ok(profiles)
}

fn inspect(
    bytes: &[u8],
    config: usize,
    layout: &Layout,
    budget: &mut Budget<'_>,
) -> ReadResult<AasNativeProfile> {
    let end = config.saturating_add(0x4000).min(bytes.len());
    let update = config + layout.config_len;
    let module = branch_target(bytes, update + layout.update_call).ok_or(ReadError::Invalid)?;
    let do_config = branch_target(bytes, config + layout.config_call).ok_or(ReadError::Invalid)?;
    if module < config
        || module >= end
        || do_config < config
        || do_config >= end
        || !sig::matches(bytes, update, layout.update)
        || !sig::matches(bytes, module, layout.module)
        || !sig::matches(bytes, do_config, layout.do_config)
    {
        return Err(ReadError::Invalid);
    }
    let play = unique(bytes, config..end, layout.play, None, budget)?;
    let irq = unique(
        bytes,
        config..end,
        layout.irq,
        Some((layout.irq_call, update)),
        budget,
    )?;
    let stop = branch_target(bytes, play + 4).ok_or(ReadError::Invalid)?;
    if stop < config
        || stop >= end
        || !sig::matches(bytes, stop, layout.stop)
        || branch_target(bytes, irq + layout.irq_call) != Some(update)
    {
        return Err(ReadError::Invalid);
    }
    let mut setup = vec![
        span(bytes, config, layout.config_len)?,
        span(bytes, play, layout.play_len)?,
        span(bytes, stop, layout.stop_len)?,
        span(bytes, update, 64)?,
        span(bytes, irq, layout.irq_len)?,
        span(bytes, module, 0x1000)?,
        span(bytes, do_config, 32)?,
    ];
    let config_state = literal(bytes, config + layout.config_state, &mut setup)?;
    let song_state = literal(bytes, play + layout.play_state, &mut setup)?;
    let pending = literal(bytes, update + layout.pending, &mut setup)?;
    if !ram(config_state)
        || !ram(song_state)
        || !ram(pending)
        || literal(bytes, play + 8, &mut setup)? != config_state
        || literal(bytes, stop + layout.stop_state, &mut setup)? != song_state
        || literal(bytes, module + 0xc, &mut setup)? != song_state
        || literal(bytes, irq + layout.irq_pending, &mut setup)? != pending
    {
        return Err(ReadError::Invalid);
    }
    let jump = rom_literal(bytes, config + 0x18, 32, &mut setup)?;
    for index in 0..8 {
        let target =
            word(bytes, jump.effective_offset as usize + index * 4).ok_or(ReadError::Invalid)?;
        if target & 1 != 0
            || target < ROM_BASE + config as u32
            || target >= ROM_BASE + (config + layout.config_len) as u32
        {
            return Err(ReadError::Invalid);
        }
    }
    let count = rom_literal(bytes, play + 0x18, 4, &mut setup)?;
    let number = half(bytes, count.effective_offset as usize).ok_or(ReadError::Invalid)? as usize;
    if !(1..=256).contains(&number) || half(bytes, count.effective_offset as usize + 2) != Some(0) {
        return Err(ReadError::Invalid);
    }
    let tables = AasTables {
        count,
        sample_headers: rom_literal(
            bytes,
            module + 0xc2,
            number * INSTRUMENTS_PER_SONG * SAMPLE_HEADER_BYTES,
            &mut setup,
        )?,
        sequence: rom_literal(
            bytes,
            play + layout.sequence,
            number * MAX_ORDERS * usize::from(layout.max_channels) * 2,
            &mut setup,
        )?,
        channels: rom_literal(bytes, play + 0x22, number, &mut setup)?,
        restart: rom_literal(bytes, module + layout.restart, number, &mut setup)?,
        sample_data: rom_literal(bytes, module + layout.sample_data, 1, &mut setup)?,
        pattern_data: rom_literal(bytes, play + layout.patterns, PATTERN_BYTES, &mut setup)?,
    };
    if !tables.sample_headers.effective_offset.is_multiple_of(4)
        || !tables.sequence.effective_offset.is_multiple_of(2)
        || !tables.pattern_data.effective_offset.is_multiple_of(4)
    {
        return Err(ReadError::Invalid);
    }
    setup.push(caller(bytes, config, budget)?);
    setup.sort_unstable();
    setup.dedup();
    Ok(AasNativeProfile {
        config: RomSpan::new(config, layout.config_len),
        play: RomSpan::new(play, layout.play_len),
        stop: RomSpan::new(stop, layout.stop_len),
        update: RomSpan::new(update, 64),
        timer1_irq: RomSpan::new(irq, layout.irq_len),
        max_channels: layout.max_channels,
        config_state,
        song_state,
        tables,
        setup_spans: setup,
    })
}

fn unique(
    bytes: &[u8],
    range: std::ops::Range<usize>,
    pattern: &[u16],
    call: Option<(usize, usize)>,
    budget: &mut Budget<'_>,
) -> ReadResult<usize> {
    let mut found = None;
    for at in range.step_by(2) {
        budget.charge()?;
        if sig::matches(bytes, at, pattern)
            && call.is_none_or(|(delta, target)| branch_target(bytes, at + delta) == Some(target))
            && found.replace(at).is_some()
        {
            return Err(ReadError::Invalid);
        }
    }
    found.ok_or(ReadError::Invalid)
}

fn caller(bytes: &[u8], config: usize, budget: &mut Budget<'_>) -> ReadResult<RomSpan> {
    for at in (0..bytes.len().saturating_sub(3)).step_by(2) {
        budget.charge()?;
        if branch_target(bytes, at) == Some(config) {
            return span(bytes, at, 4);
        }
        if at % 4 != 0 || word(bytes, at) != Some(0xe1a0_e00f) {
            continue;
        }
        let bx = word(bytes, at + 4).ok_or(ReadError::Invalid)?;
        if bx & 0xffff_fff0 != 0xe12f_ff10 {
            continue;
        }
        let register = bx & 15;
        for distance in [4, 8] {
            let Some(load) = at.checked_sub(distance) else {
                continue;
            };
            let Some(instruction) = word(bytes, load) else {
                continue;
            };
            if instruction & 0xffff_f000 != (0xe59f_0000 | register << 12) {
                continue;
            }
            let slot = load + 8 + (instruction & 0xfff) as usize;
            if word(bytes, slot) == Some(ROM_BASE + config as u32 + 1) {
                return span(bytes, load, at + 8 - load);
            }
        }
    }
    Err(ReadError::Invalid)
}

fn rom_literal(
    bytes: &[u8],
    at: usize,
    length: usize,
    setup: &mut Vec<RomSpan>,
) -> ReadResult<RomSpan> {
    let value = literal(bytes, at, setup)?;
    let offset = value.checked_sub(ROM_BASE).ok_or(ReadError::Invalid)? as usize;
    let data = span(bytes, offset, length)?;
    if length <= 32 {
        setup.push(data);
    }
    Ok(data)
}

fn literal(bytes: &[u8], at: usize, setup: &mut Vec<RomSpan>) -> ReadResult<u32> {
    let instruction = half(bytes, at).ok_or(ReadError::Invalid)?;
    if instruction & 0xf800 != 0x4800 {
        return Err(ReadError::Invalid);
    }
    let slot = ((at + 4) & !3) + usize::from(instruction & 255) * 4;
    setup.push(span(bytes, slot, 4)?);
    word(bytes, slot).ok_or(ReadError::Invalid)
}

fn ram(value: u32) -> bool {
    (0x0200_0000..0x0204_0000).contains(&value) || (0x0300_0000..0x0300_7e00).contains(&value)
}

pub(super) fn branch_target(bytes: &[u8], at: usize) -> Option<usize> {
    let high = half(bytes, at)?;
    let low = half(bytes, at + 2)?;
    if high & 0xf800 != 0xf000 || low & 0xf800 != 0xf800 {
        return None;
    }
    let relative =
        ((((u32::from(high) & 0x7ff) << 12) | ((u32::from(low) & 0x7ff) << 1)) as i32) << 9 >> 9;
    let target = at as i64 + 4 + i64::from(relative);
    (target >= 0 && target < bytes.len() as i64).then_some(target as usize)
}
