use super::*;
use zeff_emu_common::system::System;

use super::test_support::{collection, fixture, put_word};

fn discover(rom: &[u8]) -> ScanReport {
    scan(
        System::Gba,
        rom,
        ScanLimits::default(),
        &AtomicBool::new(false),
    )
}

fn song(report: &ScanReport) -> &SongCandidate {
    report
        .candidates
        .iter()
        .find(|candidate| candidate.header.effective_offset == 0x100)
        .expect("fixture collection must be reported")
}

#[test]
fn exact_structure_provenance_and_short_pcm_sample() {
    let rom = fixture();
    let report = discover(&rom);
    assert_eq!(report.status, ScanStatus::Complete);
    assert_eq!(report.candidates.len(), 1);
    let found = song(&report);
    assert_eq!(found.confidence, Confidence::Structural);
    assert_eq!(
        found.header,
        RomSpan {
            effective_offset: 0x100,
            byte_len: 16,
            canonical_cpu_address: 0x0800_0100,
        }
    );
    assert_eq!(found.voicegroup_address, 0x0800_0200);
    assert_eq!(found.voicegroup_offset, 0x200);
    assert_eq!(found.evidence.decoded_tracks, 2);
    assert_eq!(found.evidence.validated_instruments, 1);
    assert!(found.evidence.explicit_note_data);
    assert!(!found.evidence.engine_signature_verified);
    assert!(!found.evidence.song_table_verified);
    assert!(found.warnings.is_empty());
    assert_eq!(found.tracks.len(), 2);
    for (track, start) in found.tracks.iter().zip([0x400, 0x440]) {
        assert_eq!(
            track.spans,
            [RomSpan {
                effective_offset: start,
                byte_len: 11,
                canonical_cpu_address: 0x0800_0000 + start,
            }]
        );
        assert_eq!(track.event_count, 6);
        assert_eq!(track.note_count, 1);
        assert_eq!(track.voices, [0]);
        assert_eq!(track.termination, TrackTermination::Fine);
    }
    let tone = &found.instruments[0];
    assert_eq!(tone.descriptor.effective_offset, 0x200);
    assert_eq!(tone.descriptor.byte_len, 12);
    let sample = tone.sample.as_ref().unwrap();
    assert_eq!(sample.header.effective_offset, 0x300);
    assert_eq!(sample.header.byte_len, 16);
    assert_eq!(sample.data.effective_offset, 0x310);
    assert_eq!(sample.data.byte_len, 5);
    assert_eq!(sample.loop_start, 2);
    assert!(sample.looped);
    assert_eq!(sample.frequency, 8_192_000);
    assert_eq!(
        report.media.sha256.as_deref(),
        Some(const_hex::encode(zeff_firmware::sha256_bytes(&rom)).as_str())
    );
}

#[test]
fn driver_width_voice_indices_and_status_bit_operands_are_preserved() {
    let mut rom = fixture();
    let original_tone = rom[0x200..0x20C].to_vec();
    rom[0x800..0x80C].copy_from_slice(&original_tone);
    let track = [0xBD, 0x80, 0xBA, 0xFF, 0xD0, 60, 100, 0x81, 0xB1];
    rom[0x400..0x400 + track.len()].copy_from_slice(&track);
    put_word(&mut rom, 0x30C, 16);
    let report = discover(&rom);
    let found = song(&report);
    assert_eq!(found.confidence, Confidence::Structural);
    assert_eq!(found.tracks[0].voices, [128]);
    assert_eq!(found.tracks[0].note_count, 1);
    assert_eq!(found.tracks[0].event_count, 5);
    assert_eq!(
        found
            .instruments
            .iter()
            .map(|tone| (tone.voice, tone.descriptor.effective_offset))
            .collect::<Vec<_>>(),
        [(0, 0x200), (128, 0x800)]
    );
    assert!(
        found
            .instruments
            .iter()
            .all(|tone| tone.sample.as_ref().unwrap().data.byte_len == 16)
    );
}

