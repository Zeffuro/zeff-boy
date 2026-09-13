use std::collections::HashSet;

use anyhow::{Context, Result, ensure};

#[cfg(test)]
use super::bank::Preset;
use super::bank::{InstrumentBank, Region, Sample};

const MAX_OUTPUT_BYTES: usize = 128 * 1024 * 1024;
const MAX_SAMPLES: usize = 4096;
const MAX_PRESETS: usize = 4096;
const MAX_ZONES: usize = 4096;
const GUARD_POINTS: usize = 46;
const MIN_SAMPLE_POINTS: usize = 48;
const MIN_LOOP_POINTS: usize = 32;
const LOOP_MARGIN_POINTS: usize = 8;
const GENERATORS_PER_ZONE: usize = 12;
const MAX_INFO_TEXT_BYTES: usize = 1024 * 1024;

const PAN: u16 = 17;
const ATTACK_VOL_ENV: u16 = 34;
const DECAY_VOL_ENV: u16 = 36;
const SUSTAIN_VOL_ENV: u16 = 37;
const RELEASE_VOL_ENV: u16 = 38;
const INSTRUMENT: u16 = 41;
const KEY_RANGE: u16 = 43;
const COARSE_TUNE: u16 = 51;
const FINE_TUNE: u16 = 52;
const SAMPLE_ID: u16 = 53;
const SAMPLE_MODES: u16 = 54;
const SCALE_TUNING: u16 = 56;
const OVERRIDING_ROOT_KEY: u16 = 58;

pub(crate) fn encode(bank: &InstrumentBank) -> Result<Vec<u8>> {
    let prepared = prepare(bank)?;
    let bank = &prepared;
    let layout = validate(bank)?;
    let mut riff = Vec::with_capacity(layout.output_bytes);
    riff.extend_from_slice(b"RIFF");
    write_u32(&mut riff, u32_len(layout.output_bytes - 8, "RIFF payload")?);
    riff.extend_from_slice(b"sfbk");

    append_chunk(&mut riff, b"LIST", &encode_info(bank)?)?;
    append_chunk(&mut riff, b"LIST", &encode_sample_data(bank)?)?;
    append_chunk(&mut riff, b"LIST", &encode_preset_data(bank, &layout)?)?;
    ensure!(
        riff.len() == layout.output_bytes,
        "SoundFont layout length changed while encoding"
    );
    Ok(riff)
}

pub(super) fn prepare(bank: &InstrumentBank) -> Result<InstrumentBank> {
    super::bank::validate(bank)?;
    let mut result = bank.clone();
    for (index, sample) in result.samples.iter_mut().enumerate() {
        let old_root = sample.original_pitch;
        while !(400..=50_000).contains(&sample.integer_rate()) {
            let (rate, root) = if sample.sample_rate < 400.0 {
                (
                    sample.sample_rate * 2.0,
                    i16::from(sample.original_pitch) + 12,
                )
            } else {
                (
                    sample.sample_rate / 2.0,
                    i16::from(sample.original_pitch) - 12,
                )
            };
            ensure!(
                (0..=127).contains(&root),
                "PCM rate cannot be represented by a valid SoundFont root key"
            );
            sample.sample_rate = rate;
            sample.original_pitch = root as u8;
        }
        let delta = i16::from(sample.original_pitch) - i16::from(old_root);
        for region in result
            .presets
            .iter_mut()
            .flat_map(|preset| &mut preset.zones)
            .filter(|region| region.sample_index == index)
        {
            if region.scale_tuning == 100 {
                let root = i16::from(region.root_key) + delta;
                ensure!(
                    (0..=127).contains(&root),
                    "normalized SoundFont region root is out of range"
                );
                region.root_key = root as u8;
            } else {
                region.coarse_tune -= delta;
            }
        }
        if sample.loop_range.is_some_and(|(start, end)| {
            start < 8 || end - start < 32 || end as usize > sample.pcm.len().saturating_sub(8)
        }) || sample.pcm.len() < MIN_SAMPLE_POINTS
        {
            *sample = normalize_sample(sample.clone());
        }
    }
    super::bank::validate(&result)?;
    Ok(result)
}

