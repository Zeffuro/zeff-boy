use crate::sample::{SampleDirection, SampleEncoding};
use crate::test_support::{fixture, put_word};
use crate::{Confidence, ScanLimits, ScanStatus, Warning, scan};
use std::sync::atomic::AtomicBool;
use zeff_emu_common::system::System;

fn report(bytes: &[u8]) -> crate::ScanReport {
    scan(
        System::Gba,
        bytes,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
}

fn notes(bytes: &mut [u8], keys: &[u8]) {
    let mut track = vec![0xBD, 0];
    for &key in keys {
        track.extend_from_slice(&[0xD0, key, 100]);
    }
    track.push(0xB1);
    bytes[0x400..0x400 + track.len()].copy_from_slice(&track);
    put_word(bytes, 0x10C, 0x0800_0400);
}

#[test]
fn pcm_fixed_rate_and_psg_frequency_flags_preserve_tone_fields_and_sources() {
    for (kind, data_word) in [
        (8, 0x0800_0300),
        (9, 2),
        (10, 3),
        (11, 0x0800_0380),
        (12, 1),
    ] {
        let mut bytes = fixture();
        bytes[0x200..0x204].copy_from_slice(&[kind, 61, 17, 0xC2]);
        put_word(&mut bytes, 0x204, data_word);
        let found = report(&bytes);
        assert_eq!(found.status, ScanStatus::Complete);
        let song = &found.candidates[0];
        assert_eq!(song.confidence, Confidence::Structural, "kind {kind}");
        let tone = &song.instruments[0];
        assert_eq!(tone.kind, kind);
        assert_eq!(tone.fixed_pitch, kind == 8);
        assert_eq!((tone.key, tone.length, tone.pan_sweep), (61, 17, 0xC2));
        assert_eq!(tone.adsr, Some([255, 180, 128, 100]));
        assert_eq!(tone.sample.is_some(), kind == 8);
        assert_eq!(tone.waveform.is_some(), kind == 11);
    }
}

#[test]
fn rhythm_regions_follow_actual_note_keys_without_validating_unused_slots() {
    let mut bytes = fixture();
    let original = bytes[0x200..0x20C].to_vec();
    bytes[0x200] = 0x80;
    put_word(&mut bytes, 0x204, 0x0800_0800);
    notes(&mut bytes, &[60, 61, 62, 63]);
    for key in 60..=63 {
        let at = 0x800 + key * 12;
        bytes[at..at + 12].copy_from_slice(&original);
    }
    bytes[0x800 + 61 * 12] = 8;
    bytes[0x800 + 62 * 12] = 12;
    put_word(&mut bytes, 0x800 + 62 * 12 + 4, 1);
    bytes[0x800 + 63 * 12] = 11;
    put_word(&mut bytes, 0x800 + 63 * 12 + 4, 0x0800_0380);
    bytes[0x800 + 59 * 12] = 0xFF;
    let found = report(&bytes);
    let song = &found.candidates[0];
    assert_eq!(song.confidence, Confidence::Structural);
    assert_eq!(song.tracks[0].voice_keys[0].keys, [60, 61, 62, 63]);
    let instrument = &song.instruments[0];
    assert_eq!(instrument.regions.len(), 4);
    assert_eq!(instrument.adsr, None);
    for (region, key) in instrument.regions.iter().zip(60..=63) {
        assert_eq!((region.key_start, region.key_end), (key, key));
        assert_eq!(
            region.tone.as_ref().unwrap().descriptor.effective_offset,
            0x800 + u32::from(key) * 12
        );
        assert!(region.warning.is_none());
    }
    assert!(instrument.regions[1].tone.as_ref().unwrap().fixed_pitch);
    assert!(!instrument.regions[2].tone.as_ref().unwrap().fixed_pitch);
    assert!(!instrument.regions[3].tone.as_ref().unwrap().fixed_pitch);
    assert!(
        instrument.regions[3]
            .tone
            .as_ref()
            .unwrap()
            .waveform
            .is_some()
    );
}

#[test]
fn split_keys_preserve_key_map_and_merge_only_observed_adjacent_equal_regions() {
    let mut bytes = fixture();
    let original = bytes[0x200..0x20C].to_vec();
    bytes[0x200] = 0x40;
    put_word(&mut bytes, 0x204, 0x0800_0800);
    put_word(&mut bytes, 0x208, 0x0800_0700);
    notes(&mut bytes, &[60, 61, 62, 64]);
    bytes[0x700 + 60..0x700 + 65].copy_from_slice(&[1, 1, 2, 3, 3]);
    for index in 1..=3 {
        bytes[0x800 + index * 12..0x800 + index * 12 + 12].copy_from_slice(&original);
    }
    let found = report(&bytes);
    let instrument = &found.candidates[0].instruments[0];
    assert_eq!(found.candidates[0].confidence, Confidence::Structural);
    assert_eq!(instrument.key_map.unwrap().effective_offset, 0x700);
    assert_eq!(instrument.key_map.unwrap().byte_len, 128);
    assert_eq!(instrument.adsr, None);
    assert_eq!(
        instrument
            .regions
            .iter()
            .map(|r| (r.key_start, r.key_end, r.descriptor_index))
            .collect::<Vec<_>>(),
        [(60, 61, 1), (62, 62, 2), (64, 64, 3)]
    );
}

#[test]
fn invalid_and_recursive_regions_remain_explicit_without_following_cycles() {
    let mut bytes = fixture();
    let original = bytes[0x200..0x20C].to_vec();
    bytes[0x200] = 0x80;
    put_word(&mut bytes, 0x204, 0x0800_0800);
    let at = 0x800 + 60 * 12;
    bytes[at..at + 12].copy_from_slice(&original);
    put_word(&mut bytes, at + 4, 0x0C00_0300);
    let found = report(&bytes);
    assert_eq!(found.candidates[0].confidence, Confidence::Unresolved);
    assert!(
        found.candidates[0]
            .warnings
            .contains(&Warning::InvalidInstrumentRegion {
                voice: 0,
                key: 60,
                offset: at as u32
            })
    );
    bytes[at] = 0x80;
    put_word(&mut bytes, at + 4, 0x0800_0800);
    let recursive = report(&bytes);
    assert!(
        recursive.candidates[0]
            .warnings
            .contains(&Warning::UnsupportedInstrumentRegion {
                voice: 0,
                key: 60,
                offset: at as u32,
                kind: 0x80
            })
    );
    assert_eq!(recursive.candidates[0].instruments[0].regions.len(), 1);
    put_word(&mut bytes, 0x204, 0x0800_0FFC);
    assert_eq!(
        report(&bytes).candidates[0].confidence,
        Confidence::Unresolved
    );
}

#[test]
fn zero_length_sample_retains_raw_header_without_claiming_pcm_data() {
    let mut bytes = fixture();
    put_word(&mut bytes, 0x30C, 0);
    let found = report(&bytes);
    let tone = &found.candidates[0].instruments[0];
    assert_eq!(found.candidates[0].confidence, Confidence::Unresolved);
    assert_eq!(tone.sample_header.unwrap().effective_offset, 0x300);
    assert_eq!(tone.sample_header.unwrap().byte_len, 16);
    assert!(tone.sample.is_none());
    assert!(
        found.candidates[0]
            .warnings
            .contains(&Warning::EmptySample {
                voice: 0,
                offset: 0x300
            })
    );
}

#[test]
fn mp2k_reverse_pcm_is_structural_and_records_decoded_direction() {
    let mut bytes = fixture();
    bytes[0x200] = 0x10;
    let found = report(&bytes);
    let sample = found.candidates[0].instruments[0].sample.as_ref().unwrap();
    assert_eq!(found.candidates[0].confidence, Confidence::Structural);
    assert_eq!(sample.data.effective_offset, 0x310);
    assert_eq!(sample.data.byte_len, 5);
    assert_eq!(sample.decoded_len, 5);
    assert_eq!(sample.encoding, SampleEncoding::PcmS8);
    assert_eq!(sample.direction, SampleDirection::Reverse);
}

#[test]
fn mp2k_bdpcm_uses_encoded_span_instead_of_decoded_count() {
    let mut bytes = fixture();
    bytes[0x200] = 0x20;
    bytes[0x300..0x302].copy_from_slice(&1u16.to_le_bytes());
    bytes[0x302..0x304].copy_from_slice(&0u16.to_le_bytes());
    put_word(&mut bytes, 0x30C, 65);
    let found = report(&bytes);
    let sample = found.candidates[0].instruments[0].sample.as_ref().unwrap();
    assert_eq!(found.candidates[0].confidence, Confidence::Structural);
    assert_eq!(sample.data.byte_len, 66);
    assert_eq!(sample.decoded_len, 65);
    assert_eq!(sample.encoding, SampleEncoding::GameFreakBdpcm);
    assert_eq!(sample.direction, SampleDirection::Forward);
}

#[test]
fn encoding_marker_and_kind_must_form_a_supported_engine_pair() {
    let mut bytes = fixture();
    bytes[0x200] = 0x20;
    let found = report(&bytes);
    assert_eq!(found.candidates[0].confidence, Confidence::Unresolved);
    assert!(found.candidates[0].instruments[0].sample.is_none());
    assert!(
        found.candidates[0]
            .warnings
            .contains(&Warning::UnsupportedInstrument {
                voice: 0,
                kind: 0x20
            })
    );
}