#[test]
fn two_collections_have_stable_order_and_byte_identical_manifests() {
    let mut rom = fixture();
    collection(&mut rom, 0x800);
    let first = discover(&rom);
    assert_eq!(
        first
            .candidates
            .iter()
            .map(|song| song.header.effective_offset)
            .collect::<Vec<_>>(),
        [0x100, 0x800]
    );
    let encoded = serde_json::to_vec(&first).unwrap();
    for _ in 0..4 {
        assert_eq!(serde_json::to_vec(&discover(&rom)).unwrap(), encoded);
    }
    let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(value["schema"], "zeff-audio-discovery/1");
    assert_eq!(value["status"]["kind"], "complete");
    assert!(!value["limitations"].as_array().unwrap().is_empty());
}

#[test]
fn canonical_rom_mapping_checks_full_span_and_never_masks_foreign_pointers() {
    for address in [
        0,
        0x0200_0200,
        0x0700_0200,
        0x0A00_0200,
        0x0C00_0200,
        0x8800_0200,
        0xFFFF_FFFC,
        0x0800_0FFC,
        0x0800_0201,
    ] {
        let mut rom = fixture();
        put_word(&mut rom, 0x104, address);
        assert!(discover(&rom).candidates.is_empty(), "{address:08x}");
    }
    let mut rom = vec![0; 0x0100_1000];
    collection(&mut rom, 0x0100_0100);
    let report = discover(&rom);
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(
        report.candidates[0].header.canonical_cpu_address,
        0x0900_0100
    );
}

#[test]
fn running_status_notes_and_tie_do_not_end_a_track() {
    let mut rom = fixture();
    let track = [
        0xBD, 0, 0xBE, 100, 90, 0xD0, 60, 100, 0x81, 62, 90, 0x81, 0xCF, 64, 100, 0x81, 0xCE, 64,
        0xB1,
    ];
    rom[0x400..0x400 + track.len()].copy_from_slice(&track);
    let report = discover(&rom);
    let found = song(&report);
    assert_eq!(found.confidence, Confidence::Structural);
    assert_eq!(found.tracks[0].termination, TrackTermination::Fine);
    assert_eq!(found.tracks[0].note_count, 3);
    assert_eq!(found.tracks[0].event_count, 11);
    assert_eq!(found.tracks[0].spans[0].byte_len, track.len() as u32);
}

#[test]
fn pattern_calls_preserve_disjoint_consumed_spans_and_return_state() {
    let mut rom = fixture();
    rom[0x400..0x403].copy_from_slice(&[0xBD, 0, 0xB3]);
    put_word(&mut rom, 0x403, 0x0800_0500);
    rom[0x407] = 0xB1;
    rom[0x500..0x505].copy_from_slice(&[0xD0, 60, 100, 0x81, 0xB4]);
    let report = discover(&rom);
    let track = &song(&report).tracks[0];
    assert_eq!(track.termination, TrackTermination::Fine);
    assert_eq!(
        track
            .spans
            .iter()
            .map(|span| (span.effective_offset, span.byte_len))
            .collect::<Vec<_>>(),
        [(0x400, 8), (0x500, 5)]
    );
    assert_eq!(track.event_count, 6);
}

#[test]
fn finite_repeat_and_permanent_goto_have_different_terminations() {
    let mut rom = fixture();
    rom[0x409] = 0xB5;
    rom[0x40A] = 3;
    put_word(&mut rom, 0x40B, 0x0800_0406);
    rom[0x40F] = 0xB1;
    let finite = discover(&rom);
    assert_eq!(song(&finite).tracks[0].termination, TrackTermination::Fine);
    assert_eq!(song(&finite).tracks[0].note_count, 3);

    rom[0x409] = 0xB2;
    put_word(&mut rom, 0x40A, 0x0800_0406);
    let looping = discover(&rom);
    assert_eq!(song(&looping).tracks[0].termination, TrackTermination::Loop);
    assert_eq!(song(&looping).confidence, Confidence::Structural);
    assert_eq!(song(&looping).tracks[0].spans[0].byte_len, 14);
}