fn normalize_sample(mut sample: Sample) -> Sample {
    let Some((loop_start, loop_end)) = sample.loop_range else {
        sample
            .pcm
            .resize(sample.pcm.len().max(MIN_SAMPLE_POINTS), 0);
        return sample;
    };
    let start = loop_start as usize;
    let end = loop_end as usize;
    let original_loop = &sample.pcm[start..end];
    let prefix_points = LOOP_MARGIN_POINTS.saturating_sub(start);
    let mut before = sample.pcm[..start].to_vec();
    before.extend(original_loop.iter().cycle().take(prefix_points));
    let period = original_loop.len();
    let loop_len = period * MIN_LOOP_POINTS.div_ceil(period);
    // Rotating the loop after its consumed prefix preserves phase without duplicating a large cycle.
    let loop_points = (0..loop_len)
        .map(|index| original_loop[(prefix_points + index) % period])
        .collect::<Vec<_>>();
    let new_start = before.len() as u32;
    let new_end = new_start + loop_points.len() as u32;
    before.extend_from_slice(&loop_points);
    before.extend(loop_points.iter().cycle().take(LOOP_MARGIN_POINTS));
    sample.pcm = before;
    sample.loop_range = Some((new_start, new_end));
    sample
}

struct Layout {
    zone_count: usize,
    output_bytes: usize,
}

fn validate(bank: &InstrumentBank) -> Result<Layout> {
    validate_text(&bank.name, "bank name")?;
    validate_text(&bank.comment, "bank comment")?;
    ensure!(
        !bank.samples.is_empty(),
        "a SoundFont must contain at least one sample"
    );
    ensure!(
        !bank.presets.is_empty(),
        "a SoundFont must contain at least one preset"
    );
    ensure!(
        bank.samples.len() <= MAX_SAMPLES,
        "SoundFont has more than {MAX_SAMPLES} samples"
    );
    ensure!(
        bank.presets.len() <= MAX_PRESETS,
        "SoundFont has more than {MAX_PRESETS} presets"
    );

    let mut sample_names = HashSet::new();
    let mut sample_points = 0usize;
    for (index, sample) in bank.samples.iter().enumerate() {
        ensure!(
            sample_names.insert(&sample.name),
            "sample {index} has a duplicate name"
        );
        validate_fixed_name(&sample.name, "sample name")?;
        ensure!(
            sample.pcm.len() >= MIN_SAMPLE_POINTS,
            "sample {index} has fewer than {MIN_SAMPLE_POINTS} PCM points"
        );
        ensure!(
            (400..=50_000).contains(&sample.integer_rate()),
            "sample {index} has an unsupported sample rate"
        );
        ensure!(
            i8::try_from(sample.pitch_correction()).is_ok(),
            "sample {index} has unrepresentable SoundFont pitch correction"
        );
        ensure!(
            sample.original_pitch <= 127 || sample.original_pitch == 255,
            "sample {index} has an invalid original MIDI pitch"
        );
        validate_loop(index, sample)?;
        sample_points = sample_points
            .checked_add(sample.pcm.len())
            .and_then(|value| value.checked_add(GUARD_POINTS))
            .context("SoundFont sample point count overflows")?;
    }

    let mut preset_names = HashSet::new();
    let mut preset_locations = HashSet::new();
    let mut zone_count = 0usize;
    for (preset_index, preset) in bank.presets.iter().enumerate() {
        ensure!(
            preset_names.insert(&preset.name),
            "preset {preset_index} has a duplicate name"
        );
        validate_fixed_name(&preset.name, "preset name")?;
        ensure!(
            preset.program <= 127,
            "preset {preset_index} has an invalid MIDI program"
        );
        ensure!(
            preset.bank <= 128,
            "preset {preset_index} has an invalid MIDI bank"
        );
        ensure!(
            preset_locations.insert((preset.bank, preset.program)),
            "preset {preset_index} duplicates a bank/program pair"
        );
        ensure!(
            !preset.zones.is_empty(),
            "preset {preset_index} has no zones"
        );
        for (zone_index, zone) in preset.zones.iter().enumerate() {
            validate_zone(preset_index, zone_index, zone, bank.samples.len())?;
        }
        zone_count = zone_count
            .checked_add(preset.zones.len())
            .context("SoundFont zone count overflows")?;
    }
    ensure!(
        zone_count <= MAX_ZONES,
        "SoundFont has more than {MAX_ZONES} zones"
    );
    ensure!(
        zone_count <= u16::MAX as usize,
        "SoundFont zone indices exceed SF2 limits"
    );
    ensure!(
        zone_count
            .checked_mul(GENERATORS_PER_ZONE)
            .is_some_and(|count| count <= u16::MAX as usize),
        "SoundFont generator indices exceed SF2 limits"
    );

    let sample_bytes = sample_points
        .checked_mul(2)
        .context("SoundFont sample data size overflows")?;
    let info_bytes = list_size(&[
        chunk_size(4)?,
        chunk_size(zstring_len("EMU8000")?)?,
        chunk_size(zstring_len(&bank.name)?)?,
        chunk_size(zstring_len(&bank.comment)?)?,
        chunk_size(zstring_len("zeff-boy")?)?,
    ])?;
    let pdta_bytes = list_size(&[
        chunk_size(records_size(bank.presets.len() + 1, 38)?)?,
        chunk_size(records_size(zone_count + 1, 4)?)?,
        chunk_size(10)?,
        chunk_size(records_size(zone_count * 2 + 1, 4)?)?,
        chunk_size(records_size(zone_count + 1, 22)?)?,
        chunk_size(records_size(zone_count + 1, 4)?)?,
        chunk_size(10)?,
        chunk_size(records_size(zone_count * GENERATORS_PER_ZONE + 1, 4)?)?,
        chunk_size(records_size(bank.samples.len() + 1, 46)?)?,
    ])?;
    let sdta_bytes = list_size(&[chunk_size(sample_bytes)?])?;
    let info_chunk_bytes = chunk_size(info_bytes)?;
    let sdta_chunk_bytes = chunk_size(sdta_bytes)?;
    let pdta_chunk_bytes = chunk_size(pdta_bytes)?;
    let output_bytes = 12usize
        .checked_add(info_chunk_bytes)
        .and_then(|value| value.checked_add(sdta_chunk_bytes))
        .and_then(|value| value.checked_add(pdta_chunk_bytes))
        .context("SoundFont output length overflows")?;
    ensure!(
        output_bytes <= MAX_OUTPUT_BYTES,
        "SoundFont exceeds the {MAX_OUTPUT_BYTES}-byte output limit"
    );
    u32_len(output_bytes - 8, "RIFF payload")?;

    Ok(Layout {
        zone_count,
        output_bytes,
    })
}

