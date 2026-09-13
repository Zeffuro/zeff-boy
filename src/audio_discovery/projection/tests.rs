use std::collections::BTreeMap;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::audio_discovery::ScanLimits;
use zeff_emu_common::system::System;

fn song_and_bytes() -> (SongCandidate, Vec<u8>) {
    let bytes = crate::audio_discovery::test_support::gba_fixture();
    let report = crate::audio_discovery::scan(
        System::Gba,
        &bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    (report.candidates.into_iter().next().unwrap(), bytes)
}

#[test]
fn psg_frequency_rounding_flags_preserve_melodic_pitch_in_rendered_audio() -> Result<()> {
    use crate::audio_discovery::{render, sf2, test_support};
    use std::sync::atomic::AtomicU32;

    for kind in [1, 2, 3, 9, 10, 11] {
        let mut bytes = test_support::gba_fixture();
        bytes[0x100] = 1;
        bytes[0x200..0x204].copy_from_slice(&[kind, 60, 0, 0]);
        test_support::put_word(
            &mut bytes,
            0x204,
            if kind & 7 == 3 { 0x0800_0380 } else { 2 },
        );
        bytes[0x208..0x20c].copy_from_slice(&[0, 0, 15, 0]);
        bytes[0x380..0x388].fill(0xff);
        bytes[0x388..0x390].fill(0);
        let sequence = [
            0xbd, 0, 0xbb, 75, 0xbe, 100, 0xe7, 53, 100, 0x98, 0xe7, 65, 100, 0x98, 0xb1,
        ];
        bytes[0x400..0x400 + sequence.len()].copy_from_slice(&sequence);
        let report = crate::audio_discovery::scan(
            System::Gba,
            &bytes,
            ScanLimits::default(),
            &AtomicBool::new(false),
        );
        let song = &report.candidates[0];
        assert!(!song.instruments[0].fixed_pitch, "kind {kind}");
        let bank = instrument_bank(&bytes, song, "PSG pitch fixture".into(), None)?;
        assert!(
            bank.presets
                .iter()
                .flat_map(|preset| &preset.zones)
                .all(|zone| zone.scale_tuning == 100)
        );
        let audio = render::render(
            song,
            &bytes,
            &sf2::encode(&bank)?,
            render::RenderOptions {
                max_seconds: 1,
                ..Default::default()
            },
            &AtomicBool::new(false),
            &AtomicU32::new(0),
        )?;
        let lower = rising_frequency(&audio.pcm, 3_000, 15_000, audio.sample_rate);
        let upper = rising_frequency(&audio.pcm, 22_000, 34_000, audio.sample_rate);
        assert!((lower - 174.614).abs() < 1.0, "kind {kind}: {lower} Hz");
        assert!((upper - 349.228).abs() < 1.0, "kind {kind}: {upper} Hz");
    }
    Ok(())
}

fn rising_frequency(pcm: &[i16], start: usize, end: usize, rate: u32) -> f64 {
    let frames = pcm.as_chunks::<2>().0;
    let crossings = (start + 1..end)
        .filter(|&index| frames[index - 1][0] <= 0 && frames[index][0] > 0)
        .collect::<Vec<_>>();
    assert!(crossings.len() > 10);
    f64::from(rate) * (crossings.len() - 1) as f64
        / (crossings.last().unwrap() - crossings[0]) as f64
}

#[test]
fn pcm_conversion_preserves_every_signed_source_byte() -> Result<()> {
    let (song, bytes) = song_and_bytes();
    let sample = song.instruments[0].sample.as_ref().unwrap();
    let pcm = pcm_sample(&bytes, sample)?;
    assert_eq!(pcm.pcm, vec![0, 32_512, 0, -32_768, -256]);
    assert_eq!(pcm.integer_rate(), 8_000);
    assert_eq!(pcm.pitch_correction(), 0);
    assert_eq!(pcm.loop_range, Some((2, 5)));
    Ok(())
}

#[test]
fn out_of_range_pcm_rates_use_equivalent_root_keys_without_resampling() -> Result<()> {
    let (mut song, bytes) = song_and_bytes();
    for (rate, expected_rate, expected_root) in
        [(53_516, 26_758, 48), (192_000, 48_000, 36), (375, 750, 72)]
    {
        let sample = song.instruments[0].tone.sample.as_mut().unwrap();
        sample.frequency = rate * 1024;
        let decoded = pcm_sample(&bytes, sample)?;
        let bank = instrument_bank(&bytes, &song, "Rate fixture".into(), None)?;
        let prepared = super::super::sf2::prepare(&bank)?;
        let projected = &prepared.samples[0];
        assert_eq!(decoded.integer_rate(), rate);
        assert_eq!(decoded.original_pitch, 60);
        assert_eq!(projected.integer_rate(), expected_rate);
        assert_eq!(projected.original_pitch, expected_root);
        assert_eq!(bank.samples[0].pcm, decoded.pcm);
        assert_eq!(bank.samples[0].loop_range, decoded.loop_range);
        let effective_rate =
            projected.sample_rate * 2f64.powf((60.0 - f64::from(projected.original_pitch)) / 12.0);
        assert!((effective_rate - f64::from(rate)).abs() < 0.001);
        let encoded = super::super::sf2::encode(&instrument_bank(
            &bytes,
            &song,
            "Rate fixture".into(),
            None,
        )?)?;
        let parsed = rustysynth::SoundFont::new(&mut std::io::Cursor::new(encoded))?;
        assert_eq!(
            parsed.get_sample_headers()[0].get_original_pitch(),
            i32::from(expected_root)
        );
    }
    Ok(())
}

#[test]
fn camelot_vectors_preserve_phase_filter_transient_and_root_pitch() -> Result<()> {
    for kind in 0..=2 {
        let bytes = super::super::test_support::camelot::synth_fixture(kind);
        let report = super::super::scan(
            System::Gba,
            &bytes,
            Default::default(),
            &AtomicBool::new(false),
        );
        let song = &report.candidates[0];
        let recipe = song.instruments[0].synthesis.as_ref().unwrap();
        let sample = synth_sample(recipe)?;
        assert_eq!(sample.integer_rate(), 16_738);
        assert_eq!(sample.original_pitch, 60);
        assert_eq!(sample.pitch_correction(), 0);
        match kind {
            0 => {
                assert_eq!(&sample.pcm[..29], &[16_384; 29]);
                assert_eq!(&sample.pcm[29..], &[-16_384; 35]);
                for (params, positive) in [
                    ([0x60, 0x20, 0x40, 0x80], 30),
                    ([0x80, 0x40, 0x40, 0x40], 40),
                ] {
                    let mut alternative = recipe.clone();
                    alternative.kind = super::super::camelot::SynthKind::Pulse {
                        base_duty: params[0],
                        lfo_step: params[1],
                        modulation: params[2],
                        lfo_offset: params[3],
                    };
                    assert_eq!(
                        synth_sample(&alternative)?
                            .pcm
                            .iter()
                            .filter(|&&point| point > 0)
                            .count(),
                        positive
                    );
                }
            }
            1 => {
                assert_eq!(
                    &sample.pcm[..8],
                    &[-55, -81, -92, -96, -97, -96, -94, -91].map(|point| point * 256)
                );
                assert_eq!(
                    &sample.pcm[64..72],
                    &[-57, -82, -93, -97, -97, -96, -94, -91].map(|point| point * 256)
                );
                assert_eq!(sample.loop_range, Some((64, 128)));
            }
            2 => {
                assert_eq!(
                    &sample.pcm[..8],
                    &[-120, -112, -104, -96, -88, -80, -72, -64].map(|point| point * 255)
                );
                assert_eq!(sample.pcm[31], 128 * 255);
                assert_eq!(sample.pcm[63], -128 * 255);
            }
            _ => unreachable!(),
        }
        let bank = instrument_bank(&bytes, song, "Camelot fixture".into(), None)?;
        let encoded = super::super::sf2::encode(&bank)?;
        rustysynth::SoundFont::new(&mut std::io::Cursor::new(encoded))?;
        assert!(song.instruments[0].sample.is_none());
    }
    Ok(())
}

#[test]
fn portable_bank_keeps_source_samples_and_sf2_normalizes_its_own_copy() -> Result<()> {
    let (song, bytes) = song_and_bytes();
    let bank = instrument_bank(&bytes, &song, "{\"fixture\":true}".into(), None)?;
    assert_eq!(bank.presets.len(), 2);
    assert_eq!(bank.presets[0].bank, 0);
    assert_eq!(bank.presets[1].bank, 128);
    assert_eq!(bank.samples[0].pcm.len(), 5);
    assert_eq!(bank.samples[0].loop_range, Some((2, 5)));
    let prepared = super::super::sf2::prepare(&bank)?;
    assert!(prepared.samples[0].pcm.len() >= 48);
    assert!(prepared.samples[0].loop_range.unwrap().0 >= 8);
    assert_eq!(bank.samples[0].pcm[0], 0);
    Ok(())
}

#[test]
fn voices_become_distinct_programs_without_layering() -> Result<()> {
    let (mut song, bytes) = song_and_bytes();
    let mut second = song.instruments[0].clone();
    second.voice = 1;
    song.instruments.push(second);
    for track in &mut song.tracks {
        track.voices.push(1);
        track.voice_keys.push(super::super::VoiceKeys {
            voice: 1,
            keys: vec![60],
        });
    }
    let bank = instrument_bank(&bytes, &song, "{\"fixture\":true}".into(), None)?;
    assert_eq!(bank.presets.len(), 4);
    assert_eq!(
        bank.presets
            .iter()
            .map(|preset| (preset.bank, preset.program))
            .collect::<Vec<_>>(),
        vec![(0, 0), (128, 0), (0, 1), (128, 1)]
    );
    assert!(bank.presets.iter().all(|preset| preset.zones.len() == 1));
    Ok(())
}

#[test]
fn psg_waves_keep_their_complete_period_and_default_to_center_pan() -> Result<()> {
    let (song, _) = song_and_bytes();
    let mut tone = song.instruments[0].tone.clone();
    tone.kind = 1;
    tone.data_word = 2;
    tone.pan_sweep = 0;
    let square = square_sample(&tone)?;
    assert_eq!(square.pcm.len(), 8);
    assert_eq!(square.loop_range, Some((0, 8)));
    assert_eq!(square.integer_rate(), 3_520);
    assert_eq!(square.original_pitch, 69);
    assert_eq!(pan(&tone, true), 0);
    let waveform = RomSpan {
        effective_offset: 0,
        byte_len: 16,
        canonical_cpu_address: 0x0800_0000,
    };
    let wave = wave_sample(&[0x08; 16], waveform)?;
    assert_eq!(wave.pcm.len(), 32);
    assert_eq!(wave.loop_range, Some((0, 32)));
    assert_eq!(wave.integer_rate(), 14_080);
    assert_eq!(wave.original_pitch, 69);
    Ok(())
}

#[test]
fn noise_uses_the_mp2k_key_table_and_complete_lfsr_periods() -> Result<()> {
    let (song, _) = song_and_bytes();
    let mut tone = song.instruments[0].tone.clone();
    tone.kind = 4;
    tone.data_word = 0;
    let wide = noise_sample(&tone)?;
    assert_eq!(wide.pcm.len(), NOISE_15_BIT_PERIOD);
    assert_eq!(wide.loop_range, Some((0, NOISE_15_BIT_PERIOD as u32)));
    assert!(
        wide.pcm
            .iter()
            .all(|point| matches!(*point, -16_000 | 16_000))
    );
    tone.data_word = 1;
    let narrow = noise_sample(&tone)?;
    assert_eq!(narrow.pcm.len(), NOISE_7_BIT_PERIOD);
    assert_eq!(narrow.loop_range, Some((0, NOISE_7_BIT_PERIOD as u32)));
    tone.data_word = 2;
    assert!(noise_sample(&tone).is_err());

    assert_eq!(NOISE_NR43_BY_KEY[0], 0xD7);
    assert_eq!(NOISE_NR43_BY_KEY[39], 0x44); // MIDI key 60
    assert_eq!(noise_tuning(60)?, (0, 0, 0));
    let (_, low_coarse, low_fine) = noise_tuning(0)?;
    assert_eq!(low_coarse, -118);
    assert!((-99..=99).contains(&low_fine));
    Ok(())
}

#[test]
fn fixed_pcm_uses_a_distinct_mixer_rate_sample_and_ignores_key_tuning() -> Result<()> {
    let (song, bytes) = song_and_bytes();
    let normal = song.instruments[0].tone.clone();
    let mut fixed = normal.clone();
    fixed.kind = 8;
    fixed.fixed_pitch = true;
    let mut samples = Vec::new();
    let mut indices = BTreeMap::new();
    let mut points = 0;
    let normal_zone = project_tone(
        &bytes,
        &normal,
        60,
        60,
        0,
        false,
        &mut samples,
        &mut indices,
        &mut points,
        None,
        &AtomicBool::new(false),
    )?;
    assert!(
        project_tone(
            &bytes,
            &fixed,
            63,
            63,
            -3,
            true,
            &mut samples,
            &mut indices,
            &mut points,
            None,
            &AtomicBool::new(false),
        )
        .is_err()
    );
    let fixed_zone = project_tone(
        &bytes,
        &fixed,
        63,
        63,
        -3,
        true,
        &mut samples,
        &mut indices,
        &mut points,
        Some(21_024),
        &AtomicBool::new(false),
    )?;
    assert_ne!(normal_zone.sample_index, fixed_zone.sample_index);
    assert_ne!(
        samples[normal_zone.sample_index].sample_rate,
        samples[fixed_zone.sample_index].sample_rate
    );
    assert_eq!(samples[fixed_zone.sample_index].integer_rate(), 21_024);
    assert_eq!(samples[fixed_zone.sample_index].pitch_correction(), 0);
    assert_eq!(
        (
            fixed_zone.scale_tuning,
            fixed_zone.coarse_tune,
            fixed_zone.fine_tune
        ),
        (0, 0, 0)
    );
    assert_ne!(
        samples[normal_zone.sample_index].name,
        samples[fixed_zone.sample_index].name
    );
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn noise_bank_encodes_and_is_accepted_by_the_renderer_consumer() -> Result<()> {
    let (mut song, bytes) = song_and_bytes();
    song.instruments[0].tone.kind = 4;
    song.instruments[0].tone.data_word = 0;
    let bank = instrument_bank(&bytes, &song, "{\"fixture\":true}".into(), None)?;
    assert!(bank.samples[0].pcm.iter().any(|point| *point != 0));
    let mut cursor = std::io::Cursor::new(super::super::sf2::encode(&bank)?);
    let parsed = rustysynth::SoundFont::new(&mut cursor)
        .map_err(|error| anyhow::anyhow!("RustySynth rejected generated noise bank: {error}"))?;
    assert_eq!(parsed.get_sample_headers().len(), 1);
    Ok(())
}

#[test]
fn split_projection_rejects_a_referenced_key_without_a_region() {
    let (song, bytes) = song_and_bytes();
    let mut instrument = song.instruments[0].clone();
    let child = instrument.tone.clone();
    instrument.tone.kind = 0x40;
    instrument.regions = vec![super::super::InstrumentRegion {
        key_start: 60,
        key_end: 60,
        descriptor_index: 0,
        tone: Some(child),
        warning: None,
    }];
    let keys = [60, 61].into_iter().collect();
    assert!(
        project_instrument(
            &bytes,
            &instrument,
            &keys,
            &mut Vec::new(),
            &mut BTreeMap::new(),
            &mut 0,
            &mut Vec::new(),
            None,
            &AtomicBool::new(false),
        )
        .is_err()
    );
}

#[test]
fn bank_pcm_limit_is_checked_before_creating_another_sample() {
    let identity = RomSpan {
        effective_offset: 0,
        byte_len: 16,
        canonical_cpu_address: 0x0800_0000,
    };
    let mut points = MAX_BANK_PCM_POINTS - 8;
    assert!(
        sample_index_for(
            &mut Vec::new(),
            &mut BTreeMap::new(),
            (identity, None, SampleKind::Pcm, None),
            &mut points,
            identity.byte_len as usize,
            || unreachable!()
        )
        .is_err()
    );
}

#[test]
fn contiguous_key_ranges_do_not_fill_unreferenced_holes() {
    assert_eq!(
        contiguous_ranges(&[2, 3, 7, 9, 10].into_iter().collect()),
        vec![(2, 3), (7, 7), (9, 10)]
    );
}

#[test]
fn one_rom_span_used_as_pcm_and_psg_wave_keeps_distinct_decodings() -> Result<()> {
    use crate::audio_discovery::test_support::put_word;
    let (_, mut bytes) = song_and_bytes();
    put_word(&mut bytes, 0x30C, 16);
    bytes[0x20C] = 3;
    put_word(&mut bytes, 0x210, 0x0800_0310);
    let sequence = [
        0xBD, 0, 0xD0, 60, 100, 0x81, 0xBD, 1, 0xD0, 60, 100, 0x81, 0xB1,
    ];
    bytes[0x400..0x400 + sequence.len()].copy_from_slice(&sequence);
    let report = crate::audio_discovery::scan(
        System::Gba,
        &bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    );
    let bank = instrument_bank(
        &bytes,
        &report.candidates[0],
        "alias fixture".to_owned(),
        None,
    )?;
    assert_eq!(bank.samples.len(), 2);
    assert_eq!(bank.samples[0].integer_rate(), 8_000);
    assert_eq!(bank.samples[1].integer_rate(), 14_080);
    assert_ne!(bank.samples[0].pcm, bank.samples[1].pcm);
    Ok(())
}

#[test]
fn one_pcm_span_used_forward_and_reverse_keeps_distinct_decodings() -> Result<()> {
    let (song, bytes) = song_and_bytes();
    let forward = song.instruments[0].tone.clone();
    let mut reverse = forward.clone();
    reverse.kind = 0x10;
    reverse.sample.as_mut().unwrap().direction = SampleDirection::Reverse;

    let mut samples = Vec::new();
    let mut indices = BTreeMap::new();
    let mut points = 0;
    let cancel = AtomicBool::new(false);
    let forward_zone = project_tone(
        &bytes,
        &forward,
        60,
        60,
        0,
        false,
        &mut samples,
        &mut indices,
        &mut points,
        None,
        &cancel,
    )?;
    let reverse_zone = project_tone(
        &bytes,
        &reverse,
        60,
        60,
        0,
        false,
        &mut samples,
        &mut indices,
        &mut points,
        None,
        &cancel,
    )?;

    assert_ne!(forward_zone.sample_index, reverse_zone.sample_index);
    assert_ne!(samples[0].name, samples[1].name);
    assert_eq!(
        samples[forward_zone.sample_index].pcm,
        [0, 32_512, 0, -32_768, -256]
    );
    assert_eq!(samples[forward_zone.sample_index].loop_range, Some((2, 5)));
    assert_eq!(
        samples[reverse_zone.sample_index].pcm,
        [-256, -32_768, 0, 32_512, 0]
    );
    assert_eq!(samples[reverse_zone.sample_index].loop_range, None);
    assert_eq!(points, 10);
    Ok(())
}

#[test]
fn compressed_projection_budgets_decoded_points_and_ignores_padding_nibble() -> Result<()> {
    let mut bytes = vec![0; 16 + 33];
    bytes[0..2].copy_from_slice(&1u16.to_le_bytes());
    bytes[2..4].copy_from_slice(&0x4000u16.to_le_bytes());
    bytes[4..8].copy_from_slice(&(8_000 * 1024u32).to_le_bytes());
    bytes[8..12].copy_from_slice(&2u32.to_le_bytes());
    bytes[12..16].copy_from_slice(&4u32.to_le_bytes());
    bytes[16..19].copy_from_slice(&[120, 0xa7, 0x81]);
    let sample = super::super::test_support::sample::read_sample_inventory(
        &bytes,
        0,
        0x20,
        super::super::EngineProfile::Mp2k,
    )
    .unwrap();

    assert_eq!(sample.data.byte_len, 33);
    assert_eq!(sample.decoded_len, 4);
    let projected = pcm_sample(&bytes, &sample)?;
    assert_eq!(projected.pcm, [120 * 256, -87 * 256, 105 * 256, 106 * 256]);
    assert_eq!(projected.loop_range, Some((2, 4)));
    Ok(())
}
