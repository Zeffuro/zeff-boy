use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::AtomicBool;

use anyhow::{Context, Result, ensure};

use super::bank::{InstrumentBank, Preset, Region, Sample};
use super::sample::{SampleDirection, SampleEncoding, decode_sample};
use super::{InstrumentInventory, RomSpan, SampleInventory, SongCandidate, ToneInventory};

const MIXER_TICKS_PER_SECOND: f32 = 60.0;
const MAX_BANK_PCM_POINTS: usize = super::bank::MAX_PCM_POINTS;
const NOISE_SAMPLE_RATE: u32 = 4_096;
const NOISE_15_BIT_PERIOD: usize = 32_767;
const NOISE_7_BIT_PERIOD: usize = 127;
const NOISE_NR43_BY_KEY: [u8; 60] = [
    0xD7, 0xD6, 0xD5, 0xD4, 0xC7, 0xC6, 0xC5, 0xC4, 0xB7, 0xB6, 0xB5, 0xB4, 0xA7, 0xA6, 0xA5, 0xA4,
    0x97, 0x96, 0x95, 0x94, 0x87, 0x86, 0x85, 0x84, 0x77, 0x76, 0x75, 0x74, 0x67, 0x66, 0x65, 0x64,
    0x57, 0x56, 0x55, 0x54, 0x47, 0x46, 0x45, 0x44, 0x37, 0x36, 0x35, 0x34, 0x27, 0x26, 0x25, 0x24,
    0x17, 0x16, 0x15, 0x14, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01, 0x00,
];
const NOISE_DIVISORS: [u32; 8] = [8, 16, 32, 48, 64, 80, 96, 112];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SampleKind {
    Pcm,
    Synth,
    Square,
    Wave,
    Noise,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct PcmIdentity {
    frequency: u32,
    decoded_len: u32,
    encoding: SampleEncoding,
    direction: SampleDirection,
    loop_start: u32,
    looped: bool,
}

type SampleIdentity = (RomSpan, Option<u32>, SampleKind, Option<PcmIdentity>);

#[cfg(test)]
pub(crate) fn pcm_sample(bytes: &[u8], sample: &SampleInventory) -> Result<Sample> {
    pcm_sample_with_cancel(bytes, sample, &AtomicBool::new(false))
}

pub(crate) fn pcm_sample_with_cancel(
    bytes: &[u8],
    sample: &SampleInventory,
    cancel: &AtomicBool,
) -> Result<Sample> {
    ensure!(sample.frequency != 0, "PCM sample has a zero frequency");
    let sample_rate = (u64::from(sample.frequency) + 512) / 1024;
    let sample_rate = u32::try_from(sample_rate).context("PCM sample rate overflows")?;
    ensure!(
        (1..=192_000).contains(&sample_rate),
        "PCM sample rate {sample_rate} Hz is outside decoded-WAV range"
    );
    let exact_rate = sample.frequency as f64 / 1024.0;
    let decoded = decode_sample(bytes, sample, cancel)?;
    Ok(Sample {
        name: sample_name(sample, None),
        pcm: decoded.pcm,
        sample_rate: exact_rate,
        original_pitch: 60,
        loop_range: decoded.loop_range,
    })
}

fn sample_name(sample: &SampleInventory, fixed_rate: Option<u32>) -> String {
    let encoding = match sample.encoding {
        SampleEncoding::PcmS8 => 'P',
        SampleEncoding::GameFreakBdpcm => 'B',
    };
    let direction = match sample.direction {
        SampleDirection::Forward => 'F',
        SampleDirection::Reverse => 'R',
    };
    match fixed_rate {
        Some(rate) => format!(
            "F_{encoding}{direction}_{:06X}_{rate}",
            sample.data.effective_offset
        ),
        None => format!(
            "S_{encoding}{direction}_{:06X}",
            sample.data.effective_offset
        ),
    }
}

#[cfg(test)]
pub(crate) fn instrument_bank(
    bytes: &[u8],
    song: &SongCandidate,
    comment: String,
    mixer_rate: Option<u32>,
) -> Result<InstrumentBank> {
    instrument_bank_with_cancel(bytes, song, comment, mixer_rate, &AtomicBool::new(false))
}

pub(crate) fn instrument_bank_with_cancel(
    bytes: &[u8],
    song: &SongCandidate,
    comment: String,
    mixer_rate: Option<u32>,
    cancel: &AtomicBool,
) -> Result<InstrumentBank> {
    let used = used_voice_keys(song)?;
    let mut instruments = BTreeMap::new();
    for instrument in &song.instruments {
        ensure!(
            instruments.insert(instrument.voice, instrument).is_none(),
            "song has duplicate voice {}",
            instrument.voice
        );
    }

    let mut samples = Vec::new();
    let mut sample_indices = BTreeMap::new();
    let mut sample_points = 0usize;
    let mut presets = Vec::new();
    for (voice, keys) in used {
        ensure!(
            voice <= 127,
            "song voice {voice} cannot be represented by an SF2 MIDI program"
        );
        let instrument = instruments
            .get(&voice)
            .copied()
            .with_context(|| format!("song voice {voice} has no validated instrument"))?;
        let mut zones = Vec::new();
        project_instrument(
            bytes,
            instrument,
            &keys,
            &mut samples,
            &mut sample_indices,
            &mut sample_points,
            &mut zones,
            mixer_rate,
            cancel,
        )
        .with_context(|| {
            format!(
                "voice {voice} at ROM +{:08X}",
                instrument.descriptor.effective_offset
            )
        })?;
        ensure!(
            !zones.is_empty(),
            "song voice {voice} has no usable instrument regions"
        );
        let preset_name = format!("V{voice:03}_{:06X}", song.header.effective_offset);
        presets.push(Preset {
            name: format!("{preset_name}_GM"),
            bank: 0,
            program: u16::from(voice),
            zones: zones.clone(),
        });
        presets.push(Preset {
            name: format!("{preset_name}_DRM"),
            bank: 128,
            program: u16::from(voice),
            zones,
        });
    }

    let bank_name = format!("MP2K_{:06X}", song.header.effective_offset);
    let bank = InstrumentBank {
        name: bank_name,
        comment,
        samples,
        presets,
    };
    super::bank::validate(&bank)?;
    Ok(bank)
}

fn used_voice_keys(song: &SongCandidate) -> Result<BTreeMap<u8, BTreeSet<u8>>> {
    let mut result = BTreeMap::new();
    for track in &song.tracks {
        for entry in &track.voice_keys {
            result
                .entry(entry.voice)
                .or_insert_with(BTreeSet::new)
                .extend(entry.keys.iter().copied());
        }
    }
    ensure!(!result.is_empty(), "song has no referenced voices");
    result.retain(|_, keys| !keys.is_empty());
    ensure!(
        !result.is_empty(),
        "song has no explicit note keys for instrument projection"
    );
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
fn project_instrument(
    bytes: &[u8],
    instrument: &InstrumentInventory,
    keys: &BTreeSet<u8>,
    samples: &mut Vec<Sample>,
    sample_indices: &mut BTreeMap<SampleIdentity, usize>,
    sample_points: &mut usize,
    zones: &mut Vec<Region>,
    mixer_rate: Option<u32>,
    cancel: &AtomicBool,
) -> Result<()> {
    match instrument.kind {
        0x40 | 0x80 => {
            ensure!(
                !instrument.regions.is_empty(),
                "voice {} has no selected split/rhythm regions",
                instrument.voice
            );
            let mut covered = BTreeSet::new();
            for region in &instrument.regions {
                let tone = region.tone.as_ref().with_context(|| {
                    format!(
                        "voice {} region {}-{} has no usable asset",
                        instrument.voice, region.key_start, region.key_end
                    )
                })?;
                for key in region.key_start..=region.key_end {
                    ensure!(
                        keys.contains(&key),
                        "voice {} region key {key} was not referenced by the song",
                        instrument.voice
                    );
                    covered.insert(key);
                    let (key_start, key_end, coarse_tune) = if instrument.kind == 0x80 {
                        (key, key, i16::from(tone.key) - i16::from(key))
                    } else {
                        (region.key_start, region.key_end, 0)
                    };
                    let zone = project_tone(
                        bytes,
                        tone,
                        if tone.kind & 7 == 4 { key } else { key_start },
                        if tone.kind & 7 == 4 { key } else { key_end },
                        coarse_tune,
                        instrument.kind == 0x80,
                        samples,
                        sample_indices,
                        sample_points,
                        mixer_rate,
                        cancel,
                    )?;
                    if instrument.kind == 0x80 || tone.kind & 7 == 4 || key == region.key_start {
                        zones.push(zone);
                    }
                }
            }
            ensure!(
                &covered == keys,
                "voice {} has referenced keys without a mapped split/rhythm region",
                instrument.voice
            );
        }
        _ => {
            ensure!(
                instrument.regions.is_empty(),
                "voice {} has unexpected child regions",
                instrument.voice
            );
            let ranges = if instrument.tone.kind & 7 == 4 {
                keys.iter().copied().map(|key| (key, key)).collect()
            } else {
                contiguous_ranges(keys)
            };
            for (start, end) in ranges {
                zones.push(project_tone(
                    bytes,
                    &instrument.tone,
                    start,
                    end,
                    0,
                    false,
                    samples,
                    sample_indices,
                    sample_points,
                    mixer_rate,
                    cancel,
                )?);
            }
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn project_tone(
    bytes: &[u8],
    tone: &ToneInventory,
    key_start: u8,
    key_end: u8,
    coarse_tune: i16,
    rhythm: bool,
    samples: &mut Vec<Sample>,
    sample_indices: &mut BTreeMap<SampleIdentity, usize>,
    sample_points: &mut usize,
    mixer_rate: Option<u32>,
    cancel: &AtomicBool,
) -> Result<Region> {
    ensure!(
        matches!(
            tone.kind,
            0 | 8 | 0x10 | 0x18 | 0x20 | 0x28 | 0x30 | 0x38 | 1 | 9 | 2 | 10 | 3 | 11 | 4 | 12
        ),
        "MP2K tone kind {:02X} is unsupported for instrument projection",
        tone.kind
    );
    let kind = tone.kind & 7;
    let sample_index = match kind {
        0 if tone.synthesis.is_some() => {
            let recipe = tone.synthesis.as_ref().expect("matched recipe");
            sample_index_for(
                samples,
                sample_indices,
                (recipe.parameters, None, SampleKind::Synth, None),
                sample_points,
                128,
                || synth_sample(recipe),
            )?
        }
        0 => {
            let sample = tone
                .sample
                .as_ref()
                .context("PCM tone has no validated sample")?;
            let fixed_rate = tone
                .fixed_pitch
                .then(|| mixer_rate.context("fixed-pitch PCM needs the mixer sample-rate context"))
                .transpose()?;
            let max_points = sample.decoded_len as usize;
            let identity = PcmIdentity {
                frequency: sample.frequency,
                decoded_len: sample.decoded_len,
                encoding: sample.encoding,
                direction: sample.direction,
                loop_start: sample.loop_start,
                looped: sample.looped,
            };
            sample_index_for(
                samples,
                sample_indices,
                (sample.data, fixed_rate, SampleKind::Pcm, Some(identity)),
                sample_points,
                max_points,
                || {
                    if let Some(rate) = fixed_rate {
                        let mut fixed_sample = *sample;
                        fixed_sample.frequency = rate
                            .checked_mul(1024)
                            .context("fixed-pitch PCM mixer rate overflows")?;
                        let mut projected = pcm_sample_with_cancel(bytes, &fixed_sample, cancel)?;
                        projected.sample_rate = f64::from(rate);
                        projected.original_pitch = 60;
                        projected.name = sample_name(sample, Some(rate));
                        Ok(projected)
                    } else {
                        pcm_sample_with_cancel(bytes, sample, cancel)
                    }
                },
            )?
        }
        1 | 2 => {
            let identity = tone.descriptor;
            sample_index_for(
                samples,
                sample_indices,
                (identity, None, SampleKind::Square, None),
                sample_points,
                8,
                || square_sample(tone),
            )?
        }
        3 => {
            let waveform = tone
                .waveform
                .context("PSG wave tone has no validated waveform")?;
            sample_index_for(
                samples,
                sample_indices,
                (waveform, None, SampleKind::Wave, None),
                sample_points,
                32,
                || wave_sample(bytes, waveform),
            )?
        }
        4 => {
            ensure!(
                key_start == key_end,
                "PSG noise zones must map exactly one key"
            );
            sample_index_for(
                samples,
                sample_indices,
                (tone.descriptor, None, SampleKind::Noise, None),
                sample_points,
                NOISE_15_BIT_PERIOD,
                || noise_sample(tone),
            )?
        }
        _ => unreachable!(),
    };
    let (attack_seconds, decay_seconds, sustain_level, release_seconds) =
        envelope(tone, kind != 0)?;
    let (scale_tuning, coarse_tune, fine_tune) = if kind == 4 {
        noise_tuning(i16::from(key_start) + coarse_tune)?
    } else if kind == 0 && tone.fixed_pitch {
        (0, 0, 0)
    } else {
        (100, coarse_tune, 0)
    };
    Ok(Region {
        key_start,
        key_end,
        sample_index,
        root_key: samples[sample_index].original_pitch,
        scale_tuning,
        coarse_tune,
        fine_tune,
        pan: pan(tone, rhythm),
        attack_seconds,
        decay_seconds,
        sustain_level,
        release_seconds,
    })
}

fn sample_index_for(
    samples: &mut Vec<Sample>,
    sample_indices: &mut BTreeMap<SampleIdentity, usize>,
    identity: SampleIdentity,
    sample_points: &mut usize,
    worst_case_added: usize,
    create: impl FnOnce() -> Result<Sample>,
) -> Result<usize> {
    if let Some(index) = sample_indices.get(&identity) {
        return Ok(*index);
    }
    ensure!(
        sample_points
            .checked_add(worst_case_added)
            .is_some_and(|count| count <= MAX_BANK_PCM_POINTS),
        "instrument projection exceeds its {MAX_BANK_PCM_POINTS}-point PCM limit"
    );
    let index = samples.len();
    let sample = create()?;
    *sample_points = sample_points
        .checked_add(sample.pcm.len())
        .context("SoundFont PCM point count overflows")?;
    ensure!(
        *sample_points <= MAX_BANK_PCM_POINTS,
        "instrument projection exceeds its {MAX_BANK_PCM_POINTS}-point PCM limit"
    );
    samples.push(sample);
    sample_indices.insert(identity, index);
    Ok(index)
}

fn square_sample(tone: &ToneInventory) -> Result<Sample> {
    ensure!(
        tone.data_word <= 3,
        "PSG square tone has an invalid duty setting"
    );
    let high = [1, 2, 4, 6][tone.data_word as usize];
    let pcm = (0..8)
        .map(|point| if point < high { 24_000 } else { -24_000 })
        .collect();
    Ok(Sample {
        name: format!("PSG_SQ_{:06X}", tone.descriptor.effective_offset),
        pcm,
        sample_rate: 3_520.0,
        original_pitch: 69,
        loop_range: Some((0, 8)),
    })
}

fn synth_sample(recipe: &super::camelot::SynthRecipe) -> Result<Sample> {
    use super::camelot::SynthKind;
    let rate = (u64::from(recipe.frequency) + 512) / 1024;
    ensure!(
        (400..=50_000).contains(&rate),
        "Camelot synthesis rate is outside SoundFont limits"
    );
    let exact_rate = f64::from(recipe.frequency) / 1024.0;
    let correction = (1200.0 * (exact_rate / rate as f64).log2()).round();
    ensure!(
        (-128.0..=127.0).contains(&correction),
        "Camelot synthesis tuning is outside SoundFont limits"
    );
    let (pcm, loop_start) = match recipe.kind {
        SynthKind::Pulse {
            base_duty,
            lfo_step,
            modulation,
            lfo_offset,
        } => {
            let lfo = (u32::from(lfo_step) << 24).wrapping_add(u32::from(lfo_offset) << 24);
            let folded = if lfo & 0x8000_0000 != 0 { !lfo } else { lfo };
            let threshold = (folded >> 8)
                .wrapping_mul(u32::from(modulation))
                .wrapping_add(u32::from(base_duty) << 24);
            (
                (0..64u32)
                    .map(|point| {
                        if point << 26 < threshold {
                            16_384
                        } else {
                            -16_384
                        }
                    })
                    .collect(),
                0,
            )
        }
        SynthKind::Triangle => {
            let pcm = (1..=64u32)
                .map(|point| {
                    let phase = point.wrapping_shl(26);
                    let amplitude = if phase & 0x8000_0000 == 0 {
                        (phase >> 23) as i32 - 128
                    } else {
                        384 - (phase >> 23) as i32
                    };
                    (amplitude * 255) as i16
                })
                .collect();
            (pcm, 0)
        }
        SynthKind::PseudoSaw => {
            let mut state = 0i32;
            let pcm = (1..=128u32)
                .map(|point| {
                    let phase = point.wrapping_shl(26);
                    let signal = (phase >> 24) as i32 - 112 - (phase.wrapping_shl(1) >> 27) as i32;
                    state = signal + (state >> 1);
                    ((state >> 1) * 256).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16
                })
                .collect();
            (pcm, 64)
        }
    };
    Ok(Sample {
        name: format!("CAM_{:06X}", recipe.parameters.effective_offset),
        pcm,
        sample_rate: exact_rate,
        original_pitch: 60,
        loop_range: Some((loop_start, loop_start + 64)),
    })
}

fn wave_sample(bytes: &[u8], waveform: RomSpan) -> Result<Sample> {
    let packed = span_bytes(bytes, waveform, "PSG waveform")?;
    ensure!(
        packed.len() == 16,
        "PSG waveform must contain exactly 16 bytes"
    );
    let pcm = packed
        .iter()
        .flat_map(|byte| [byte >> 4, byte & 0x0F])
        .map(|nibble| (i16::from(nibble) - 8) * 4096)
        .collect::<Vec<_>>();
    Ok(Sample {
        name: format!("PSG_WV_{:06X}", waveform.effective_offset),
        pcm,
        sample_rate: 14_080.0,
        original_pitch: 69,
        loop_range: Some((0, 32)),
    })
}

fn noise_sample(tone: &ToneInventory) -> Result<Sample> {
    ensure!(
        tone.data_word <= 1,
        "PSG noise tone has an invalid width setting"
    );
    let narrow = tone.data_word == 1;
    let period = if narrow {
        NOISE_7_BIT_PERIOD
    } else {
        NOISE_15_BIT_PERIOD
    };
    let mut lfsr = 0x7FFFu16;
    let mut pcm = Vec::with_capacity(period);
    for _ in 0..period {
        pcm.push(if lfsr & 1 == 0 { 16_000 } else { -16_000 });
        let xor = (lfsr & 1) ^ ((lfsr >> 1) & 1);
        lfsr = (lfsr >> 1) | (xor << 14);
        if narrow {
            lfsr = (lfsr & !(1 << 6)) | (xor << 6);
        }
    }
    ensure!(
        if narrow {
            lfsr & 0x7F == 0x7F
        } else {
            lfsr == 0x7FFF
        },
        "PSG noise LFSR did not return to its initial phase"
    );
    Ok(Sample {
        name: format!("PSG_NS_{:06X}", tone.descriptor.effective_offset),
        pcm,
        sample_rate: f64::from(NOISE_SAMPLE_RATE),
        original_pitch: 60,
        loop_range: Some((0, period as u32)),
    })
}

fn noise_tuning(key: i16) -> Result<(i16, i16, i16)> {
    let index = key.saturating_sub(21).clamp(0, 59) as usize;
    let nr43 = NOISE_NR43_BY_KEY[index];
    let divisor = NOISE_DIVISORS[(nr43 & 7) as usize];
    let clock_hz = 4_194_304.0 / f64::from(divisor << (nr43 >> 4));
    let cents = 1200.0 * (clock_hz / f64::from(NOISE_SAMPLE_RATE)).log2();
    ensure!(cents.is_finite(), "PSG noise pitch is not finite");
    let coarse = (cents / 100.0).round() as i16;
    let fine = (cents - f64::from(coarse) * 100.0).round() as i16;
    ensure!(
        (-120..=120).contains(&coarse) && (-99..=99).contains(&fine),
        "PSG noise pitch is outside SoundFont tuning limits"
    );
    Ok((0, coarse, fine))
}

fn envelope(tone: &ToneInventory, psg: bool) -> Result<(f32, f32, f32, f32)> {
    if psg {
        return Ok((0.005, 0.020, 1.0, 0.050));
    }
    let [attack, decay, sustain, release] = tone.adsr.context("tone has no MP2K envelope")?;
    let attack_seconds = if attack == 0 {
        0.001
    } else {
        255.0 / attack as f32 / MIXER_TICKS_PER_SECOND
    };
    let sustain_level = sustain as f32 / 255.0;
    let decay_seconds = exponential_time(decay, sustain_level);
    let release_seconds = exponential_time(release, 0.00001);
    Ok((
        attack_seconds,
        decay_seconds,
        sustain_level,
        release_seconds,
    ))
}

fn exponential_time(multiplier: u8, target: f32) -> f32 {
    if multiplier == 0 {
        return 0.001;
    }
    let target = target.clamp(0.00001, 0.99999);
    (target.ln() / (multiplier as f32 / 256.0).ln() / MIXER_TICKS_PER_SECOND).clamp(0.001, 100.0)
}

fn pan(tone: &ToneInventory, rhythm: bool) -> i16 {
    if !rhythm || tone.kind & 7 != 0 || tone.pan_sweep & 0x80 == 0 {
        return 0;
    }
    let pan = i16::from(tone.pan_sweep & 0x7F) - 64;
    (pan * 500 / 64).clamp(-500, 500)
}

fn contiguous_ranges(keys: &BTreeSet<u8>) -> Vec<(u8, u8)> {
    let mut result = Vec::new();
    let mut iter = keys.iter().copied();
    let Some(mut start) = iter.next() else {
        return result;
    };
    let mut end = start;
    for key in iter {
        if end.checked_add(1) == Some(key) {
            end = key;
        } else {
            result.push((start, end));
            start = key;
            end = key;
        }
    }
    result.push((start, end));
    result
}

fn span_bytes<'a>(bytes: &'a [u8], span: RomSpan, label: &str) -> Result<&'a [u8]> {
    let start = usize::try_from(span.effective_offset).context("ROM span offset overflows")?;
    let len = usize::try_from(span.byte_len).context("ROM span length overflows")?;
    let end = start.checked_add(len).context("ROM span end overflows")?;
    ensure!(
        span.byte_len > 0
            && span.canonical_cpu_address
                == 0x0800_0000_u32
                    .checked_add(span.effective_offset)
                    .context("ROM span CPU address overflows")?,
        "{label} has an invalid ROM mapping"
    );
    bytes
        .get(start..end)
        .with_context(|| format!("{label} is outside loaded ROM bytes"))
}

#[cfg(test)]
mod tests;