fn validate_text(value: &str, label: &str) -> Result<()> {
    ensure!(!value.is_empty(), "{label} must not be empty");
    ensure!(
        value.len() <= MAX_INFO_TEXT_BYTES,
        "{label} exceeds the {MAX_INFO_TEXT_BYTES}-byte limit"
    );
    ensure!(
        value.bytes().all(|byte| byte.is_ascii()
            && byte != 0
            && (!byte.is_ascii_control() || matches!(byte, b'\t' | b'\n' | b'\r'))),
        "{label} contains unsupported characters"
    );
    Ok(())
}

fn validate_fixed_name(value: &str, label: &str) -> Result<()> {
    validate_text(value, label)?;
    ensure!(value.len() <= 20, "{label} exceeds SF2's 20-byte limit");
    Ok(())
}

fn validate_loop(index: usize, sample: &Sample) -> Result<()> {
    let Some((start, end)) = sample.loop_range else {
        return Ok(());
    };
    let start = usize::try_from(start).context("loop start does not fit this platform")?;
    let end = usize::try_from(end).context("loop end does not fit this platform")?;
    ensure!(
        start >= 8,
        "sample {index} loop lacks the required eight points before its start"
    );
    ensure!(
        end >= start
            .checked_add(32)
            .context("sample loop length overflows")?,
        "sample {index} loop is shorter than 32 points"
    );
    ensure!(
        end <= sample.pcm.len().saturating_sub(8),
        "sample {index} loop lacks the required eight points after its end"
    );
    Ok(())
}