#[test]
fn malformed_control_flow_and_unknown_dialect_never_become_structural_success() {
    let mut rom = fixture();
    rom[0x409] = 0xB2;
    put_word(&mut rom, 0x40A, 0x0800_0407);
    let report = discover(&rom);
    assert_eq!(song(&report).confidence, Confidence::Unresolved);
    assert!(
        song(&report)
            .warnings
            .contains(&Warning::InvalidTrack { offset: 0x407 })
    );

    for opcode in [
        0xB6, 0xB7, 0xB8, 0xB9, 0xC6, 0xC7, 0xC9, 0xCA, 0xCB, 0xCC, 0xCD,
    ] {
        let mut rom = fixture();
        rom[0x409] = opcode;
        let report = discover(&rom);
        assert_eq!(song(&report).confidence, Confidence::Unresolved);
        assert!(
            song(&report)
                .warnings
                .contains(&Warning::UnsupportedCommand {
                    offset: 0x409,
                    opcode
                })
        );
    }
}

#[test]
fn runtime_song_return_requires_the_verified_song_id_header_profile() {
    let mut rom = fixture();
    collection(&mut rom, 0x800);
    for offset in [0x40A, 0xB0A] {
        rom[offset] = 0xB6;
        rom[offset + 1..offset + 5].copy_from_slice(&[0xB7, 0xFF, 0xFF, 0xFF]);
    }
    let cancel = AtomicBool::new(false);
    let mut budget = Budget {
        cancel: &cancel,
        remaining: 100_000,
    };
    let mut candidates = Vec::new();
    mp2k::scan(
        &rom,
        &mut candidates,
        ScanLimits::default(),
        &mut budget,
        &std::collections::BTreeSet::from([0x100]),
    )
    .unwrap();
    assert_eq!(candidates.len(), 2);
    let verified = &candidates[0];
    assert_eq!(verified.engine, EngineProfile::Mp2kSongId);
    assert_eq!(verified.confidence, Confidence::Structural);
    assert!(verified.warnings.is_empty());
    assert_eq!(
        verified.tracks[0].termination,
        TrackTermination::RuntimeSongReturn
    );
    assert_eq!(verified.tracks[0].spans, [RomSpan::new(0x400, 11)]);
    assert_eq!(verified.tracks[0].event_count, 6);
    assert_eq!(verified.tracks[1].termination, TrackTermination::Fine);
    let unverified = &candidates[1];
    assert_eq!(unverified.engine, EngineProfile::Mp2k);
    assert_eq!(
        unverified.tracks[0].termination,
        TrackTermination::Unresolved
    );
    assert!(unverified.warnings.contains(&Warning::UnsupportedCommand {
        offset: 0xB0A,
        opcode: 0xB6,
    }));

    #[cfg(not(target_arch = "wasm32"))]
    {
        let program = mp2k::program_for_song(&rom, verified, 0x0800_0400, &cancel).unwrap();
        assert_eq!(program.ticks, 1);
        assert!(matches!(
            program.events.last().unwrap(),
            mp2k::TimedEvent {
                tick: 1,
                event: mp2k::Event::RuntimeSongReturn,
            }
        ));
        assert!(mp2k::program_for_song(&rom, unverified, 0x0800_0B00, &cancel).is_err());
        assert!(mp2k::program(&rom, 0x0800_0400, &cancel).is_err());
    }
}

