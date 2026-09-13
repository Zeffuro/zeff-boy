use super::{
    Budget, DescriptorMidiNativeProfile, DescriptorMidiPlayer, MAX_ROM_BYTES, ReadError,
    ReadResult, RomSpan, ScanStop, half, signatures as sig, span, word,
};

const ROM_BASE: u32 = 0x0800_0000;
const MAX_PROFILES: usize = 8;

pub(super) fn recognize(
    bytes: &[u8],
    budget: &mut Budget<'_>,
) -> Result<Vec<DescriptorMidiNativeProfile>, ScanStop> {
    let mut profiles = Vec::new();
    if bytes.len() > MAX_ROM_BYTES {
        return Ok(profiles);
    }
    for selector in (0..bytes.len().saturating_sub(44)).step_by(4) {
        budget.charge()?;
        if half(bytes, selector) != Some(sig::SELECTOR[0])
            || !sig::matches(bytes, selector, sig::SELECTOR)
        {
            continue;
        }
        match inspect(bytes, selector, budget) {
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
    Ok(profiles)
}

fn inspect(
    bytes: &[u8],
    selector: usize,
    budget: &mut Budget<'_>,
) -> ReadResult<DescriptorMidiNativeProfile> {
    let descriptor_play = selector.checked_sub(0x1c0).ok_or(ReadError::Invalid)?;
    let dma = selector.checked_sub(0x2238).ok_or(ReadError::Invalid)?;
    let init = selector + 0x1248;
    let update = selector + 0xc48;
    let configure = selector + 0xe18;
    let sample = selector.checked_sub(0x20f0).ok_or(ReadError::Invalid)?;
    let voice = selector.checked_sub(0xe58).ok_or(ReadError::Invalid)?;
    for (at, pattern) in [
        (descriptor_play, sig::PLAY),
        (dma, sig::DMA),
        (init, sig::INIT),
        (update, sig::UPDATE),
        (update + 0x38, sig::UPDATE_PLAYERS),
        (configure, sig::CONFIGURE),
        (selector + 0x798, sig::EVENT),
        (voice, sig::VOICE),
        (sample, sig::SAMPLE),
    ] {
        budget.charge()?;
        if !sig::matches(bytes, at, pattern) {
            return Err(ReadError::Invalid);
        }
    }
    if thumb_bl(bytes, selector + 0x1c) != Some(descriptor_play)
        || thumb_bl(bytes, init + 0x22) != selector.checked_sub(0x1f6c)
        || thumb_bl(bytes, init + 0x50) != selector.checked_sub(0x1538)
        || thumb_bl(bytes, descriptor_play + 0x48) != selector.checked_sub(0x1538)
    {
        return Err(ReadError::Invalid);
    }
    let player_table = literal_pointer(bytes, selector + 4, 9 * 12)?;
    let songs = literal_pointer(bytes, selector + 6, 8)?;
    let configs = literal_pointer(bytes, init + 0x34, 9 * 20)?;
    let player_states = literal_pointer(bytes, update + 0x38, 9 * 4)?;
    let songs_end = literal_pointer(bytes, update + 0xa4, 4)?;
    let banks = literal_pointer(bytes, descriptor_play + 0x4c, 4)?;
    if songs_end <= songs
        || !(songs_end - songs).is_multiple_of(8)
        || (songs_end - songs) / 8 > usize::from(u16::MAX) + 1
        || player_states != songs_end + 12
        || configs != player_states + 9 * 4
        || player_table != configs + 9 * 20 + 4
        || word(bytes, songs_end) != Some(8)
        || word(bytes, player_table - 4) != Some(9)
    {
        return Err(ReadError::Invalid);
    }
    let mut players = Vec::new();
    let mut ram_ranges = Vec::new();
    for index in 0..9 {
        budget.charge()?;
        let at = configs + index * 20;
        let flags = half(bytes, at).ok_or(ReadError::Invalid)?;
        let channels = ((flags >> 5) & 31) as u8;
        let channel_data = word(bytes, at + 4).ok_or(ReadError::Invalid)?;
        let voice_state = word(bytes, at + 8).ok_or(ReadError::Invalid)?;
        let track_data = word(bytes, at + 12).ok_or(ReadError::Invalid)?;
        let state = word(bytes, at + 16).ok_or(ReadError::Invalid)?;
        if flags & 31 != index as u16
            || !(1..=16).contains(&channels)
            || word(bytes, player_states + index * 4) != Some(state)
            || word(bytes, player_table + index * 12) != Some(state)
            || !ram_span(channel_data, u32::from(channels) * 32)
            || !ram_span(voice_state, 40)
            || !ram_span(track_data, u32::from(channels) * 36)
            || !ram_span(state, 48)
        {
            return Err(ReadError::Invalid);
        }
        for (start, length) in [
            (channel_data, u32::from(channels) * 32),
            (voice_state, 40),
            (track_data, u32::from(channels) * 36),
            (state, 48),
        ] {
            let end = start + length;
            if ram_ranges
                .iter()
                .any(|&(other_start, other_end)| start < other_end && other_start < end)
            {
                return Err(ReadError::Invalid);
            }
            ram_ranges.push((start, end));
        }
        players.push(DescriptorMidiPlayer {
            state,
            voice_state,
            channels,
        });
    }
    let handoff = handoff(bytes, init, configure, budget)?;
    let init_span = span(bytes, init, 0xf0)?;
    let configure_span = span(bytes, configure, 0x18)?;
    let selector_span = span(bytes, selector, 44)?;
    let play_span = span(bytes, descriptor_play, 0x1c0)?;
    let update_span = span(bytes, update, 0x1d0)?;
    let dma_span = span(bytes, dma, 0x148)?;
    let player_span = span(bytes, player_table, 9 * 12)?;
    let config_span = span(bytes, configs, 9 * 20)?;
    let mut setup_spans = vec![
        init_span,
        configure_span,
        selector_span,
        play_span,
        update_span,
        dma_span,
        player_span,
        config_span,
        handoff,
        span(bytes, handoff.effective_offset as usize - 16, 16)?,
        span(bytes, songs_end, 12 + 9 * 4)?,
        span(bytes, player_table - 4, 4)?,
    ];
    for (at, length) in [(descriptor_play + 0x16e, 2), (descriptor_play + 0x17a, 2)] {
        let marker = literal_pointer(bytes, at, length)?;
        let expected = if at == descriptor_play + 0x16e {
            b'['
        } else {
            b']'
        };
        if bytes.get(marker..marker + 2) != Some(&[expected, 0]) {
            return Err(ReadError::Invalid);
        }
        setup_spans.push(span(bytes, marker, length)?);
    }
    setup_spans.sort_unstable();
    setup_spans.dedup();
    Ok(DescriptorMidiNativeProfile {
        init: init_span,
        configure: configure_span,
        handoff,
        play: selector_span,
        descriptor_play: play_span,
        update: update_span,
        dma_irq: dma_span,
        song_table: span(bytes, songs, songs_end - songs)?,
        player_table: player_span,
        player_config: config_span,
        bank_table: span(bytes, banks, 4)?,
        players,
        setup_spans,
    })
}

fn handoff(
    bytes: &[u8],
    init: usize,
    configure: usize,
    budget: &mut Budget<'_>,
) -> ReadResult<RomSpan> {
    let mut found = None;
    for at in (0..bytes.len().saturating_sub(28)).step_by(2) {
        budget.charge()?;
        if thumb_bl(bytes, at) != Some(init)
            || half(bytes, at + 4) != Some(0x2023)
            || half(bytes, at + 6) != Some(0x2102)
            || half(bytes, at + 8) != Some(0x2202)
            || half(bytes, at + 10) != Some(0x2304)
            || thumb_bl(bytes, at + 12) != Some(configure)
        {
            continue;
        }
        let hook = at + 16;
        if hook % 4 != 0
            || (0..6).any(|i| {
                half(bytes, hook + i * 2)
                    != Some([0x4927, 0x2008, 0x8008, 0x4927, 0x4a27, 0x1c10][i])
            })
            || thumb_literal(bytes, hook) != Some(0x0400_0004)
            || thumb_literal(bytes, hook + 6) != Some(0x0400_0200)
            || thumb_literal(bytes, hook + 8) != Some(0x2401)
            || thumb_literal(bytes, configure + 2) != thumb_literal(bytes, init + 0x8c)
            || found.is_some()
        {
            return Err(ReadError::Invalid);
        }
        found = Some(span(bytes, hook, 12)?);
    }
    found.ok_or(ReadError::Invalid)
}

fn ram_span(address: u32, length: u32) -> bool {
    address.is_multiple_of(4)
        && address >= 0x0300_0000
        && address
            .checked_add(length)
            .is_some_and(|end| end <= 0x0300_7c00)
}

pub(super) fn thumb_literal(bytes: &[u8], at: usize) -> Option<u32> {
    let op = half(bytes, at)?;
    if op & 0xf800 != 0x4800 {
        return None;
    }
    word(bytes, ((at + 4) & !3) + usize::from(op & 255) * 4)
}

fn literal_pointer(bytes: &[u8], at: usize, length: usize) -> ReadResult<usize> {
    let value = thumb_literal(bytes, at).ok_or(ReadError::Invalid)?;
    super::rom_pointer(bytes, value, length, 4).ok_or(ReadError::Invalid)
}

pub(super) fn thumb_bl(bytes: &[u8], at: usize) -> Option<usize> {
    let hi = half(bytes, at)?;
    if hi & 0xf800 != 0xf000 {
        return None;
    }
    let lo = half(bytes, at + 2)?;
    if lo & 0xf800 != 0xf800 {
        return None;
    }
    let displacement = (u32::from(hi & 0x7ff) << 12) | (u32::from(lo & 0x7ff) << 1);
    let signed = ((displacement << 9) as i32) >> 9;
    let target = i64::from(ROM_BASE) + at as i64 + 4 + i64::from(signed);
    let address = u32::try_from(target).ok()?;
    super::rom_pointer(bytes, address, 2, 2)
}