fn validate_zone(
    preset_index: usize,
    zone_index: usize,
    zone: &Region,
    sample_count: usize,
) -> Result<()> {
    ensure!(
        zone.key_start <= zone.key_end && zone.key_end <= 127,
        "preset {preset_index} zone {zone_index} has an invalid inclusive MIDI key range"
    );
    ensure!(
        zone.sample_index < sample_count,
        "preset {preset_index} zone {zone_index} references a missing sample"
    );
    ensure!(
        zone.root_key <= 127,
        "preset {preset_index} zone {zone_index} has an invalid MIDI root key"
    );
    ensure!(
        (0..=1200).contains(&zone.scale_tuning),
        "preset {preset_index} zone {zone_index} has an invalid scale tuning"
    );
    ensure!(
        (-120..=120).contains(&zone.coarse_tune),
        "preset {preset_index} zone {zone_index} has an invalid coarse tune"
    );
    ensure!(
        (-99..=99).contains(&zone.fine_tune),
        "preset {preset_index} zone {zone_index} has an invalid fine tune"
    );
    ensure!(
        (-500..=500).contains(&zone.pan),
        "preset {preset_index} zone {zone_index} has an invalid pan"
    );
    timecents(zone.attack_seconds, "attack", preset_index, zone_index)?;
    timecents(zone.decay_seconds, "decay", preset_index, zone_index)?;
    timecents(zone.release_seconds, "release", preset_index, zone_index)?;
    ensure!(
        zone.sustain_level.is_finite() && (0.0..=1.0).contains(&zone.sustain_level),
        "preset {preset_index} zone {zone_index} has an invalid sustain amplitude"
    );
    Ok(())
}

fn encode_info(bank: &InstrumentBank) -> Result<Vec<u8>> {
    let mut info = b"INFO".to_vec();
    append_chunk(&mut info, b"ifil", &[2, 0, 1, 0])?;
    append_chunk(&mut info, b"isng", &zstring("EMU8000")?)?;
    append_chunk(&mut info, b"INAM", &zstring(&bank.name)?)?;
    append_chunk(&mut info, b"ICMT", &zstring(&bank.comment)?)?;
    append_chunk(&mut info, b"ISFT", &zstring("zeff-boy")?)?;
    Ok(info)
}

fn encode_sample_data(bank: &InstrumentBank) -> Result<Vec<u8>> {
    let mut sdta = b"sdta".to_vec();
    let mut samples = Vec::new();
    for sample in &bank.samples {
        for point in &sample.pcm {
            write_i16(&mut samples, *point);
        }
        for _ in 0..GUARD_POINTS {
            write_i16(&mut samples, 0);
        }
    }
    append_chunk(&mut sdta, b"smpl", &samples)?;
    Ok(sdta)
}

fn encode_preset_data(bank: &InstrumentBank, layout: &Layout) -> Result<Vec<u8>> {
    let mut pdta = b"pdta".to_vec();
    append_chunk(&mut pdta, b"phdr", &encode_phdr(bank)?)?;
    append_chunk(&mut pdta, b"pbag", &encode_pbag(bank, layout.zone_count)?)?;
    append_chunk(&mut pdta, b"pmod", &[0; 10])?;
    append_chunk(&mut pdta, b"pgen", &encode_pgen(bank)?)?;
    append_chunk(&mut pdta, b"inst", &encode_inst(bank, layout.zone_count)?)?;
    append_chunk(&mut pdta, b"ibag", &encode_ibag(layout.zone_count)?)?;
    append_chunk(&mut pdta, b"imod", &[0; 10])?;
    append_chunk(&mut pdta, b"igen", &encode_igen(bank)?)?;
    append_chunk(&mut pdta, b"shdr", &encode_shdr(bank)?)?;
    Ok(pdta)
}

fn encode_phdr(bank: &InstrumentBank) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut bag_index = 0usize;
    for preset in &bank.presets {
        write_fixed_name(&mut bytes, &preset.name)?;
        write_u16(&mut bytes, preset.program);
        write_u16(&mut bytes, preset.bank);
        write_u16(&mut bytes, u16_len(bag_index, "preset bag index")?);
        bytes.extend_from_slice(&[0; 12]);
        bag_index += preset.zones.len();
    }
    write_fixed_name(&mut bytes, "EOP")?;
    bytes.extend_from_slice(&[0; 4]);
    write_u16(&mut bytes, u16_len(bag_index, "terminal preset bag index")?);
    bytes.extend_from_slice(&[0; 12]);
    Ok(bytes)
}