#[test]
fn recursive_patterns_and_long_tracks_remain_bounded() {
    let mut rom = fixture();
    rom[0x400..0x403].copy_from_slice(&[0xBD, 0, 0xB3]);
    put_word(&mut rom, 0x403, 0x0800_0402);
    let report = discover(&rom);
    assert_eq!(
        song(&report).tracks[0].termination,
        TrackTermination::Unresolved
    );
    assert!(song(&report).tracks[0].event_count < 10);

    let mut rom = fixture();
    rom.resize(0x4000, 0x80);
    put_word(&mut rom, 0x108, 0x0800_1000);
    rom[0x1000..0x1005].copy_from_slice(&[0xBD, 0, 0xD0, 60, 100]);
    let report = discover(&rom);
    assert_eq!(
        report.status,
        ScanStatus::Incomplete(ScanStop::ValidationLimit)
    );
    assert_eq!(song(&report).tracks[0].event_count, 8192);
}

#[test]
fn bad_sample_lengths_loops_and_bank_pointers_do_not_validate() {
    for (at, value) in [
        (0x30C, u32::MAX),
        (0x304, 0),
        (0x308, 5),
        (0x204, 0x0800_0FFC),
    ] {
        let mut rom = fixture();
        put_word(&mut rom, at, value);
        let report = discover(&rom);
        let found = song(&report);
        assert_eq!(
            found.confidence,
            Confidence::Unresolved,
            "field {at:x} value {value:x}"
        );
        assert_eq!(found.evidence.validated_instruments, 0);
        assert!(
            found
                .warnings
                .contains(&Warning::InvalidInstrument { voice: 0 })
        );
        assert_eq!(found.instruments.len(), 1);
    }
    let mut rom = fixture();
    rom[0x200] = 0x40;
    let report = discover(&rom);
    assert_eq!(song(&report).confidence, Confidence::Unresolved);
    assert_eq!(song(&report).evidence.validated_instruments, 0);
    assert!(
        song(&report)
            .warnings
            .contains(&Warning::InvalidInstrument { voice: 0 })
    );
}

#[test]
fn base_psg_tones_validate_only_their_actual_data_shape() {
    for (kind, word_value) in [(1, 3), (2, 0), (3, 0x0800_0300), (4, 1)] {
        let mut rom = fixture();
        rom[0x200] = kind;
        put_word(&mut rom, 0x204, word_value);
        let report = discover(&rom);
        let found = song(&report);
        assert_eq!(found.confidence, Confidence::Structural, "kind {kind}");
        assert!(found.instruments[0].sample.is_none());
        assert_eq!(found.instruments[0].waveform.is_some(), kind == 3);
        if let Some(wave) = &found.instruments[0].waveform {
            assert_eq!((wave.effective_offset, wave.byte_len), (0x300, 16));
        }
        put_word(&mut rom, 0x204, u32::MAX);
        let bad = discover(&rom);
        assert_eq!(song(&bad).confidence, Confidence::Unresolved);
        assert!(
            song(&bad)
                .warnings
                .contains(&Warning::InvalidInstrument { voice: 0 })
        );
    }
}

#[test]
fn inventory_budget_bounds_many_headers_aliasing_large_voice_sets() {
    let mut rom = vec![0; 0x10000];
    for index in 0..40 {
        let header = 0x100 + index * 128;
        rom[header] = 24;
        put_word(&mut rom, header + 4, 0x0800_4000);
        for track in 0..24 {
            put_word(&mut rom, header + 8 + track * 4, 0x0800_8000);
        }
    }
    for voice in 0..=255 {
        rom[0x8000 + voice * 2] = 0xBD;
        rom[0x8001 + voice * 2] = voice as u8;
    }
    rom[0x8200..0x8204].copy_from_slice(&[0xD0, 60, 100, 0xB1]);
    let report = discover(&rom);
    assert_eq!(
        report.status,
        ScanStatus::Incomplete(ScanStop::InventoryLimit)
    );
    assert!(!report.candidates.is_empty() && report.candidates.len() < 40);
    assert!(report.work_used < report.limits.max_work);
    assert!(serde_json::to_vec(&report).unwrap().len() < 6 * 1024 * 1024);
}