fn encode_pbag(bank: &InstrumentBank, zone_count: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut generator_index = 0usize;
    for preset in &bank.presets {
        for _ in &preset.zones {
            write_u16(
                &mut bytes,
                u16_len(generator_index, "preset generator index")?,
            );
            write_u16(&mut bytes, 0);
            generator_index += 2;
        }
    }
    write_u16(
        &mut bytes,
        u16_len(generator_index, "terminal preset generator index")?,
    );
    write_u16(&mut bytes, 0);
    ensure!(
        bytes.len() == records_size(zone_count + 1, 4)?,
        "preset bag count changed while encoding"
    );
    Ok(bytes)
}

fn encode_pgen(bank: &InstrumentBank) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut instrument_index = 0usize;
    for preset in &bank.presets {
        for zone in &preset.zones {
            generator_range(&mut bytes, KEY_RANGE, zone.key_start, zone.key_end);
            generator_u16(
                &mut bytes,
                INSTRUMENT,
                u16_len(instrument_index, "instrument index")?,
            );
            instrument_index += 1;
        }
    }
    bytes.extend_from_slice(&[0; 4]);
    Ok(bytes)
}

fn encode_inst(bank: &InstrumentBank, zone_count: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for (index, _) in bank
        .presets
        .iter()
        .flat_map(|preset| preset.zones.iter())
        .enumerate()
    {
        write_fixed_name(&mut bytes, &format!("I{index:04}"))?;
        write_u16(&mut bytes, u16_len(index, "instrument bag index")?);
    }
    write_fixed_name(&mut bytes, "EOI")?;
    write_u16(
        &mut bytes,
        u16_len(zone_count, "terminal instrument bag index")?,
    );
    Ok(bytes)
}

fn encode_ibag(zone_count: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for index in 0..zone_count {
        write_u16(
            &mut bytes,
            u16_len(index * GENERATORS_PER_ZONE, "instrument generator index")?,
        );
        write_u16(&mut bytes, 0);
    }
    write_u16(
        &mut bytes,
        u16_len(
            zone_count * GENERATORS_PER_ZONE,
            "terminal instrument generator index",
        )?,
    );
    write_u16(&mut bytes, 0);
    Ok(bytes)
}

fn encode_igen(bank: &InstrumentBank) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for preset in &bank.presets {
        for zone in &preset.zones {
            generator_range(&mut bytes, KEY_RANGE, zone.key_start, zone.key_end);
            generator_i16(&mut bytes, PAN, zone.pan);
            generator_i16(
                &mut bytes,
                ATTACK_VOL_ENV,
                timecents(zone.attack_seconds, "attack", 0, 0)?,
            );
            generator_i16(
                &mut bytes,
                DECAY_VOL_ENV,
                timecents(sf2_decay_seconds(zone)?, "decay", 0, 0)?,
            );
            generator_i16(
                &mut bytes,
                SUSTAIN_VOL_ENV,
                sustain_centibels(zone.sustain_level)?,
            );
            generator_i16(
                &mut bytes,
                RELEASE_VOL_ENV,
                timecents(zone.release_seconds, "release", 0, 0)?,
            );
            generator_i16(&mut bytes, COARSE_TUNE, zone.coarse_tune);
            generator_i16(&mut bytes, FINE_TUNE, zone.fine_tune);
            generator_u16(
                &mut bytes,
                SAMPLE_MODES,
                if bank.samples[zone.sample_index].loop_range.is_some() {
                    1
                } else {
                    0
                },
            );
            generator_i16(&mut bytes, SCALE_TUNING, zone.scale_tuning);
            generator_u16(&mut bytes, OVERRIDING_ROOT_KEY, u16::from(zone.root_key));
            generator_u16(
                &mut bytes,
                SAMPLE_ID,
                u16_len(zone.sample_index, "sample index")?,
            );
        }
    }
    bytes.extend_from_slice(&[0; 4]);
    Ok(bytes)
}

fn encode_shdr(bank: &InstrumentBank) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut sample_start = 0usize;
    for sample in &bank.samples {
        let sample_end = sample_start
            .checked_add(sample.pcm.len())
            .context("sample end overflows")?;
        let (loop_start, loop_end) = sample
            .loop_range
            .map_or((sample_start + 8, sample_end - 8), |(start, end)| {
                (sample_start + start as usize, sample_start + end as usize)
            });
        write_fixed_name(&mut bytes, &sample.name)?;
        write_u32(&mut bytes, u32_len(sample_start, "sample start")?);
        write_u32(&mut bytes, u32_len(sample_end, "sample end")?);
        write_u32(&mut bytes, u32_len(loop_start, "sample loop start")?);
        write_u32(&mut bytes, u32_len(loop_end, "sample loop end")?);
        write_u32(&mut bytes, sample.integer_rate());
        bytes.push(sample.original_pitch);
        bytes.push(sample.pitch_correction() as u8);
        write_u16(&mut bytes, 0);
        write_u16(&mut bytes, 1);
        sample_start = sample_end
            .checked_add(GUARD_POINTS)
            .context("next sample start overflows")?;
    }
    write_fixed_name(&mut bytes, "EOS")?;
    bytes.extend_from_slice(&[0; 26]);
    Ok(bytes)
}

fn timecents(seconds: f32, name: &str, preset_index: usize, zone_index: usize) -> Result<i16> {
    ensure!(
        seconds.is_finite() && seconds >= 0.0,
        "preset {preset_index} zone {zone_index} has an invalid {name} time"
    );
    if seconds == 0.0 {
        return Ok(i16::MIN);
    }
    let value = (1200.0 * seconds.log2()).round();
    ensure!(
        (-12_000.0..=8_000.0).contains(&value),
        "preset {preset_index} zone {zone_index} has an out-of-range {name} time"
    );
    Ok(value as i16)
}

fn sustain_centibels(level: f32) -> Result<i16> {
    ensure!(
        level.is_finite() && (0.0..=1.0).contains(&level),
        "sustain amplitude must be finite and between zero and one"
    );
    if level == 0.0 {
        return Ok(1440);
    }
    Ok((-200.0 * level.log10()).round().clamp(0.0, 1440.0) as i16)
}

fn sf2_decay_seconds(zone: &Region) -> Result<f32> {
    ensure!(
        zone.decay_seconds.is_finite() && zone.decay_seconds >= 0.0,
        "decay time must be finite and nonnegative"
    );
    ensure!(
        zone.sustain_level.is_finite() && (0.0..=1.0).contains(&zone.sustain_level),
        "sustain amplitude must be finite and between zero and one"
    );
    if zone.sustain_level == 0.0 || zone.sustain_level == 1.0 {
        return Ok(zone.decay_seconds);
    }

    // SF2 decayVolEnv describes a full 100 dB descent. The portable bank stores
    // the actual duration only as far as the sustain level, so recover that rate.
    let attenuation_db = (-20.0 * zone.sustain_level.log10()).min(100.0);
    let seconds = zone.decay_seconds * (100.0 / attenuation_db);
    ensure!(seconds.is_finite(), "SoundFont decay time is not finite");
    Ok(seconds)
}

fn zstring(value: &str) -> Result<Vec<u8>> {
    validate_text(value, "INFO text")?;
    let mut bytes = value.as_bytes().to_vec();
    bytes.push(0);
    if !bytes.len().is_multiple_of(2) {
        bytes.push(0);
    }
    Ok(bytes)
}

fn zstring_len(value: &str) -> Result<usize> {
    validate_text(value, "INFO text")?;
    padded_size(
        value
            .len()
            .checked_add(1)
            .context("INFO text length overflows")?,
    )
}

fn write_fixed_name(bytes: &mut Vec<u8>, name: &str) -> Result<()> {
    validate_fixed_name(name, "SF2 fixed name")?;
    bytes.extend_from_slice(name.as_bytes());
    bytes.resize(bytes.len() + (20 - name.len()), 0);
    Ok(())
}

fn generator_range(bytes: &mut Vec<u8>, operation: u16, start: u8, end: u8) {
    write_u16(bytes, operation);
    bytes.push(start);
    bytes.push(end);
}

fn generator_i16(bytes: &mut Vec<u8>, operation: u16, amount: i16) {
    write_u16(bytes, operation);
    write_i16(bytes, amount);
}

fn generator_u16(bytes: &mut Vec<u8>, operation: u16, amount: u16) {
    write_u16(bytes, operation);
    write_u16(bytes, amount);
}