#[test]
fn budget_cancel_candidate_and_media_limits_are_not_empty_success() {
    let rom = fixture();
    for (limits, cancel, expected) in [
        (
            ScanLimits {
                max_work: 0,
                ..ScanLimits::default()
            },
            false,
            ScanStop::WorkLimit,
        ),
        (ScanLimits::default(), true, ScanStop::Cancelled),
        (
            ScanLimits {
                max_candidates: 0,
                ..ScanLimits::default()
            },
            false,
            ScanStop::CandidateLimit,
        ),
        (
            ScanLimits {
                max_work: u64::MAX,
                ..ScanLimits::default()
            },
            false,
            ScanStop::InvalidLimits,
        ),
    ] {
        let report = scan(System::Gba, &rom, limits, &AtomicBool::new(cancel));
        assert_eq!(report.status, ScanStatus::Incomplete(expected));
        assert!(report.work_used <= limits.max_work);
        if cancel {
            assert!(report.media.sha256.is_none());
        }
    }
    let oversized = vec![0; MAX_ROM_BYTES + 1];
    let largest_supported = discover(&oversized[..MAX_ROM_BYTES]);
    assert_eq!(largest_supported.status, ScanStatus::Complete);
    assert!(largest_supported.candidates.is_empty());
    let report = discover(&oversized);
    assert_eq!(report.status, ScanStatus::Incomplete(ScanStop::MediaLimit));
    assert!(report.media.sha256.is_none());
    let report = scan(
        System::Gba,
        &[],
        ScanLimits::default(),
        &AtomicBool::new(true),
    );
    assert_eq!(report.status, ScanStatus::Incomplete(ScanStop::Cancelled));
}

#[test]
fn candidate_limit_retains_prior_complete_candidates() {
    let mut rom = fixture();
    collection(&mut rom, 0x800);
    let report = scan(
        System::Gba,
        &rom,
        ScanLimits {
            max_candidates: 1,
            ..ScanLimits::default()
        },
        &AtomicBool::new(false),
    );
    assert_eq!(
        report.status,
        ScanStatus::Incomplete(ScanStop::CandidateLimit)
    );
    assert_eq!(report.candidates.len(), 1);
    assert_eq!(report.candidates[0].header.effective_offset, 0x100);
}

#[test]
fn default_scan_covers_more_than_256_roots_and_explicit_limit_stays_partial() {
    let mut rom = vec![0; 261 * 0x1000];
    for index in 0..261 {
        collection(&mut rom, index * 0x1000 + 0x100);
    }
    let full = discover(&rom);
    assert_eq!(full.status, ScanStatus::Complete);
    assert_eq!(full.candidates.len(), 261);
    assert_eq!(
        full.candidates.last().unwrap().header.effective_offset,
        260 * 0x1000 + 0x100
    );
    let partial = scan(
        System::Gba,
        &rom,
        ScanLimits {
            max_candidates: 256,
            ..ScanLimits::default()
        },
        &AtomicBool::new(false),
    );
    assert_eq!(
        partial.status,
        ScanStatus::Incomplete(ScanStop::CandidateLimit)
    );
    assert_eq!(partial.candidates.len(), 256);
    assert_eq!(partial.candidates, full.candidates[..256]);
}