fn append_chunk(target: &mut Vec<u8>, id: &[u8; 4], data: &[u8]) -> Result<()> {
    ensure!(
        data.len() <= u32::MAX as usize,
        "SF2 chunk {:?} exceeds RIFF32 limits",
        std::str::from_utf8(id).unwrap_or("????")
    );
    target.extend_from_slice(id);
    write_u32(target, data.len() as u32);
    target.extend_from_slice(data);
    if !data.len().is_multiple_of(2) {
        target.push(0);
    }
    Ok(())
}

fn write_u16(bytes: &mut Vec<u8>, value: u16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_i16(bytes: &mut Vec<u8>, value: i16) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}

fn padded_size(size: usize) -> Result<usize> {
    size.checked_add(size % 2)
        .context("SF2 chunk padding overflows")
}

fn chunk_size(data_size: usize) -> Result<usize> {
    8usize
        .checked_add(padded_size(data_size)?)
        .context("SF2 chunk size overflows")
}

fn list_size(chunk_sizes: &[usize]) -> Result<usize> {
    chunk_sizes.iter().try_fold(4usize, |size, chunk| {
        size.checked_add(*chunk).context("SF2 LIST size overflows")
    })
}

fn records_size(count: usize, size: usize) -> Result<usize> {
    count
        .checked_mul(size)
        .context("SF2 record data size overflows")
}

fn u16_len(value: usize, label: &str) -> Result<u16> {
    u16::try_from(value).with_context(|| format!("{label} exceeds SF2's u16 limit"))
}