#[test]
fn applicable_cartridge_detectors_complete_even_without_a_match() {
    for system in [
        System::Gb,
        System::Gba,
        System::Nes,
        System::Sms,
        System::Gg,
        System::Sg,
        System::Pce,
        System::Ws,
        System::Coleco,
    ] {
        let report = scan(system, &[], ScanLimits::default(), &AtomicBool::new(false));
        assert_eq!(report.status, ScanStatus::Complete, "{system:?}");
        assert_eq!(report.song_count(), 0);
        assert!(
            report
                .applicable_detectors
                .iter()
                .any(|entry| entry.id == "tracker-structure")
        );
        assert_eq!(
            report.applicable_detectors.len(),
            match system {
                System::Gba => 15,
                System::Nes => 6,
                System::Gb => 11,
                System::Ws => 2,
                System::Sms | System::Gg => 2,
                _ => 1,
            }
        );
        let interrupted = scan(
            system,
            &[0; 256],
            ScanLimits {
                max_work: 0,
                ..ScanLimits::default()
            },
            &AtomicBool::new(false),
        );
        assert_eq!(
            interrupted.status,
            ScanStatus::Incomplete(ScanStop::WorkLimit)
        );
        assert_eq!(
            interrupted.applicable_detectors,
            report.applicable_detectors
        );
    }
}

#[test]
fn empty_and_random_inputs_complete_without_candidates() {
    for fill in [0, 0x80, 0xFF] {
        let report = discover(&vec![fill; 4096]);
        assert_eq!(report.status, ScanStatus::Complete);
        assert!(report.candidates.is_empty());
    }
    let mut value = 0x6D2B_79F5u32;
    let mut bytes = vec![0; 65536];
    for byte in &mut bytes {
        value ^= value << 13;
        value ^= value >> 17;
        value ^= value << 5;
        *byte = value as u8;
    }
    let report = discover(&bytes);
    assert_eq!(report.status, ScanStatus::Complete);
    assert!(report.candidates.is_empty());
    assert_eq!(discover(&[]).status, ScanStatus::Complete);
}

#[test]
fn all_fixture_truncations_and_single_byte_corruptions_are_bounded() {
    let rom = fixture();
    for len in 0..=0x450 {
        let report = discover(&rom[..len]);
        assert!(report.work_used <= report.limits.max_work);
        for candidate in &report.candidates {
            let mut spans = vec![&candidate.header];
            spans.extend(candidate.tracks.iter().flat_map(|track| &track.spans));
            for tone in &candidate.instruments {
                spans.push(&tone.descriptor);
                spans.extend(tone.sample_header.as_ref());
                spans.extend(tone.key_map.as_ref());
                if let Some(sample) = &tone.sample {
                    spans.extend([&sample.header, &sample.data]);
                }
                spans.extend(tone.waveform.as_ref());
                for region in &tone.regions {
                    if let Some(tone) = &region.tone {
                        spans.push(&tone.descriptor);
                        spans.extend(tone.sample_header.as_ref());
                        spans.extend(tone.waveform.as_ref());
                        if let Some(sample) = &tone.sample {
                            spans.extend([&sample.header, &sample.data]);
                        }
                    }
                }
            }
            for span in spans {
                assert!(span.effective_offset as usize + span.byte_len as usize <= len);
            }
        }
    }
    for at in [
        0x100, 0x101, 0x104, 0x105, 0x106, 0x107, 0x108, 0x200, 0x204, 0x300, 0x304, 0x308, 0x30C,
        0x400, 0x401, 0x406, 0x409,
    ] {
        for value in 0..=u8::MAX {
            let mut bytes = rom.clone();
            bytes[at] = value;
            let report = discover(&bytes);
            assert!(report.work_used <= report.limits.max_work);
        }
    }
}

#[test]
fn changed_effective_bytes_change_identity_without_mutating_input_bytes() {
    let mut rom = fixture();
    put_word(&mut rom, 0, 0xEAFF_FFFE);
    rom[0xB2] = 0x96;
    let before = rom.clone();
    let first = discover(&rom);
    let cancelled = scan(
        System::Gba,
        &rom,
        ScanLimits::default(),
        &AtomicBool::new(true),
    );
    assert_eq!(
        cancelled.status,
        ScanStatus::Incomplete(ScanStop::Cancelled)
    );
    assert_eq!(before, rom);
    rom[0x310] ^= 1;
    let patched = discover(&rom);
    assert_ne!(first.media.sha256, patched.media.sha256);
    assert_eq!(first.candidates, patched.candidates);
}