fn u32_len(value: usize, label: &str) -> Result<u32> {
    u32::try_from(value).with_context(|| format!("{label} exceeds RIFF32's u32 limit"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Sample {
        let mut pcm = vec![0_i16; 64];
        for (index, point) in pcm.iter_mut().enumerate() {
            *point = index as i16;
        }
        Sample {
            name: "sample".into(),
            pcm,
            sample_rate: 32_768.0,
            original_pitch: 60,
            loop_range: Some((8, 56)),
        }
    }

    fn bank() -> InstrumentBank {
        InstrumentBank {
            name: "Unit test bank".into(),
            comment: "{\"source\":\"fixture\"}".into(),
            samples: vec![sample()],
            presets: vec![Preset {
                name: "Preset".into(),
                bank: 0,
                program: 0,
                zones: vec![Region {
                    key_start: 60,
                    key_end: 72,
                    sample_index: 0,
                    root_key: 60,
                    scale_tuning: 100,
                    coarse_tune: 0,
                    fine_tune: 0,
                    pan: 0,
                    attack_seconds: 0.01,
                    decay_seconds: 0.25,
                    sustain_level: 0.5,
                    release_seconds: 0.2,
                }],
            }],
        }
    }

    #[test]
    fn encodes_a_deterministic_complete_sf2_structure() -> Result<()> {
        let first = encode(&bank())?;
        assert_eq!(first, encode(&bank())?);
        assert_eq!(&first[..4], b"RIFF");
        assert_eq!(&first[8..12], b"sfbk");
        let lists = parse_riff_lists(&first)?;
        assert_eq!(
            lists.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
            vec![*b"INFO", *b"sdta", *b"pdta"]
        );
        assert_eq!(
            chunk_ids(lists[0].1)?,
            vec![*b"ifil", *b"isng", *b"INAM", *b"ICMT", *b"ISFT"]
        );
        assert_eq!(chunk_ids(lists[1].1)?, vec![*b"smpl"]);
        assert_eq!(
            chunk_ids(lists[2].1)?,
            vec![
                *b"phdr", *b"pbag", *b"pmod", *b"pgen", *b"inst", *b"ibag", *b"imod", *b"igen",
                *b"shdr"
            ]
        );
        let smpl = find_chunk(lists[1].1, b"smpl")?;
        assert_eq!(smpl.len(), (64 + GUARD_POINTS) * 2);
        assert!(smpl[128..].iter().all(|byte| *byte == 0));
        let igen = find_chunk(lists[2].1, b"igen")?;
        assert_eq!(u16::from_le_bytes([igen[0], igen[1]]), KEY_RANGE);
        assert_eq!(&igen[2..4], &[60, 72]);
        assert_eq!(u16::from_le_bytes([igen[44], igen[45]]), SAMPLE_ID);
        assert_eq!(u16::from_le_bytes([igen[46], igen[47]]), 0);
        assert_eq!(&igen[48..52], &[0; 4]);
        let pgen = find_chunk(lists[2].1, b"pgen")?;
        assert_eq!(&pgen[..4], &[KEY_RANGE as u8, 0, 60, 72]);
        assert_eq!(&pgen[8..12], &[0; 4]);
        let shdr = find_chunk(lists[2].1, b"shdr")?;
        assert_eq!(shdr.len(), 92);
        assert_eq!(u32::from_le_bytes(shdr[20..24].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(shdr[24..28].try_into().unwrap()), 64);
        assert_eq!(u32::from_le_bytes(shdr[28..32].try_into().unwrap()), 8);
        assert_eq!(u32::from_le_bytes(shdr[32..36].try_into().unwrap()), 56);
        Ok(())
    }

    #[test]
    fn rejects_ranges_and_nonfinite_values_that_cannot_be_written_portably() {
        let mut invalid = bank();
        invalid.samples[0].loop_range = Some((56, 7));
        assert!(encode(&invalid).is_err());
        invalid = bank();
        invalid.presets[0].zones[0].attack_seconds = f32::NAN;
        assert!(encode(&invalid).is_err());
        invalid = bank();
        invalid.presets[0].zones[0].key_end = 128;
        assert!(encode(&invalid).is_err());
        invalid = bank();
        invalid.samples[0].pcm.truncate(47);
        assert!(encode(&invalid).is_err());
    }

    #[test]
    fn soundfont_decay_converts_time_to_sustain_into_full_range_rate() -> Result<()> {
        let mut region = bank().presets.remove(0).zones.remove(0);
        region.decay_seconds = 0.25;
        region.sustain_level = 0.1; // -20 dB, one fifth of SF2's full decay range.
        assert!((sf2_decay_seconds(&region)? - 1.25).abs() < 0.000_001);
        region.sustain_level = 0.0;
        assert_eq!(sf2_decay_seconds(&region)?, 0.25);
        region.sustain_level = 1.0;
        assert_eq!(sf2_decay_seconds(&region)?, 0.25);
        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rustysynth_accepts_the_complete_bank() -> Result<()> {
        let mut cursor = std::io::Cursor::new(encode(&bank())?);
        let parsed = rustysynth::SoundFont::new(&mut cursor)
            .map_err(|error| anyhow::anyhow!("RustySynth rejected generated SF2: {error}"))?;
        assert_eq!(parsed.get_sample_headers().len(), 1);
        assert_eq!(parsed.get_presets().len(), 1);
        assert_eq!(parsed.get_instruments().len(), 1);
        Ok(())
    }

    fn parse_riff_lists(bytes: &[u8]) -> Result<Vec<([u8; 4], &[u8])>> {
        ensure!(bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"sfbk");
        ensure!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()) as usize + 8 == bytes.len());
        let mut cursor = 12;
        let mut lists = Vec::new();
        while cursor < bytes.len() {
            ensure!(&bytes[cursor..cursor + 4] == b"LIST");
            let size =
                u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
            let end = cursor + 8 + size;
            ensure!(end <= bytes.len() && size >= 4);
            lists.push((
                bytes[cursor + 8..cursor + 12].try_into().unwrap(),
                &bytes[cursor + 12..end],
            ));
            cursor = end + size % 2;
        }
        Ok(lists)
    }

    fn chunk_ids(bytes: &[u8]) -> Result<Vec<[u8; 4]>> {
        let mut cursor = 0;
        let mut ids = Vec::new();
        while cursor < bytes.len() {
            ensure!(cursor + 8 <= bytes.len());
            let size =
                u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
            let end = cursor + 8 + size;
            ensure!(end <= bytes.len());
            ids.push(bytes[cursor..cursor + 4].try_into().unwrap());
            cursor = end + size % 2;
        }
        Ok(ids)
    }

    fn find_chunk<'a>(bytes: &'a [u8], wanted: &[u8; 4]) -> Result<&'a [u8]> {
        let mut cursor = 0;
        while cursor < bytes.len() {
            let size =
                u32::from_le_bytes(bytes[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
            let end = cursor + 8 + size;
            if &bytes[cursor..cursor + 4] == wanted {
                return Ok(&bytes[cursor + 8..end]);
            }
            cursor = end + size % 2;
        }
        Err(anyhow::anyhow!("missing chunk"))
    }
}
